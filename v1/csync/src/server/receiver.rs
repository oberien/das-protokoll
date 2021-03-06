use std::io::{Seek, SeekFrom, Error as IoError};
use std::mem;
use std::fs::{self, File as StdFile, OpenOptions};
use std::path::{PathBuf, Path};
use std::sync::{Arc, Mutex};

use futures::{Stream, Async, Poll, Future};
use futures::sync::mpsc::UnboundedSender;
use tokio::io;
use tokio::net::UdpSocket;
use tokio::reactor::{PollEvented2, Handle};
use tokio_file_unix::File;
use ring::digest;
use hex::ToHex;
use bitte_ein_bit::BitMap;
use memmap::{MmapMut, MmapOptions};

use codec::{self, MTU, Login, Command, UploadRequest, Chunk, ChunkInfo};
use server::congestion::CongestionInfo;
use server::ChannelMessage;

pub struct Receiver {
    state: State,
    socket: UdpSocket,
    tx: UnboundedSender<ChannelMessage>,
    folder: PathBuf,
    congestion: CongestionInfo,
}

pub enum State {
    Invalid,
    WaitForAck(WaitForChunk),
    WaitForChunk(WaitForChunk),
    WritingChunk(WritingChunk),
    Shutdown(Vec<u8>),
}

pub struct WaitForChunk {
    file: PollEvented2<File<StdFile>>,
    bitmap: Arc<Mutex<BitMap<MmapMut>>>,
    bitmap_path: PathBuf,
    buf: Vec<u8>,
    chunk_info: ChunkInfo,
}

type WriteChunk = io::WriteAll<PollEvented2<File<StdFile>>, Chunk>;

pub struct WritingChunk {
    bitmap: Arc<Mutex<BitMap<MmapMut>>>,
    bitmap_path: PathBuf,
    chunk_info: ChunkInfo,
    future: WriteChunk,
}

impl Receiver {
    pub fn new(socket: UdpSocket, login: Login, tx: UnboundedSender<ChannelMessage>) -> Receiver {
        debug!("Login");
        trace!("Client Token: {:?}", login);
        let sha = digest::digest(&digest::SHA256, login.client_token);
        let mut hex = String::with_capacity(digest::SHA256_OUTPUT_LEN * 2);
        sha.as_ref().write_hex(&mut hex).unwrap();
        let mut path = PathBuf::from("./files/");
        path.push(hex);
        debug!("Folder: {}", path.display());
        let mut receiver = Receiver {
            state: State::Invalid,
            socket,
            tx,
            folder: path,
            congestion: CongestionInfo::new(),
        };
        receiver.command(login.command);
        receiver.tx.unbounded_send(ChannelMessage::UploadStatus).unwrap();
        receiver.congestion.start_rtt();
        receiver
    }

    pub fn command(&mut self, command: Command) {
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
        let path = self.folder.join(req_path);
        fs::create_dir_all(&path).unwrap();
        let file_path = path.join("file");
        let bitmap_path = path.join("bitmap");

        let continue_upload = bitmap_path.exists();

        let bitmap_file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&bitmap_path)
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
            //debug!("Continue Upload file: {:x?}", bitmap);
            if bitmap.all() {
                warn!("Continue upload, but all chunks are already received.");
            }
            file.append(true);
        }
        let file = file.create(true)
            .write(true)
            .open(file_path).unwrap();
        file.set_len(req.length as u64).unwrap();

        let bitmap = Arc::new(Mutex::new(bitmap));
        self.tx.unbounded_send(ChannelMessage::UploadStart(Arc::clone(&bitmap))).unwrap();

        self.state = State::WaitForChunk(WaitForChunk {
            file: File::new_nb(file).unwrap().into_io(&Handle::current()).unwrap(),
            bitmap,
            bitmap_path,
            buf: Vec::with_capacity(MTU),
            chunk_info,
        });
    }

    pub fn ack(&mut self) {
        self.congestion.stop_rtt();
        let rtt = self.congestion.rtt();
        debug!("Ack from Client, RTT {}ms", rtt.as_secs() * 1000 + rtt.subsec_nanos() as u64 / 1_000_000);
        let state = mem::replace(&mut self.state, State::Invalid);
        let state = if let State::WaitForAck(state) = state { state } else { unreachable!() };
        self.state = State::WaitForChunk(state);

    }

    pub fn chunk(&mut self, chunk: Chunk, chunk_info: ChunkInfo, bitmap_path: PathBuf,
                 mut file: PollEvented2<File<StdFile>>, bitmap: Arc<Mutex<BitMap<MmapMut>>>) {
        self.congestion.ipt_packet();
        if bitmap.lock().unwrap().get(chunk.index) {
            info!("Chunk {} already received, skipping", chunk.index);
            self.state = State::WaitForChunk(WaitForChunk {
                file,
                bitmap,
                bitmap_path,
                buf: chunk.into_vec(),
                chunk_info,
            });
            return;
        }
        trace!("Switch to WritingChunk with len {}", chunk.as_ref().len());
        file.get_mut().seek(SeekFrom::Start(chunk.index * chunk_info.chunk_size)).unwrap();
        let future = io::write_all(file, chunk);
        self.state = State::WritingChunk(WritingChunk {
            bitmap,
            bitmap_path,
            chunk_info,
            future,
        });
        // TODO: length check of chunks to ensure max usage of MTU
    }
}

