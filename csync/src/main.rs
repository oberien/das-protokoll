#[macro_use]
extern crate log;
extern crate env_logger;
extern crate futures;
extern crate bytes;
extern crate net2;
extern crate tokio;
extern crate tokio_io;

mod server;
mod client;
mod codec;
mod timeout;

fn main() {
    env_logger::init();

    server::run();
}
