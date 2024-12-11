use std::collections::{HashMap, VecDeque};
use std::io;
use std::io::prelude::*;
use std::net::Ipv4Addr;
use std::os::unix::thread;
use std::sync::{Arc, Condvar, Mutex};
mod tcp;

#[derive(Clone, Copy, Debug, Hash, Eq, PartialEq)]
struct Quad {
    src: (Ipv4Addr, u16),
    dst: (Ipv4Addr, u16),
}

#[derive(Default)]
struct ConnectionManager {
    terminate: bool,
    connections: HashMap<Quad, tcp::Connection>,
    pending: HashMap<u16, VecDeque<Quad>>,
}

struct InterfacHandle {
    manager: Mutex<ConnectionManager>,
    pending_var: Condvar,
}

type  InterfacHandleSafeRef = Arc<InterfacHandle>;

pub struct TcpStream {
    quad: Quad,
    ih: InterfacHandleSafeRef
}

impl Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        todo!()
    }
}
impl Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        todo!()
    }

    fn flush(&mut self) -> std::io::Result<()> {
        todo!()
    }
}

impl TcpStream {
    pub fn shutdown(&self, how: std::net::Shutdown) -> io::Result<()> {
        // TODO: 在指定的连接上发送fin
       todo!()
    }
}


pub struct TcpListener{
    port:u16,
    h: InterfacHandleSafeRef
}

impl Drop for TcpListener {
    fn drop(&mut self) {
        todo!()
    }
}

impl  TcpListener {
    pub fn  accept(&mut self) -> io::Result<()> {
        todo!()
    }
}

pub struct Interface{
    ih: Option<InterfacHandleSafeRef>,
    jh: Option<thread::JoinHandle<io::Result<()>>>
}

impl Drop for Interface {
    fn drop(&mut self) {
        todo!()
    }
}

impl Interface {
    pub fn new() ->io::Result<Self>{
        todo!()
    }

    pub fn bind(&mut self,port: u16) -> io::Result<TcpListener>{
        todo!()
    }
}