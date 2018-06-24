use std::io::{Seek, SeekFrom};
use std::time::{Instant, Duration};
use std::net::{SocketAddr, ToSocketAddrs, UdpSocket as StdUdp};
use std::fs::{self, File as StdFile};
use std::cell::RefCell;
use std::path::Path;

use walkdir::WalkDir;
use futures::future::{self, ok, loop_fn, Loop, Either};
use futures::Future;
use tokio::net::UdpSocket;
use tokio::io::Error;
use tokio::reactor::Handle;
use tokio::runtime::current_thread::Runtime;
use tokio::reactor::PollEvented2 as PollEvented;
use tokio::net::RecvDgram;
use tokio_file_unix::File;
use tokio::io;
use byteorder::{WriteBytesExt, LE};

use codec::*;

pub fn client(opt: super::Opt) -> Result<(), Error> {
    for file in WalkDir::new(opt.files.as_ref().unwrap()) {
        let file = file.unwrap();
        if file.file_type().is_file() {
            let file = file.path().strip_prefix(opt.files.as_ref().unwrap()).unwrap();
            println!("uploading {:?}", file);
            client_once(file.to_str().unwrap(), &opt)?;
        }
    }

    Ok(())
}

pub fn client_once(filename: &str, opt: &super::Opt) -> Result<(), Error>  {
    let socket = StdUdp::bind("0.0.0.0:0")?;
    let server = &(opt.host.as_str(), opt.port).to_socket_addrs().unwrap().next().unwrap();
    socket.connect(server)?;

    let socket2 = socket.try_clone().unwrap();


    let mut send_buf: Vec<u8> = Vec::with_capacity(MTU);
    let recv_buf: Vec<u8> = vec![0; MTU];


    let mut runtime = Runtime::new()?;

    let missing = &RefCell::new(MissingRanges::default());

    let file = StdFile::open(Path::new(opt.files.as_ref().unwrap()).join(filename)).unwrap();
    let filesize = file.metadata().unwrap().len();

    let chunk_info = &index_field_size(filesize);

    let client = future::lazy(move || {
        let reactor: &Handle = &Handle::current();

        let socket = UdpSocket::from_std(socket, reactor).unwrap();
        let socket2 = UdpSocket::from_std(socket2, reactor).unwrap();

        let chunk_size = chunk_info.chunk_size;
        let chunk_count = chunk_info.num_chunks;
        let last_chunk_size = chunk_info.last_chunk_size;
        let file = File::new_nb(file).unwrap().into_io(reactor).unwrap();

        Login { client_token: b"roflcopter", command: Command::UploadRequest(UploadRequest { path: filename, length: filesize }) }.encode(&mut send_buf);

        let client = socket.send_dgram(send_buf, server)
            .and_then(move |(socket, send_buf)| {
                let server = server.clone();
	    /*
                let syn_send_stamp = Instant::now();
                socket.recv_dgram(recv_buf).map(move |x| (x, send_buf, syn_send_stamp))
            })
            .and_then(move |((socket, recv_buf, recv_len, server), mut send_buf, syn_send_stamp)| {
                // TODO: process contents of this packet?
                let ack_recv_stamp = Instant::now();
                let rtt = ack_recv_stamp - syn_send_stamp;
                // assert_eq!(recv_len, 0); // ???

                //let mut missing_chunks = MissingRanges::default();
                missing.borrow_mut().parse_status_update(&recv_buf[..recv_len]);
                //println!("{:?}", missing_chunks);

                //send_buf.clear();
                // TODO: write something??

                socket.send_dgram(send_buf, &server).map(move |x| (x, recv_buf, server, rtt, missing))
            })
            .and_then(move |((socket, send_buf), recv_buf, server, rtt, missing)| {
                println!("sta rtt={:?}", rtt);
		*/

                loop_fn((do_chunk(chunk_info, missing, file, socket, send_buf, server), socket2.recv_dgram(recv_buf), last_chunk_size), move |(chunk_send, update_recv, lcs)| {
                    match chunk_send {
                        Ok(chunk_send) => Box::new(chunk_send.select2(update_recv).then(move |e| Ok(match e {
                            Ok(Either::A(((file, socket, send_buf), update_recv))) => {
                                // send done, go send another
                                Loop::Continue((do_chunk(chunk_info, missing, file, socket, send_buf, server), update_recv, lcs))
                            }
                            Ok(Either::B(((socket2, recv_buf, recv_len, server), chunk_send))) => {
                                // got a status update!
                                if missing.borrow_mut().parse_status_update(&recv_buf[..recv_len]) {
                                    Loop::Break((socket2, recv_buf, server))
                                } else {
                                    // read the next one
                                    Loop::Continue((Ok(chunk_send), socket2.recv_dgram(recv_buf), lcs))
                                }
                            }
                            Err(Either::A((e, _))) | Err(Either::B((e, _))) => return Err(e), // just forward errors
                        }))) as Box<Future<Item=_, Error=_>>,
                        Err((file, socket, send_buf)) => {
                            Box::new(update_recv.map(move |(socket2, recv_buf, recv_len, server)| {
                                // got a status update while sleeping
                                if missing.borrow_mut().parse_status_update(&recv_buf[..recv_len]) {
                                    Loop::Break((socket, send_buf, server))
                                } else {
                                    // start sending again and read the next one
                                    Loop::Continue((do_chunk(chunk_info, missing, file, socket, send_buf, server), socket2.recv_dgram(recv_buf), lcs))
                                }
                            })) as Box<Future<Item=_, Error=_>>
                        }
                    }
                }).and_then(move |(socket, send_buf, server): (UdpSocket, _, SocketAddr)| {
                    let chunk = Chunk::new(send_buf, chunk_info.num_chunks, chunk_info.index_field_size, 0);

                    socket.send_dgram(chunk.into_vec(), &server)
                })
            });
        client
    });

    let res = runtime.block_on(client).unwrap();
    println!("{:?}", res);

    Ok(())
}