impl Stream for Receiver {
    type Item = ();
    type Error = IoError;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Self::Error> {
        match self.congestion.poll() {
            Ok(Async::Ready(Some(()))) => {
                self.tx.unbounded_send(ChannelMessage::UploadStatus).unwrap();
            }
            Ok(Async::NotReady) => {}
            Ok(Async::Ready(None)) => unreachable!(),
            Err(e) => return Err(io::Error::new(io::ErrorKind::Other, e))
        }
        if let State::WritingChunk(_) = self.state {
            trace!("Try writing Chunk");
            let (file, chunk) = {
                let state = if let State::WritingChunk(ref mut state) = self.state { state } else { unreachable!() };
                try_ready!(state.future.poll())
            };
            trace!("Chunk written");

            let state = mem::replace(&mut self.state, State::Invalid);
            let mut state = if let State::WritingChunk(state) = state { state } else { unreachable!() };
            {
                let mut bitmap = state.bitmap.lock().unwrap();
                bitmap.set(chunk.index, true);

                if bitmap.zeroes().is_power_of_two() || bitmap.zeroes() == 0 {
                    debug!("Power of 2: {}", bitmap.zeroes());
                    self.tx.unbounded_send(ChannelMessage::UploadStatus).unwrap();
                }

                // if last chunk
                if bitmap.all() {
                    info!("Last chunk received. Switch to WaitForChunk until fin-chunk is received.");
                }
            }
            trace!("Switch to WaitForChunk");
            self.state = State::WaitForChunk(WaitForChunk {
                file,
                buf: chunk.into_vec(),
                bitmap: state.bitmap,
                bitmap_path: state.bitmap_path,
                chunk_info: state.chunk_info,
            });
        }

        match self.state {
            State::Invalid => unreachable!(),
            State::WaitForAck(_) => {
                if let State::WaitForAck(WaitForChunk { ref mut buf, .. }) = self.state {
                    try_ready!(self.socket.poll_recv(buf));
                }
                self.ack()
            },
            State::WaitForChunk(_) => {
                if let State::WaitForChunk(WaitForChunk { ref mut buf, .. }) = self.state {
                    buf.resize(MTU, 0);
                    let size = try_ready!(self.socket.poll_recv(buf));
                    buf.truncate(size);
                } else { unreachable!() };
                let state = mem::replace(&mut self.state, State::Invalid);
                let state = if let State::WaitForChunk(state) = state { state } else { unreachable!() };
                let chunk = Chunk::decode(state.buf, state.chunk_info.index_field_size);
                match chunk.index.wrapping_sub(state.chunk_info.num_chunks) {
                    0 => {
                        let all = state.bitmap.lock().unwrap().all();
                        if !all {
                            error!("Got FIN from client, but bitmap is not full???");
                        }
                        debug!("Moving to shutdown");
                        if all && state.bitmap_path.exists() {
                            info!("Remove bitmap file");
                            fs::remove_file(state.bitmap_path).unwrap();
                        }
                        self.congestion.shutdown();
                        self.state = State::Shutdown(chunk.into_vec());
                    },
                    _ => self.chunk(chunk, state.chunk_info, state.bitmap_path, state.file, state.bitmap)
                }
            }
            State::WritingChunk(_) => unreachable!(),
            State::Shutdown(ref mut buf) => {
                buf.resize(MTU, 0);
                try_ready!(self.socket.poll_recv(buf));
                // ignore everything, we're shutting down
                debug!("Got message in Shutdown");
            }
        }
        if let State::Invalid = self.state {
            panic!("Invalid Receiver-State");
        }
        Ok(Async::Ready(Some(())))
    }
}

