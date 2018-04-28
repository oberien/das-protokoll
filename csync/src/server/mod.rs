use std::net::SocketAddr;
use std::io;
use std::net::UdpSocket as StdUdpSocket;
use std::time::Duration;

use futures::sync::mpsc;
use futures::{Future, Stream, Sink};
use tokio::net::UdpSocket;
use tokio::reactor::Handle;
use tokio;
use net2;

use codec::{MTU, Login};
use timeout::TimeoutStream;

mod listener;
mod receiver;
mod sender;

pub fn run() {
    let listener = get_socket().expect("Can't bind main UdpSocket");
    let listener = listener::Listener::new(listener);

    let server = listener.for_each(|(buf, size, addr)| {
        debug!("connection from {}: {:?}", addr, &buf[..size]);
        let client = handle_client(buf, size, addr);
        tokio::spawn(client);
        Ok(())
    }).map_err(|e| eprintln!("Error during server: {:?}", e));

    tokio::run(server);
}

fn get_std_socket() -> io::Result<StdUdpSocket> {
    net2::UdpBuilder::new_v4()?
        .reuse_address(true)?
        .bind("127.0.0.1:21088")
}

fn get_socket() -> io::Result<UdpSocket> {
    UdpSocket::from_std(get_std_socket()?, &Handle::current())
}

fn get_sockets() -> io::Result<(UdpSocket, UdpSocket)> {
    let std_sock = get_std_socket()?;
    let std_sock2 = std_sock.try_clone()?;
    let sock = UdpSocket::from_std(std_sock, &Handle::current())?;
    let sock2 = UdpSocket::from_std(std_sock2, &Handle::current())?;
    Ok((sock, sock2))
}

type BoxedFuture = Box<Future<Item = (), Error = ()> + Send>;

fn handle_client(buf: [u8; MTU], size: usize, addr: SocketAddr) -> BoxedFuture {
    let (sock, sock2) = get_sockets().expect("Can't create client UdpSocket");
    sock.connect(&addr).expect("Can't connect to client");
    sock2.connect(&addr).expect("Can't connect to client");

    let (tx, rx) = mpsc::unbounded();
    let login = Login::decode(&buf[..size]);
    let stream = receiver::Receiver::new(sock, login, tx);
    let sink = sender::Sender::new(sock2);

    let sender = TimeoutStream::new(rx, Duration::from_secs(10))
        .map_err(|e| eprintln!("Error in channel-receiver: {:?}", e))
        .forward(sink.sink_map_err(|e| eprintln!("Error sending to sink: {:?}", e)))
        .map(|_| ());

    let receiver = TimeoutStream::new(stream, Duration::from_secs(10))
        .for_each(Ok)
        .map_err(|e| eprintln!("Error during receive: {:?}", e));

    let client = sender.select(receiver)
        .map(|(res, _)| println!("Client finished successfully: {:?}", res))
        .map_err(|(err, _)| println!("Client finished with error: {:?}", err));
    Box::new(client)

}
