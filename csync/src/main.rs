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
#[macro_use]
extern crate structopt;
extern crate bitte_ein_bit;
extern crate memmap;

mod server;
mod client;
mod codec;
mod timeout;

use std::path::PathBuf;

use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(name = "csync", about = "Cloud Sync")]
struct Opt {
    /// Server Mode
    #[structopt(short = "s", long = "server")]
    server: bool,
    /// Port to connect to
    #[structopt(short = "p", long = "port", default_value = "21088")]
    port: u16,
    /// Remote host
    #[structopt(short = "h", long = "host", default_value = "localhost")]
    host: String,

/*
    /// Directory to upload files from
    #[structopt(short = "f", long = "files")]
    files: Option<PathBuf>,
*/
}

fn main() {
    env_logger::init();

    let opt = Opt::from_args();

    if opt.server {
        server::run();
    } else {
        client::client().unwrap();
    }
}
