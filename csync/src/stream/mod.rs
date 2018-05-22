use std::os::unix::io::{AsRawFd, FromRawFd};
use std::net::{UdpSocket as StdUdpSocket, SocketAddr};
use std::ops::Range;
use std::collections::VecDeque;

use futures::Future;
use tokio::net::UdpSocket;
use tokio::reactor::Handle;
use varmint;

use codec;

mod packet;

use stream::packet::Packet;

pub struct Connection<F: Fn(&mut Packet)> {
    socket: UdpSocket,
    to: SocketAddr,
    next_id: u64,
    streams: Vec<(Range<u64>, FileStream<F>)>,
}

pub struct FileStream<F: Fn(&mut Packet)> {
    ids: Range<u64>,
    next_id: u64,
    packet_fn: F,
}

pub struct FileChunk {
    id: u64,
    data: Vec<u8>,
}

pub struct ChunkInfo {
    id: u64,
}

impl ChunkInfo {
    pub fn new(id: u64) -> ChunkInfo {
        ChunkInfo { id }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn id_field_length(&self) -> usize {
        varmint::len_u64_varint(self.id)
    }

    pub fn data_length(&self) -> usize {
        codec::MTU - self.id_field_length()
    }

    pub fn data_since(&self, start: u64) -> u64 {
        let mut sum = 0;
        for val in start..self.id {
            sum += (codec::MTU - varmint::len_u64_varint(val)) as u64;
        }
        sum
    }

    pub fn id_until(&self, size: u64) -> u64 {
        let mut sum = 0;
        for val in self.id.. {
            sum += (codec::MTU - varmint::len_u64_varint(val)) as u64;
            if sum > size {
                return val;
            }
        }
        unreachable!()
    }
}

impl<F: Fn(&mut Packet)> Connection<F> {
    pub fn new(to: SocketAddr) -> Connection<F> {
        let socket = StdUdpSocket::bind("0.0.0.0:21088").unwrap();
        socket.connect(to).unwrap();
        let socket = UdpSocket::from_std(socket, &Handle::current()).unwrap();
        Connection {
            socket,
            to,
            next_id: 0,
            streams: Vec::new(),
        }
    }

    // TODO: Remove this because it doesn't use next_id
    pub fn send_next(self, data: Vec<u8>) -> impl Future<Item = (Self, Vec<u8>)> {
        let Connection { socket, to, next_id, streams } = self;
        socket.send_dgram(data, &self.to)
            .map(move |(socket, data)| {
                (Connection {
                    socket,
                    to,
                    next_id: next_id + 1,
                    streams,
                }, data)
            })
    }

    pub fn file_stream(self, packet_fn: F, file_size: u64, file_name: &str) -> impl Future<Item = Self> {
        let info = ChunkInfo::new(self.next_id + 1);
        let ids = info.id()..info.id_until(file_size);
        self.streams.push((ids, FileStream {
            ids,
            next_id: ids.start,
            packet_fn,
        }));
        let packet =
        let Connection { socket, to, next_id, streams } = self;
    }
}

fn dup(socket: UdpSocket) -> UdpSocket {
    let new = unsafe { StdUdpSocket::from_raw_fd(socket.as_raw_fd()) };
    UdpSocket::from_std(new, &Handle::current()).unwrap()
}
