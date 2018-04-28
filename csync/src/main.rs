#[macro_use]
extern crate log;
extern crate env_logger;
#[macro_use]
extern crate futures;
extern crate bytes;
extern crate net2;
extern crate tokio;
extern crate varmint;
extern crate byteorder;
extern crate bit_vec;
extern crate tokio_file_unix;
extern crate ring;
extern crate hex;
extern crate take_mut;

mod server;
mod client;
mod codec;
mod timeout;

fn main() {
    env_logger::init();

    client::client().unwrap();
//    server::run();
}
