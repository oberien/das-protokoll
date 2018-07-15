#[macro_use]
extern crate log;
extern crate env_logger;
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
extern crate crypto;
extern crate rand;
extern crate aesstream;
#[macro_use]
extern crate structopt;

use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(name = "scsync", about = "Secure (tm) Cloud Sync")]
pub struct Opt {
    /// Server Mode
    #[structopt(short = "s", long = "server")]
    server: bool,
    /// Port to connect to
    #[structopt(short = "p", long = "port", default_value = "21088")]
    port: u16,
    /// Remote host
    #[structopt(short = "h", long = "host", default_value = "127.0.0.1")]
    host: String,
    /// Directory to upload files from
    #[structopt(short = "f", long = "files")]
    files: String,
    #[structopt(short = "cc", long = "packet-rate")]
    pps: u32,
}

use tokio::prelude::*;
use tokio::net::{UdpSocket, UdpFramed};
use futures::unsync::mpsc;
use futures::future::Either::*;

use std::usize;
use std::net::{SocketAddr, ToSocketAddrs};

mod frontend;
mod blockdb;
mod codec;
mod handler;

use std::time::Duration;

use codec::{Msg, MyCodec};
use frontend::Frontend;
use handler::{Handler, ClientState};

fn main() {
    env_logger::init();

    let opt = Opt::from_args();
    let addr = SocketAddr::new(opt.host.parse().unwrap(), opt.port);
    let bind_addr = if opt.server {
        addr
    } else {
        "0.0.0.0:0".to_socket_addrs().unwrap().next().unwrap()
    };

    let blockdb = Frontend::blockdb_from_folder(&opt.files);

    let socket = UdpSocket::bind(&bind_addr).unwrap();

    let framed = UdpFramed::new(socket, MyCodec);
    let (utx, rx) = framed.split();
    let (tx, crx) = mpsc::channel(1); // would like this to be 0 but impossibruh

    let handler = Handler::new(opt.files.into(), Duration::from_secs(1) / opt.pps, blockdb, &tx);

    let init_task = if opt.server {
        A(future::ok(())) // nothing to do for servers
    } else {
        B(handler.connect(addr))
    };

    // omfg give bang type already !!!!!!!!
    let crx = crx.map_err(|()| None.unwrap());

    // cant happen
    //let tx = tx.sink_map_err(|_| { unreachable!(); std::io::Error::new(std::io::ErrorKind::AddrInUse, "") });

    let send_task = crx.forward(utx).map(|(_, _)| ());

    let recv_task = rx.map(|(msg, addr)| {
        trace!("received message from {}", addr);
        match handler.client_state(&addr) {
            ClientState::New => A(match msg {
                Msg::RootUpdate(update) => {
                    handler.unconnected_root_update(addr, update);
                    future::ok(())
                },
                _ => future::ok(()), // ignore all other messages from a node we don't know
            }),
            ClientState::Known => B({
                match msg {
                    // TODO: not sure how to handle root updates here??
                    Msg::RootUpdate(_) => A(future::ok(())),
                    Msg::RootUpdateResponse(res) => {
                        if !handler.needs_update(&res) {
                            // nothing to do
                            A(future::ok(()))
                        } else {
                            // we need to update
                            B(A(A(handler.root_update_response(addr, res))))
                        }
                    }
                    Msg::BlockRequest(req) => {
                        // do things, then send response
                        B(B(handler.block_request(addr, req)))
                    }
                    Msg::BlockRequestResponse(res) => {
                        handler.block_request_response(addr, res);
                        A(future::ok(()))
                    }
                    Msg::TransferPayload(payload) => {
                        // receive data
                        handler.transfer_payload(addr, payload);
                        A(future::ok(()))
                    }
                    Msg::TransferStatus(status) => {
                        // update transfer status
                        B(A(B(handler.transfer_status(addr, status))))
                    }
                }
            }),
        }

    }).buffer_unordered(usize::MAX).for_each(|()| Ok(()));


    let app = future::join_all(vec![
        A(A(send_task)),
        A(B(init_task)),
        B(recv_task),
    ]);

    let res: std::io::Result<_> = tokio::executor::current_thread::block_on_all(app);

    assert!(res.is_ok());
    println!("{:?}", res);
}
