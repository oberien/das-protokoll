use std::net::SocketAddr;
use std::io;
use std::time::Duration;

use futures::sync::mpsc;
use futures::{Future, Stream, Sink};
use tokio::net::{UdpSocket, UdpFramed};
use tokio::reactor::Handle;
use tokio::timer::Interval;
use tokio;
use net2;

use codec::Codec;
use timeout::TimeoutStream;

pub fn run() {
    let buf = vec![0; 8192];
    let listener = get_socket().expect("Can't bind main UdpSocket");
    let listener = UdpFramed::new(listener, Codec);

    let server = listener.for_each(|(vec, addr)| {
        println!("connection from {}: {:?}", addr, vec);
        let client = handle_client(vec, addr);
        tokio::spawn(client);
        Ok(())
    }).map_err(|e| eprintln!("Error during server: {:?}", e));

    tokio::run(server);
}

fn get_socket() -> io::Result<UdpSocket> {
    let std_sock = net2::UdpBuilder::new_v4()?
        .reuse_address(true)?
        .bind("127.0.0.1:21088")?;
    UdpSocket::from_std(std_sock, &Handle::current())
}

type BoxedFuture = Box<Future<Item = (), Error = ()> + Send>;

fn handle_client(vec: Vec<u8>, addr: SocketAddr) -> BoxedFuture {
    let sock = get_socket().expect("Can't create client UdpSocket");
    sock.connect(&addr).expect("Can't connect to client");
    let framed = UdpFramed::new(sock, Codec);

    let (sink, stream) = framed.split();
    let (tx, rx) = mpsc::unbounded();

    let sender = TimeoutStream::new(rx, Duration::from_secs(10))
        .map_err(|e| eprintln!("Error in channel-receiver: {:?}", e))
        .forward(sink.sink_map_err(|e| eprintln!("Error sending to sink: {:?}", e)))
        .map(|_| ());

    let receiver = TimeoutStream::new(stream, Duration::from_secs(10))
        .for_each(move |(vec, _)| {
            println!("received: {:?}", vec);
            tx.unbounded_send((vec, addr)).unwrap();
            Ok(())
        }).map_err(|e| eprintln!("Error during receive: {:?}", e));

    let client = sender.select(receiver)
        .map(|(res, _)| println!("Client finished successfully: {:?}", res))
        .map_err(|(err, _)| println!("Client finished with error: {:?}", err));
    Box::new(client)

}
