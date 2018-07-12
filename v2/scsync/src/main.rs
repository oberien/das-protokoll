extern crate tokio;
extern crate tokio_io;
extern crate bytes;
extern crate futures;
extern crate serde;
#[macro_use]
extern crate serde_derive;
extern crate serde_cbor;
extern crate varmint;
extern crate itertools;
extern crate walkdir;
extern crate tiny_keccak;

use tokio::prelude::*;
use tokio::net::{UdpSocket, UdpFramed};
use tokio::timer::Delay;
use futures::unsync::{mpsc, oneshot};
use futures::future::{IntoFuture, Either};

use std::usize;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::time::{Instant, Duration};

mod frontend;
mod blockdb;
mod lel;
mod codec;

use lel::Lel::*;
use codec::{Msg, MyCodec};
use blockdb::BlockId;
use frontend::Frontend;

#[derive(Default)]
struct Client {
    pending_block_requests: HashMap<BlockId, oneshot::Sender<Msg>>,
}

fn request_retry<T, F, B>(rx: oneshot::Receiver<T>, mut f: F) -> impl Future<Item = T, Error = std::io::Error>
    where F: FnMut() -> B, B: IntoFuture<Item = ()> {
    future::loop_fn(rx, move |rx| {
        f().into_future()
            .map_err(|_| unimplemented!())
            .and_then(move |()| rx.select2(Delay::new(Instant::now() + Duration::from_secs(1))))
            .map(|x| match x {
                Either::A((response, _delay)) => future::Loop::Break(response),
                Either::B(((), orx)) => future::Loop::Continue(orx),
            })
            .map_err(|_| None.unwrap())
    })
}

fn main() {
    let frontend = Frontend::from_folder("foo");
    println!("{:#x?}", frontend);
    panic!("nyi");

    let mut clients = HashMap::new();

    let addr = "127.0.0.1:12345".parse().unwrap();
    let socket = UdpSocket::bind(&addr).unwrap();

    let framed = UdpFramed::new(socket, MyCodec);
    let (utx, rx) = framed.split();
    let (tx, crx) = mpsc::channel(1); // would like this to be 0 but impossibruh

    let tx = &tx;

    // omfg give bang type already !!!!!!!!
    let crx = crx.map_err(|()| None.unwrap());

    // cant happen
    //let tx = tx.sink_map_err(|_| { unreachable!(); std::io::Error::new(std::io::ErrorKind::AddrInUse, "") });

    let send_task = crx.forward(utx).map(|(_, _)| ());

    let recv_task = rx.map(|(msg, addr)| {
        match clients.entry(addr.clone()) {
            Entry::Vacant(v) => match msg {
                Msg::RootUpdate(update) => {
                    let response = if true {
                        // nothing to do, just respond
                        Msg::RootUpdateResponse(unimplemented!())
                    } else {
                        // open connection
                        let client = v.insert(Client::default());
                        Msg::RootUpdateResponse(unimplemented!())
                    };
                    A(tx.clone().send((response, addr))
                        .map(|_sender| ()).map_err(|_| unreachable!()))
                },
                _ => B(future::ok(())), // ignore all other messages from a node we don't know
            },
            Entry::Occupied(mut o) => {
                let client = o.get_mut();

                match msg {
                    // not sure how to handle root updates here??
                    Msg::RootUpdate(update) => C(future::ok(())),

                    Msg::RootUpdateResponse(res) => {
                        if true {
                            // nothing to do
                            D(future::ok(()))
                        } else {
                            // we need to update

                            // prepare to receive a block request response, then send one out
                            let (otx, orx) = oneshot::channel();
                            client.pending_block_requests.insert([0; 32], otx);
                            // todo: be "smart" about concurrency
                            // consider: A-B, A-C, B-D, C-D
                            // an update from A goes to B and C perhaps simultaneously,
                            // leading to B and C concurrently advertising the same state to D
                            // ideally D would receive half of the blocks from B and the other half from C
                            // however this requires cross-connection reasoning

                            E(
                                request_retry(orx, move || tx.clone().send((Msg::BlockRequest(unimplemented!()), addr.clone())).map(|_| ()))
                                    .map(|msg| ())
                            )
                        }
                    }
                    Msg::BlockRequest(req) => {
                        // do things, then send response
                        G(tx.clone().send((Msg::BlockRequestResponse(unimplemented!()), addr))
                            .map(|_sender| ()).map_err(|_| unreachable!()))
                    }
                    Msg::BlockRequestResponse(_) => {
                        if let Some(task) = client.pending_block_requests.remove(&[0; 32]) {
                            task.send(msg).unwrap();
                        }
                        F(future::ok(()))
                    }
                    Msg::TransferPayload(payload) => {
                        // receive data
                        H(future::ok(()))
                    }
                    Msg::TransferStatus(status) => {
                        // update transfer status
                        I(future::ok(()))
                    }
                }
            }
        }

    }).buffer_unordered(usize::MAX).for_each(|()| Ok(()));


    let app = future::join_all(vec![
        Either::A(send_task),
        Either::B(recv_task),
    ]);

    let res: std::io::Result<_> = tokio::executor::current_thread::block_on_all(app);

    println!("{:?}", res);
}