struct Client<'a> {
    socket: UdpSocket,
    server: SocketAddr,
    send_buf: Vec<u8>,
    rtt: Duration,

    file: PollEvented<File<StdFile>>, // we only ever read whole chunks out of this
    missing_chunks: &'a RefCell<MissingRanges>,
    chunk_cursor: u64,
    last_chunk_size: u64,
}


fn do_chunk(chunk_info: &ChunkInfo,
            missing_chunks: &RefCell<MissingRanges>,
            mut file: PollEvented<File<StdFile>>,
            socket: UdpSocket,
            send_buf: Vec<u8>,
            server: SocketAddr)
            -> Result<Box<Future<Item = (PollEvented<File<StdFile>>, UdpSocket, Vec<u8>), Error = Error>>, (PollEvented<File<StdFile>>, UdpSocket, Vec<u8>)> {
    let chunk_cursor = match missing_chunks.borrow_mut().next_chunk() {
        Some(x) => x,
        None => return Err((file, socket, send_buf)),
    };

    let payload = if chunk_cursor == chunk_info.num_chunks - 1 {
        chunk_info.last_chunk_size
    } else {
        chunk_info.chunk_size
    };

    let chunk = Chunk::new(send_buf, chunk_cursor, chunk_info.index_field_size, payload as usize);
    file.get_mut().seek(SeekFrom::Start(chunk_cursor as u64 * chunk_info.chunk_size)).unwrap();

    Ok(Box::new(
        io::read_exact(file, chunk)
            .and_then(move |(file, chunk)| {
                let send_buf = chunk.into_vec();
                let server = server;
                socket.send_dgram(send_buf, &server).map(move |(socket, send_buf)| {
                    (file, socket, send_buf)
                })
            })
    ))
}



// draft v1: pump as much as possible

// constant desire to send
// need a send cursor
// as well as the bitmap
// open file descriptor
