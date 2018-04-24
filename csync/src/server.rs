use std::net::SocketAddr;
use std::io;
use std::time::{Instant, Duration};

use futures::sync::mpsc;
use futures::{Future, Stream, Sink};
use tokio::net::{UdpSocket, UdpFramed};
use tokio::reactor::Handle;
use tokio;
use net2;

use codec::Codec;

pub fn run() {
    let buf = vec![0; 8192];
    let listener = get_socket().expect("Can't bind main UdpSocket");
    let mut listener = UdpFramed::new(listener, Codec);

    let server = listener.for_each(|(vec, addr)| {
        println!("connection from {}: {:?}", addr, vec);
        let (sender, receiver) = handle_client(vec, addr);
        let sender = sender.map_err(|e| eprintln!("Error during Client Send: {:?}", e));
        let receiver = receiver.map_err(|e| eprintln!("Error during Client Receive: {:?}", e));
        tokio::spawn(sender);
        tokio::spawn(receiver);
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

fn handle_client(vec: Vec<u8>, addr: SocketAddr) -> (BoxedFuture, BoxedFuture) {
    let sock = get_socket().expect("Can't create client UdpSocket");
    sock.connect(&addr).expect("Can't connect to client");
    let framed = UdpFramed::new(sock, Codec);

    let (sink, stream) = framed.split();
    let (tx, rx) = mpsc::unbounded();

    let sender = rx.forward(sink.sink_map_err(|e| eprintln!("Error sending to sink: {:?}", e)))
        .map(|_| ());

    let receiver = stream.for_each(move |(vec, _)| {
        println!("received: {:?}", vec);
        tx.unbounded_send((vec, addr));
        Ok(())
    }).map_err(|e| eprintln!("Error during receive: {:?}", e));
    (Box::new(sender), Box::new(receiver))

}
