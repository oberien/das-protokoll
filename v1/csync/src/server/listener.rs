use std::io;
use std::net::SocketAddr;
use std::mem;

use futures::{Stream, Poll, Async};
use tokio::net::UdpSocket;

use codec::MTU;

pub struct Listener {
    socket: UdpSocket,
    buf: [u8; MTU],
}

impl Listener {
    pub fn new(socket: UdpSocket) -> Listener {
        Listener {
            socket,
            buf: [0u8; MTU],
        }
    }
}

impl Stream for Listener {
    type Item = ([u8; MTU], usize, SocketAddr);
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<(Self::Item)>, Self::Error> {
        let (read, addr) = try_ready!(self.socket.poll_recv_from(&mut self.buf));
        let buf = mem::replace(&mut self.buf, [0u8; MTU]);
        Ok(Async::Ready(Some((buf, read, addr))))
    }
}