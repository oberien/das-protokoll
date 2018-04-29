use std::io::{self, Seek, SeekFrom};
use std::mem;
use std::time::{Instant, Duration};
use std::fs::{self, File as StdFile};
use std::path::{PathBuf, Path};

use futures::{Stream, Async, Poll};
use futures::sync::mpsc::UnboundedSender;
use tokio::io::AsyncWrite;
use tokio::net::UdpSocket;
use tokio::reactor::{PollEvented2, Handle};
use tokio_file_unix::File;
use ring::digest;
use hex::ToHex;
use take_mut;

use codec::{self, MTU, Login, Command, UploadRequest, Chunk};

pub struct Receiver {
    state: State,
    socket: UdpSocket,
    tx: UnboundedSender<([u8; MTU], usize)>,
    rtt: Duration,
    folder: PathBuf,
}

pub enum State {
    Invalid,
    WaitForAckAndCommand(WaitForAckAndCommand),
    WaitForChunk(WaitForChunk),
    WritingChunk(WritingChunk),
}

#[derive(Clone, Copy)]
pub struct WaitForAckAndCommand {
    sent: Instant,
}

pub struct WaitForChunk {
    file: PollEvented2<File<StdFile>>,
    index_field_size: usize,
    len_left: usize,
}

pub struct WritingChunk {
    old: WaitForChunk,
    chunk: Chunk,
}

impl Receiver {
    pub fn new(socket: UdpSocket, login: Login, tx: UnboundedSender<([u8; MTU], usize)>) -> Receiver {
        tx.unbounded_send(([0u8; MTU], 0)).unwrap();
        debug!("Login: {:?}", login);
        let sha = digest::digest(&digest::SHA256, login.client_token);
        let mut hex = String::with_capacity(digest::SHA256_OUTPUT_LEN * 2);
        sha.as_ref().write_hex(&mut hex).unwrap();
        let mut path = PathBuf::from("./files/");
        path.push(hex);
        debug!("Folder: {}", path.display());
        Receiver {
            state: State::WaitForAckAndCommand(WaitForAckAndCommand {
                sent: Instant::now(),
            }),
            socket,
            tx,
            rtt: Duration::from_secs(0),
            folder: path,
        }
    }

    pub fn ack(&mut self, sent: Instant, command: Command) {
        self.rtt = Instant::now() - sent;
        debug!("Ack from Client, RTT {}ms", self.rtt.as_secs() * 1000 + self.rtt.subsec_nanos() as u64 / 1_000_000);

        match command {
            Command::UploadRequest(req) => self.upload_request(req),
        }
    }


    pub fn upload_request(&mut self, req: UploadRequest) {
        debug!("upload request: {:?}", req);
        // TODO: handle connection abort / write bitvec to disk
        let mut req_path = Path::new(req.path);
        if req_path.has_root() {
            req_path = req_path.strip_prefix("/").unwrap();
        }
        let mut path = self.folder.join(req_path);
        fs::create_dir_all(&path).unwrap();
        path.push("file");
        let file = StdFile::create(path).unwrap();
        self.state = State::WaitForChunk(WaitForChunk {
            file: File::new_nb(file).unwrap().into_io(&Handle::current()).unwrap(),
            index_field_size: codec::index_field_size(MTU, req.length),
            len_left: req.length,
        });
    }

    pub fn chunk(&mut self, chunk: Chunk) {
        take_mut::take_or_recover(&mut self.state, || State::Invalid, |old| {
            let old = if let State::WaitForChunk(old) = old { old } else { unreachable!() };
            State::WritingChunk(WritingChunk {
                old,
                chunk,
            })
        });
        // TODO: Continue old upload
        // TODO: Handle existing completed files
        // TODO: length check of chunks to ensure max usage of MTU
        // TODO: BitVec
        // TODO: Write BitVec to Disk
    }
}

impl Stream for Receiver {
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let State::WritingChunk(_) = self.state {
            let state = mem::replace(&mut self.state, State::Invalid);
            let mut state = if let State::WritingChunk(state) = state { state } else { unreachable!() };

            let written = {
                let buf = &state.chunk.data[state.chunk.offset..state.chunk.size];
                let start = state.chunk.index * MTU as u64;
                state.old.file.get_mut().seek(SeekFrom::Start(start))?;
                try_ready!(state.old.file.poll_write(buf))
            };
            state.chunk.offset += written;
            state.old.len_left -= written;
            // still stuff to write
            if state.chunk.offset != state.chunk.size {
                self.state = State::WritingChunk(state);
                return Ok(Async::NotReady);
            } else {
                // last chunk
                if state.chunk.size != MTU {
                    // close connection
                    return Ok(Async::Ready(None));
                } else {
                    self.state = State::WaitForChunk(state.old)
                }
            }
        }
        if let State::Invalid = self.state {
            unreachable!();
        }

        let mut buf = [0u8; MTU];
        let size = match self.socket.poll_recv(&mut buf) {
            Ok(Async::NotReady) => return Ok(Async::NotReady),
            Ok(Async::Ready(size)) => size,
            Err(e) => return Err(e),
        };

        debug!("Got Message {:?}", &buf[..size]);

        match self.state {
            State::Invalid => unreachable!(),
            State::WaitForAckAndCommand(state) => {
                let command = Command::decode(&mut buf[..size])
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
                self.ack(state.sent, command)
            },
            State::WaitForChunk(WaitForChunk { index_field_size, .. }) =>
                self.chunk(Chunk::decode(buf, size, index_field_size)),
            State::WritingChunk(_) => unreachable!(),
        }
        Ok(Async::Ready(Some(())))
    }
}

