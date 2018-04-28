use std::io;

use futures::{Stream, Async, Poll};
use futures::sync::mpsc::UnboundedSender;
use tokio::net::UdpSocket;

use codec::{MTU, Login, Command, UploadRequest, Chunk};

pub struct Receiver {
    state: State,
    socket: UdpSocket,
    tx: UnboundedSender<([u8; MTU], usize)>,
}

pub enum State {
    WaitForAck,
    WaitForCommand,
    WaitForChunk(WaitForChunk),
}

#[derive(Clone, Copy)]
pub struct WaitForChunk {
    index_field_size: usize,
    len_left: usize,
}

impl Receiver {
    pub fn new(socket: UdpSocket, login: Login, tx: UnboundedSender<([u8; MTU], usize)>) -> Receiver {
        let mut receiver = Receiver {
            state: State::WaitForAck,
            socket,
            tx,
        };
        receiver.login(login);
        receiver
    }

    pub fn login(&mut self, login: Login) {
        self.state = State::WaitForAck;
        self.tx.unbounded_send(([0u8; MTU], 0)).unwrap();
        unimplemented!()
    }

    pub fn ack(&mut self) {
        self.state = State::WaitForCommand;
        unimplemented!()
    }

    pub fn upload_request(&mut self, req: UploadRequest) {
        self.state = State::WaitForChunk(WaitForChunk {
            index_field_size: index_field_size(MTU, req.length),
            len_left: req.length,
        });
        unimplemented!()
    }

    pub fn chunk(&mut self, chunk: Chunk) {
        unimplemented!()
    }
}

impl Stream for Receiver {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        let mut buf = [0u8; MTU];
        let size = match self.socket.poll_recv(&mut buf) {
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(size)) => size,
            Err(e) => return Err(e),
        };

        let buf = &mut buf[..size];

        match self.state {
            State::WaitForAck => self.ack(),
            State::WaitForCommand => match Command::decode(buf) {
                Ok(Command::UploadRequest(req)) => self.upload_request(req),
                Err(e) => return Err(io::Error::new(io::ErrorKind::InvalidData, e)),
            },
            State::WaitForChunk(state) => self.chunk(Chunk::decode(buf, state.index_field_size)),
        }
        Ok(Async::Ready(Some(())))
    }
}

fn index_field_size(mtu: usize, length: usize) -> usize {
    let mut index_field_size = 1;
    loop {
        let size = mtu - index_field_size;
        let num = (length + size - 1) / size;
        if num <= 1 << (index_field_size * 8 - 1) {
            return index_field_size;
        }
        index_field_size += 1;
    }
}
