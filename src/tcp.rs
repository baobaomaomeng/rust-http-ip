use etherparse::err::ip;
use std::io;

enum State {
    //Listen,
    SynRcvd,
    Estab,
    FinWait1,
    FinWait2,
    TimeWait,
    //CloseWait
    //LastAck
    //Closing
}

impl State {
    fn is_synchronized(&self) -> bool {
        match *self {
            State::SynRcvd => false,
            State::Estab | State::FinWait1 | State::FinWait2 | State::TimeWait => true,
        }
    }
}

pub struct Connection {
    state: State,
    send: SendSequenceSpace,
    rcv: RecvSequenceSpace,
    ip: etherparse::Ipv4Header,
    tcp: etherparse::TcpHeader,
}

/// State of the Send Sequence Space (RFC 793 S3.2 F4)
///
/// ```
///            1         2          3          4
///       ----------|----------|----------|----------
///              SND.UNA    SND.NXT    SND.UNA
///                                   +SND.WND
///
/// 1 - old sequence numbers which have been acknowledged
/// 2 - sequence numbers of unacknowledged data
/// 3 - sequence numbers allowed for new data transmission
/// 4 - future sequence numbers which are not yet allowed
/// ```
struct SendSequenceSpace {
    /// send unacknowledged
    una: u32,
    /// send next
    nxt: u32,
    /// send window
    wnd: u16,
    /// send urgent pointer
    up: bool,
    /// segment sequence number used for last window update
    wl1: usize,
    /// segment acknowledgment number used for last window update
    wl2: usize,
    /// initial send sequence number
    iss: u32,
}

/// State of the Receive Sequence Space (RFC 793 S3.2 F5)
///
/// ```
///                1          2          3
///            ----------|----------|----------
///                   RCV.NXT    RCV.NXT
///                             +RCV.WND
///
/// 1 - old sequence numbers which have been acknowledged
/// 2 - sequence numbers allowed for new reception
/// 3 - future sequence numbers which are not yet allowed
/// ```
struct RecvSequenceSpace {
    /// receive next
    nxt: u32,
    /// receive window
    wnd: u16,
    /// receive urgent pointer
    up: bool,
    /// initial receive sequence number
    irs: u32,
}

impl Connection {
    fn write(&mut self, nic: &mut tun_tap::Iface, payload: &[u8]) -> io::Result<usize> {
        let mut buf = [0u8; 1500];
        self.tcp.sequence_number = self.send.nxt;
        self.tcp.acknowledgment_number = self.rcv.nxt;

        let size = std::cmp::min(
            buf.len(),
            self.tcp.header_len() as usize + self.ip.header_len() as usize + payload.len(),
        );
        let _ = self
            .ip
            .set_payload_len(size - self.ip.header_len() as usize);

        // the kernel is nice and does this for us
        self.tcp.checksum = self
            .tcp
            .calc_checksum_ipv4(&self.ip, &[])
            .expect("failed to compute checksum");

        // write out the headers
        use std::io::Write;
        let mut unwritten = &mut buf[..];
        let _ = self.ip.write(&mut unwritten);
        let _ = self.tcp.write(&mut unwritten);
        let payload_bytes = unwritten.write(payload)?;
        let unwritten = unwritten.len();
        self.send.nxt = self.send.nxt.wrapping_add(payload_bytes as u32);
        if self.tcp.syn {
            self.send.nxt = self.send.nxt.wrapping_add(1);
            self.tcp.syn = false;
        }
        if self.tcp.fin {
            self.send.nxt = self.send.nxt.wrapping_add(1);
            self.tcp.fin = false;
        }
        nic.send(&buf[..buf.len() - unwritten])?;
        Ok(payload_bytes)
    }

