use std::io;
use std::sync::{Arc, Mutex};
use std::path::PathBuf;
use std::fs;

use futures::{Sink, Async, AsyncSink, Poll, StartSend};
use tokio::net::UdpSocket;
use memmap::MmapMut;
use bitte_ein_bit::BitMap;

use codec::{self, MTU};
use server::ChannelMessage;

pub struct Sender {
    socket: UdpSocket,
    vec: Vec<u8>,
    bitmap: Option<Arc<Mutex<BitMap<MmapMut>>>>,
    bitmap_path: Option<PathBuf>,
    state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Sending,
    Waiting,
}

impl Sender {
    pub fn new(socket: UdpSocket) -> Sender {
        Sender {
            socket,
            vec: vec![0u8; MTU],
            bitmap: None,
            bitmap_path: None,
            state: State::Waiting,
        }
    }
}

impl Sink for Sender {
    type SinkItem = ChannelMessage;
    type SinkError = io::Error;

    fn start_send(&mut self, item: ChannelMessage) -> StartSend<ChannelMessage, Self::SinkError> {
        if self.state == State::Sending {
            match self.poll_complete()? {
                Async::Ready(()) => {},
                Async::NotReady => return Ok(AsyncSink::NotReady(item)),
            }
        }

        match item {
            ChannelMessage::UploadStart(bitmap, bitmap_path) => {
                if self.bitmap.is_some() {
                    panic!("Bitmap is already some");
                }
                self.bitmap = Some(bitmap);
                self.bitmap_path = Some(bitmap_path);
            }
            ChannelMessage::UploadStatus => {
                self.vec.resize(MTU, 0u8);
                let bitmap = self.bitmap.as_ref().unwrap().lock().unwrap();
                let size = codec::write_runlength_encoded(&bitmap, &mut self.vec[..]).unwrap();
                self.vec.truncate(size);
                trace!("Sending UploadStatus: {:?}", self.vec);
                if bitmap.all() && self.bitmap_path.as_ref().unwrap().exists() {
                    info!("Remove bitmap file");
                    fs::remove_file(self.bitmap_path.as_ref().unwrap()).unwrap();
                }
                self.state = State::Sending;
            }
        }
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        if self.state == State::Waiting {
            return Ok(Async::Ready(()))
        }

        let written = try_ready!(self.socket.poll_send(&self.vec));

        if written == self.vec.len() {
            self.state = State::Waiting;
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