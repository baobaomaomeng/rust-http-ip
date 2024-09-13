use std::collections::HashMap;
use std::io;
use std::net::Ipv4Addr;

mod tcp;

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
struct Quad {
    src: (Ipv4Addr, u16),
    dst: (Ipv4Addr, u16),
}
fn is_little_endian() -> bool {
    let num: u32 = 0x01020304;
    let bytes = num.to_le_bytes();
    bytes[0] == 0x04
}
fn main() -> io::Result<()> {
    if is_little_endian() {
        println!("This system is little-endian.");
    } else {
        println!("This system is big-endian.");
    }
    let mut connections: HashMap<Quad, tcp::Connection> = Default::default();
    let mut nic =
        tun_tap::Iface::without_packet_info("tun0", tun_tap::Mode::Tun)?;
    let mut buf = [0u8; 1504];
    loop {
        let nbytes = nic.recv(&mut buf[..])?;
        //let flags = u16::from_be_bytes([buf[0], buf[1]]);
        // let proto = u16::from_be_bytes([buf[2], buf[3]]);
        // if proto != 0x0800 {
        //     //no ipv4
        //     eprintln!("recive ipv6 {}",proto);
        //     continue;
        // }
        eprintln!("recive ipv4");
        match etherparse::Ipv4HeaderSlice::from_slice(&buf[..nbytes]) {
            Ok(ip_header) => {
                let src = ip_header.source_addr();
                let dst = ip_header.destination_addr();
                let proto = ip_header.protocol();

                if proto != etherparse::IpNumber(0x06) {
                    // not tcp
                    eprintln!("not tcp {}",proto.0);
                    continue;
                }

                match etherparse::TcpHeaderSlice::from_slice(&buf[ip_header.slice().len()..nbytes]) {
                    Ok(tcp_header) => {
                        use std::collections::hash_map::Entry;
                        let data_begin = ip_header.slice().len() + tcp_header.slice().len();
                        match connections.entry(Quad {
                            src: (src, tcp_header.source_port()),
                            dst: (dst, tcp_header.destination_port()),
                        }) {
                            Entry::Occupied(mut c) => {
                                c.get_mut().on_packet(&mut nic,ip_header, tcp_header, &buf[data_begin..nbytes]);
                            }
                            Entry::Vacant(e) => {
                                if let Some(c) =  tcp::Connection::accept(
                                    &mut nic,  
                                    ip_header,
                                    tcp_header,
                                    &buf[data_begin..nbytes],
                                )? {
                                    e.insert(c);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!(
                            "someting error when prase ipv4Header {:?},ignoring packet ",
                            e
                        );
                    }
                }
            }
            Err(e) => {
                eprintln!(
                    "someting error when prase ipv4Header {:?},ignoring packet ",
                    e
                );
            }
        }
    }
}
