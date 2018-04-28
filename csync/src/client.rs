use std::io::Write;
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

const MTU: usize = 1460;
const CHUNK_SIZE: usize = 16;
pub fn client() -> Result<(), Error>  {
    let socket = UdpSocket::bind(&"127.0.0.1:0".parse().unwrap())?;
    let server = &"127.0.0.1:21088".parse().unwrap();
    socket.connect(server)?;

    let mut send_buf: Vec<u8> = Vec::with_capacity(MTU);
    let recv_buf: Vec<u8> = vec![0; MTU];

    let mut runtime = Runtime::new().unwrap();

    let filename = "/etc/passwd";
    let file = StdFile::open(filename).unwrap();
    let filesize = file.metadata().unwrap().len() as usize; // FIXME usize wtf
    let chunk_count = (filesize + CHUNK_SIZE - 1) / CHUNK_SIZE;
    let last_chunk_size = filesize - (chunk_count - 1) * CHUNK_SIZE;
    let file = File::new_nb(file).unwrap().into_io(runtime.reactor()).unwrap();

    writeln!(send_buf, "{}", filename).unwrap();

    let client = socket.send_dgram(send_buf, server)
        .and_then(move |(socket, send_buf)| {
            let syn_send_stamp = Instant::now();
            socket.recv_dgram(recv_buf).map(move |x| (x, send_buf, syn_send_stamp))
        })
        .and_then(|((socket, recv_buf, recv_len, server), mut send_buf, syn_send_stamp)| {
            // TODO: process contents of this packet?
            let ack_recv_stamp = Instant::now();
            let rtt = ack_recv_stamp - syn_send_stamp;
            // assert_eq!(recv_len, 0); // ???

            send_buf.clear();
            send_buf.push(0);
            send_buf.push(4);
            send_buf.extend_from_slice(b"yolo");

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
                chunk_bitmap: BitVec::from_elem(chunk_count, false),
                chunk_cursor: 0,
                last_chunk_size,
            }, |Client { socket, server, mut send_buf, recv_buf, rtt, file, chunk_bitmap, chunk_cursor, last_chunk_size }| {
                if chunk_cursor == chunk_bitmap.len() {
                    Box::new(ok(Loop::Break(()))) as Box<Future<Item=_, Error=_> + Send>
                    //unimplemented!();
                } else {
                    send_buf.clear();
                    send_buf.extend_from_slice(&[0; CHUNK_SIZE]);
                    if chunk_cursor == chunk_bitmap.len() - 1 {
                        send_buf.truncate(last_chunk_size);
                    }
                    Box::new(
                        io::read_exact(file, send_buf)
                            .and_then(move |(file, send_buf)| {
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
    last_chunk_size: usize,
}

// draft v1: pump as much as possible

// constant desire to send
// need a send cursor
// as well as the bitmap
// open file descriptor
