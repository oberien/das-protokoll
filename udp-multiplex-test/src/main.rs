extern crate net2;

fn main() {
    let unconnected = net2::UdpBuilder::new_v4().unwrap()
        .reuse_address(true).unwrap()
        .bind("127.0.0.1:1337").unwrap();
    let connected = net2::UdpBuilder::new_v4().unwrap()
        .reuse_address(true).unwrap()
        .bind("127.0.0.1:1337").unwrap();
    connected.connect("127.0.0.1:1338").unwrap();
    let mut buf = [0u8; 64];
    println!("start");
    loop {
        println!("{:?}", unconnected.recv_from(&mut buf).unwrap());
    }
}
