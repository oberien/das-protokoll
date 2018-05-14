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
extern crate tokio_file_unix;
extern crate ring;
extern crate hex;
extern crate take_mut;
#[macro_use]
extern crate structopt;
extern crate bitte_ein_bit;
extern crate memmap;
extern crate itertools;
extern crate walkdir;


mod server;
mod client;
mod codec;
mod timeout;

use structopt::StructOpt;

#[derive(StructOpt)]
#[structopt(name = "csync", about = "Cloud Sync")]
pub struct Opt {
    /// Server Mode
    #[structopt(short = "s", long = "server")]
    server: bool,
    /// Port to connect to
    #[structopt(short = "p", long = "port", default_value = "21088")]
    port: u16,
    /// Remote host
    #[structopt(short = "h", long = "host", default_value = "localhost")]
    host: String,
    /// Directory to upload files from
    #[structopt(short = "f", long = "files")]
    files: Option<String>,
}

fn main() {
    env_logger::init();

    let opt = Opt::from_args();

    if opt.server {
        server::run(opt);
    } else {
        if opt.files.is_none() {
            eprintln!("Files required for client mode. Execute --help for help.");
            return;
        }
        client::client(opt).unwrap();
    }
}