    pub fn accept<'a>(
        nic: &mut tun_tap::Iface,
        ip_header: etherparse::Ipv4HeaderSlice<'a>,
        tcp_header: etherparse::TcpHeaderSlice<'a>,
        data: &'a [u8],
    ) -> io::Result<Option<Self>> {
        let mut buf = [0u8; 1500];
        if !tcp_header.syn() {
            return Ok(None);
        }

        let iss = 0;
        let wnd = 1024;

        let mut syn_ack = etherparse::TcpHeader::new(
            tcp_header.destination_port(),
            tcp_header.source_port(),
            iss,
            wnd,
        );
        syn_ack.acknowledgment_number = tcp_header.sequence_number() + 1;
        syn_ack.syn = true;
        syn_ack.ack = true;

        let connection = Connection {
            state: State::SynRcvd,
            send: SendSequenceSpace {
                iss,
                una: iss,
                nxt: iss + 1,
                wnd,

                up: false,
                wl1: 0,
                wl2: 0,
            },
            rcv: RecvSequenceSpace {
                irs: tcp_header.sequence_number(),
                nxt: tcp_header.sequence_number() + 1,
                wnd: tcp_header.window_size(),
                up: false,
            },
            tcp: etherparse::TcpHeader::new(
                tcp_header.destination_port(),
                tcp_header.source_port(),
                iss,
                wnd,
            ),
            ip: etherparse::Ipv4Header::new(
                syn_ack.header_len().try_into().unwrap(),
                64,
                etherparse::IpNumber::TCP,
                ip_header.destination_addr().octets(),
                ip_header.source_addr().octets(),
            )
            .unwrap(),
        };

        let unwritten = {
            let mut unwritten = &mut buf[..];
            connection.ip.write(&mut unwritten)?;
            syn_ack.write(&mut unwritten)?;
            unwritten.len()
        };
        nic.send(&buf[..(buf.len() - unwritten)])?;
        Ok(Some(connection))
    }

    pub fn on_packet<'a>(
        &mut self,
        nic: &mut tun_tap::Iface,
        iph: etherparse::Ipv4HeaderSlice<'a>,
        tcph: etherparse::TcpHeaderSlice<'a>,
        data: &'a [u8],
    ) -> io::Result<()> {
        let seq = tcph.sequence_number();
        let mut len = data.len() as u32;

        if tcph.fin() {
            len += 1;
        };
        if tcph.syn() {
            len += 1;
        };
        let ok = self.check(seq, len);

        if !ok {
            self.write(nic, &[])?;
            return Ok(());
        }
        self.rcv.nxt = seq.wrapping_add(len);

        if !tcph.ack() {
            return Ok(());
        }
        let ackn = tcph.acknowledgment_number();
        if let State::SynRcvd = self.state {
            if Self::is_between_wrapped(
                self.send.una.wrapping_sub(1),
                ackn,
                self.send.nxt.wrapping_add(1),
            ) {
                // must have ACKed our SYN, since we detected at least one acked byte,
                // and we have only sent one byte (the SYN).
                self.state = State::Estab;
            } else {
                // TODO: <SEQ=SEG.ACK><CTL=RST>
            }
        }

        if let State::Estab | State::FinWait1 | State::FinWait2 = self.state {
            if !Self::is_between_wrapped(self.send.una, ackn, self.send.nxt.wrapping_add(1)) {
                return Ok(());
            }
            self.send.una = ackn;
            // TODO
            assert!(data.is_empty());

            if let State::Estab = self.state {
                // now let's terminate the connection!
                // TODO: needs to be stored in the retransmission queue!
                self.tcp.fin = true;
                self.write(nic, &[])?;
                self.state = State::FinWait1;
            }
        }

        if let State::FinWait1 = self.state {
            if self.send.una == self.send.iss + 2 {
                // our FIN has been ACKed!
                self.state = State::FinWait2;
            }
        }

        if tcph.fin() {
            match self.state {
                State::FinWait2 => {
                    // we're done with the connection!
                    self.write(nic, &[])?;
                    self.state = State::TimeWait;
                }
                _ => unimplemented!(),
            }
        }

        Ok(())
    }

    fn check(&self, seq: u32, len: u32) -> bool {
        //发送窗口的最后一个字节数据为接收的nxt+接收窗口大小
        let wend = self.rcv.nxt.wrapping_add(self.rcv.wnd as u32);
        if len == 0 {
            // zero-length segment has separate rules for acceptance
            if self.rcv.wnd == 0 {
                if seq != self.rcv.nxt {
                    false
                } else {
                    true
                }
            } else if !Self::is_between_wrapped(self.rcv.nxt.wrapping_sub(1), seq, wend) {
                false
            } else {
                true
            }
        } else {
            if self.rcv.wnd == 0 {
                false
            } else if !Self::is_between_wrapped(self.rcv.nxt.wrapping_sub(1), seq, wend)
                && !Self::is_between_wrapped(
                    self.rcv.nxt.wrapping_sub(1),
                    seq.wrapping_add(len - 1),
                    wend,
                )
            {
                false
            } else {
                true
            }
        }
    }

    fn send_rst(&mut self, nic: &mut tun_tap::Iface) -> io::Result<()> {
        self.tcp.rst = true;
        // TODO: fix sequence numbers here
        // If the incoming segment has an ACK field, the reset takes its
        // sequence number from the ACK field of the segment, otherwise the
        // reset has sequence number zero and the ACK field is set to the sum
        // of the sequence number and segment length of the incoming segment.
        // The connection remains in the same state.
        //
        // TODO: handle synchronized RST
        // 3.  If the connection is in a synchronized state (ESTABLISHED,
        // FIN-WAIT-1, FIN-WAIT-2, CLOSE-WAIT, CLOSING, LAST-ACK, TIME-WAIT),
        // any unacceptable segment (out of window sequence number or
        // unacceptible acknowledgment number) must elicit only an empty
        // acknowledgment segment containing the current send-sequence number
        // and an acknowledgment indicating the next sequence number expected
        // to be received, and the connection remains in the same state.
        self.tcp.sequence_number = 0;
        self.tcp.acknowledgment_number = 0;
        self.write(nic, &[])?;
        Ok(())
    }

    //在u32 max为模的情况下，判断三个数的位置
    fn is_between_wrapped(start: u32, x: u32, end: u32) -> bool {
        use std::cmp::Ordering;
        match start.cmp(&x) {
            Ordering::Equal => return false,
            Ordering::Less => {
                // we have:
                //
                //   0 |-------------S------X---------------------| (wraparound)
                //
                // X is between S and E (S < X < E) in these cases:
                //
                //   0 |-------------S------X---E-----------------| (wraparound)
                //
                //   0 |----------E--S------X---------------------| (wraparound)
                //
                // but *not* in these cases
                //
                //   0 |-------------S--E---X---------------------| (wraparound)
                //
                //   0 |-------------|------X---------------------| (wraparound)
                //                   ^-S+E
                //
                //   0 |-------------S------|---------------------| (wraparound)
                //                      X+E-^
                //
                // or, in other words, iff !(S <= E <= X)
                if end >= start && end <= x {
                    return false;
                }
            }
            Ordering::Greater => {
                // we have the opposite of above:
                //
                //   0 |-------------X------S---------------------| (wraparound)
                //
                // X is between S and E (S < X < E) *only* in this case:
                //
                //   0 |-------------X--E---S---------------------| (wraparound)
                //
                // but *not* in these cases
                //
                //   0 |-------------X------S---E-----------------| (wraparound)
                //
                //   0 |----------E--X------S---------------------| (wraparound)
                //
                //   0 |-------------|------S---------------------| (wraparound)
                //                   ^-X+E
                //
                //   0 |-------------X------|---------------------| (wraparound)
                //                      S+E-^
                //
                // or, in other words, iff S < E < X
                if end < start && end > x {
                } else {
                    return false;
                }
            }
        }
        true
    }
}
