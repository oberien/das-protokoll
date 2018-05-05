use std::io::{Seek, SeekFrom, Error as IoError, ErrorKind};
use std::mem;
use std::time::{Instant, Duration};
use std::fs::{self, File as StdFile, OpenOptions};
use std::path::{PathBuf, Path};
use std::os::unix::fs::MetadataExt;

use futures::{Stream, Async, Poll, Future};
use futures::sync::mpsc::UnboundedSender;
use tokio::io::{self, AsyncWrite};
use tokio::net::UdpSocket;
use tokio::reactor::{PollEvented2, Handle};
use tokio_file_unix::File;
use ring::digest;
use hex::ToHex;
use take_mut;
use bitte_ein_bit::BitMap;
use memmap::{MmapMut, MmapOptions};

use codec::{self, MTU, Login, Command, UploadRequest, Chunk, ChunkInfo};

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

pub struct WaitForAckAndCommand {
    sent: Instant,
}

pub struct WaitForChunk {
    file: PollEvented2<File<StdFile>>,
    bitmap: BitMap<MmapMut>,
    buf: Vec<u8>,
    chunk_info: ChunkInfo,
}

type WriteChunk = io::WriteAll<PollEvented2<File<StdFile>>, Chunk>;

pub struct WritingChunk {
    bitmap: BitMap<MmapMut>,
    chunk_info: ChunkInfo,
    future: WriteChunk,
}

impl Receiver {
    pub fn new(socket: UdpSocket, login: Login, tx: UnboundedSender<([u8; MTU], usize)>) -> Receiver {
        tx.unbounded_send(([0u8; MTU], 0)).unwrap();
        debug!("Login");
        trace!("Client Token: {:?}", login);
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

        let chunk_info = codec::index_field_size(req.length);
        let mut req_path = Path::new(req.path);
        if req_path.has_root() {
            req_path = req_path.strip_prefix("/").unwrap();
        }
        let mut path = self.folder.join(req_path);
        fs::create_dir_all(&path).unwrap();
        let file_path = path.join("file");
        let bitmap_path = path.join("bitmap");

        let continue_upload = bitmap_path.exists();

        let bitmap_file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(bitmap_path)
            .unwrap();

        let bitmap_file_len = (chunk_info.num_chunks + 7) / 8;
        if !continue_upload {
            debug!("New File");
            bitmap_file.set_len(bitmap_file_len).unwrap();
        }

        let mmap = unsafe {
            MmapOptions::new()
                .map_mut(&bitmap_file)
                .unwrap()
        };
        let mut bitmap = BitMap::with_length(mmap, chunk_info.num_chunks);


        let mut file = OpenOptions::new();
        if continue_upload {
            debug!("Continue Upload file: {:x?}", bitmap);
            if bitmap.all() {
                warn!("Continue upload, but all chunks are already received???");
                bitmap.reset();
            }
            // TODO: Send RLE bitmap to client
            file.append(true);
        }
        let file = file.create(true)
            .write(true)
            .open(file_path).unwrap();
        file.set_len(req.length as u64).unwrap();

        self.state = State::WaitForChunk(WaitForChunk {
            file: File::new_nb(file).unwrap().into_io(&Handle::current()).unwrap(),
            bitmap,
            buf: Vec::with_capacity(MTU),
            chunk_info,
        });
    }

    pub fn chunk(&mut self, chunk: Chunk, chunk_info: ChunkInfo,
                 mut file: PollEvented2<File<StdFile>>, bitmap: BitMap<MmapMut>) {
        if bitmap.get(chunk.index) {
            trace!("Chunk {} already received, skipping", chunk.index);
            self.state = State::WaitForChunk(WaitForChunk {
                file,
                bitmap,
                buf: chunk.into_vec(),
                chunk_info: chunk_info,
            });
            return;
        }
        trace!("Switch to WritingChunk with len {}", chunk.as_ref().len());
        file.get_mut().seek(SeekFrom::Start(chunk.index * chunk_info.chunk_size)).unwrap();
        let future = io::write_all(file, chunk);
        self.state = State::WritingChunk(WritingChunk {
            bitmap,
            chunk_info,
            future,
        });
        // TODO: Continue old upload
        // TODO: Handle existing completed files
        // TODO: length check of chunks to ensure max usage of MTU
    }
}

impl Stream for Receiver {
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        if let State::WritingChunk(_) = self.state {
            trace!("Try writing Chunk");
            let (file, chunk) = {
                let state = if let State::WritingChunk(ref mut state) = self.state { state } else { unreachable!() };
                try_ready!(state.future.poll())
            };
            trace!("Chunk written");

            let state = mem::replace(&mut self.state, State::Invalid);
            let mut state = if let State::WritingChunk(state) = state { state } else { unreachable!() };
            state.bitmap.set(chunk.index, true);

            // if last chunk
            if state.bitmap.all() {
                info!("Last chunk received. Closing Connection");
                // TODO: remove bitmap file
                // close connection
                return Ok(Async::Ready(None));
            }
            trace!("Switch to WaitForChunk");
            self.state = State::WaitForChunk(WaitForChunk {
                file,
                buf: chunk.into_vec(),
                bitmap: state.bitmap,
                chunk_info: state.chunk_info,
            });
        }

        match self.state {
            State::Invalid => unreachable!(),
            State::WaitForAckAndCommand(WaitForAckAndCommand { sent }) => {
                let mut buf = [0u8; MTU];
                let size = try_ready!(self.socket.poll_recv(&mut buf));
                let command = Command::decode(&mut buf[..size])
                    .map_err(|e| IoError::new(ErrorKind::InvalidData, e))?;
                self.ack(sent, command)
            },
            State::WaitForChunk(_) => {
                let size = if let State::WaitForChunk(WaitForChunk { ref mut buf, .. }) = self.state {
                    buf.resize(MTU, 0);
                    let size = try_ready!(self.socket.poll_recv(buf));
                    buf.truncate(size);
                    size
                } else { unreachable!() };
                let state = mem::replace(&mut self.state, State::Invalid);
                let state = if let State::WaitForChunk(state) = state { state } else { unreachable!() };
                let chunk = Chunk::decode(state.buf, size, state.chunk_info.index_field_size);
                self.chunk(chunk, state.chunk_info, state.file, state.bitmap);
            }
            State::WritingChunk(_) => unreachable!(),
        }
        if let State::Invalid = self.state {
            panic!("Invalid Receiver-State");
        }
        Ok(Async::Ready(Some(())))
    }
}

