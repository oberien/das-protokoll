use std::io::{Write, Cursor, Seek, SeekFrom};
use std::time::{Instant, Duration};
use std::net::SocketAddr;
use std::fs::File as StdFile;

use bit_vec::BitVec;
use futures::future::{ok, loop_fn, Loop};
use futures::Future;
use tokio::net::UdpSocket;
use tokio::io::Error;
use tokio::runtime::Runtime;
use tokio::reactor::PollEvented2 as PollEvented;
use tokio_file_unix::File;
use tokio::io;
use byteorder::{ReadBytesExt, WriteBytesExt, LE};

use codec::*;

pub fn client() -> Result<(), Error>  {
    let socket = UdpSocket::bind(&"127.0.0.1:0".parse().unwrap())?;
    let server = &"127.0.0.1:21088".parse().unwrap();
    socket.connect(server)?;

    let mut send_buf: Vec<u8> = Vec::with_capacity(MTU);
    let recv_buf: Vec<u8> = vec![0; MTU];

    let mut runtime = Runtime::new().unwrap();

    let filename = "/usr/share/dict/cracklib-small";
    let file = StdFile::open(filename).unwrap();
    let filesize = file.metadata().unwrap().len();

    let chunk_info = index_field_size(filesize);
    let chunk_size = chunk_info.chunk_size;
    let chunk_count = chunk_info.num_chunks;
    let last_chunk_size = chunk_info.last_chunk_size;
    let file = File::new_nb(file).unwrap().into_io(runtime.reactor()).unwrap();

    Login { client_token: b"roflcopter" }.encode(&mut send_buf);

    let client = socket.send_dgram(send_buf, server)
        .and_then(move |(socket, send_buf)| {
            let syn_send_stamp = Instant::now();
            socket.recv_dgram(recv_buf).map(move |x| (x, send_buf, syn_send_stamp))
        })
        .and_then(move |((socket, recv_buf, recv_len, server), mut send_buf, syn_send_stamp)| {
            // TODO: process contents of this packet?
            let ack_recv_stamp = Instant::now();
            let rtt = ack_recv_stamp - syn_send_stamp;
            // assert_eq!(recv_len, 0); // ???

            send_buf.clear();
            Command::UploadRequest(UploadRequest { path: filename, length: filesize }).encode(&mut send_buf);

            socket.send_dgram(send_buf, &server).map(move |x| (x, recv_buf, server, rtt))
        })
        .and_then(move |((socket, send_buf), recv_buf, server, rtt)| {
            println!("sta rtt={:?}", rtt);

            loop_fn(Client {
                socket,
                server,
                send_buf,
                recv_buf,
                rtt,
                file,
                chunk_bitmap: BitVec::from_elem(chunk_count as usize, false),
                chunk_cursor: 0,
                last_chunk_size,
            }, move |Client { socket, server, mut send_buf, recv_buf, rtt, file, chunk_bitmap, chunk_cursor, last_chunk_size }| {
                if chunk_cursor == chunk_bitmap.len() {
                    // TODO: only exit once server has confirmed everything
                    Box::new(ok(Loop::Break(()))) as Box<Future<Item=_, Error=_> + Send>
                } else if chunk_bitmap[chunk_cursor] {
                    // server has confirmed, we can skip this
                    Box::new(ok(Loop::Continue(Client { socket, server, send_buf, recv_buf, rtt, file, chunk_bitmap, chunk_cursor: chunk_cursor + 1, last_chunk_size }))) as Box<Future<Item=_, Error=_> + Send>
                } else {
                    send_buf.clear();

                    let payload = if chunk_cursor == chunk_bitmap.len() - 1 {
                        last_chunk_size
                    } else {
                        chunk_size
                    };
                    send_buf.resize(payload as usize, 0);

                    Box::new(
                        io::read_exact(file, send_buf)
                            .and_then(move |(mut file, send_buf)| {
                                let mut arr = [0u8; 8];
                                (&mut arr[..]).write_u64::<LE>(chunk_cursor as u64).unwrap();

                                file.get_mut().seek(SeekFrom::Start(chunk_cursor as u64 * chunk_size));

                                let send_buf: Vec<u8> = arr.iter().cloned().take(index_field_size(filesize).index_field_size as usize).chain(send_buf.into_iter()).collect();

                                let server = server;
                                socket.send_dgram(send_buf, &server).map(move |x| (x, file, server))
                            })
                            .map(move |((socket, send_buf), file, server)| Client { socket, server, send_buf, recv_buf, rtt, file, chunk_bitmap, chunk_cursor: chunk_cursor + 1, last_chunk_size }).map(Loop::Continue)) as Box<Future<Item=_, Error=_> + Send>
                }
            })
        });


    runtime.spawn(client.map_err(|e| { panic!("{:?}", e); () }));
    runtime.shutdown_on_idle().wait().unwrap();

    Ok(())
}

struct Client {
    socket: UdpSocket,
    server: SocketAddr,
    send_buf: Vec<u8>,
    recv_buf: Vec<u8>,
    rtt: Duration,

    file: PollEvented<File<StdFile>>, // we only ever read whole chunks out of this
    chunk_bitmap: BitVec,
    chunk_cursor: usize,
    last_chunk_size: u64,
}

// draft v1: pump as much as possible

// constant desire to send
// need a send cursor
// as well as the bitmap
// open file descriptor
