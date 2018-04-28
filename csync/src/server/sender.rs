use std::io;

use futures::{Sink, Async, AsyncSink, Poll, StartSend};
use tokio::net::UdpSocket;

use codec::MTU;

pub struct Sender {
    socket: UdpSocket,
    packet: Option<([u8; MTU], usize)>,
}

impl Sender {
    pub fn new(socket: UdpSocket) -> Sender {
        Sender {
            socket,
            packet: None,
        }
    }
}

impl Sink for Sender {
    type SinkItem = ([u8; MTU], usize);
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<([u8; MTU], usize), Self::SinkError> {
        if self.packet.is_some() {
            match self.poll_complete()? {
                Async::Ready(()) => {},
                Async::NotReady => return Ok(AsyncSink::NotReady(item)),
            }
        }

        self.packet = Some(item);
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        if self.packet.is_none() {
            return Ok(Async::Ready(()))
        }

        let written = {
            let &(ref buf, len) = self.packet.as_ref().unwrap();
            try_ready!(self.socket.poll_send(&buf[..len]))
        };

        if written == self.packet.take().unwrap().1 {
            Ok(Async::Ready(()))
        } else {
            Err(io::Error::new(io::ErrorKind::Other,
                               "failed to write entire datagram to socket").into())
        }
    }

    fn close(&mut self) -> Poll<(), io::Error> {
        try_ready!(self.poll_complete());
        Ok(().into())
    }}