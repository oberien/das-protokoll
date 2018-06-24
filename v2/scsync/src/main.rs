#![allow(unused_variables, dead_code)]

use std::net::{ToSocketAddrs, SocketAddr, UdpSocket};
use std::io::Result as IoResult;
use std::time::{Duration, Instant};
use std::sync::{Arc, Mutex};
use std::thread;
use std::collections::HashMap;

const MTU: usize = 1460;

struct BlockDb {
    root: Mutex<[u8; HL]>,
    pending_root: Mutex<Option<[u8; HL]>>,
    blocks: Mutex<HashMap<[u8; HL], Arc<Mutex<Block>>>>,
}

struct Block {
    id: [u8; HL],
    data: Vec<u8>,
    avail: Vec<bool>,
}

impl Block {
    fn id(&self) -> &[u8; HL] {
        &self.id
    }

    fn len(&self) -> u64 {
        self.data.len() as u64
    }
}

impl BlockDb {
    fn root(&self) -> [u8; HL] {
        self.root.lock().unwrap().clone()
    }

    fn pending_root(&self) -> Option<[u8; HL]> {
        self.pending_root.lock().unwrap().clone()
    }

    fn set_pending_root(&self, id: Option<[u8; HL]>) {
        *self.pending_root.lock().unwrap() = id;
    }

    fn block(&self, id: &[u8; HL]) -> Option<Arc<Mutex<Block>>> {
        Some(self.blocks.lock().unwrap().get(id)?.clone())
    }

    fn block_new(&self, id: &[u8; HL], len: usize) -> Arc<Mutex<Block>> {
        self.blocks.lock().unwrap().entry(id.clone()).or_insert(Arc::new(Mutex::new(Block { id: id.clone(), data: vec![0; len], avail: vec![false; len] }))).clone()
    }
}

fn timeouted(dur: &mut Duration, last: Instant, timeout: Duration) -> bool {
    let target = match timeout.checked_sub(Instant::now() - last) {
        Some(x) => x,
        None => return true,
    };
    if target < *dur {
        *dur = target;
    }

    false
}

fn to_bid(bytes: &[u8]) -> [u8; HL] {
    let mut ret = [0; HL];
    ret.copy_from_slice(bytes);
    ret
}

fn send<A: ToSocketAddrs>(socket: &UdpSocket, buf: &[u8], addr: A, packet_rate: u32) -> IoResult<usize> {
    thread::sleep(Duration::from_secs(1) / packet_rate);
    socket.send_to(buf, addr)
}

#[derive(Default)]
struct PeerState {
    transfer_out: TransferOut,
    transfer_in: TransferIn,
}

impl PeerState {
    fn update(&mut self, dur: &mut Duration, pr: u32, socket: &UdpSocket, peer: &SocketAddr) -> IoResult<()> {
        self.transfer_in.update(dur, pr, socket, peer)?;
        //self.transfer_out.update(dur, pr, socket, peer)?;
        Ok(())
    }
}

#[derive(Default)]
struct TransferOut {
    // from, to, block, remote-requested
    allocated_ids: Vec<(u64, u64, Arc<Mutex<Block>>, Vec<bool>)>,
    next_alloc: u64,
}

#[derive(Default)]
struct TransferIn {
    allocated_ids: Vec<(u64, u64, Arc<Mutex<Block>>, Instant)>,
    pending_block_requests: Vec<([u8; HL], Instant)>,
}

const PKSZ: u64 = 100;
impl TransferOut {
    fn alloc(&mut self, block: Arc<Mutex<Block>>) -> (u64, u64) {
        // TODO: check if block was already allocd (in case of lost ack)
        let start = self.next_alloc;
        self.next_alloc += (block.lock().unwrap().len() + PKSZ - 1) / PKSZ;
        let end = self.next_alloc;
        self.allocated_ids.push((start, end, block, vec![false; (end - start) as usize]));
        (start, end)
    }

    fn update(&mut self, dur: &mut Duration, pr: u32, socket: &UdpSocket, peer: &SocketAddr) -> IoResult<()> {
        // TODO: keep sending out blocks for open transfers
        Ok(())
    }
}

fn dummy_instant() -> Instant {
    Instant::now() - Duration::from_secs(999999)
}

impl TransferIn {
    fn transfer_allocated(&mut self, from: u64, to: u64, block: Arc<Mutex<Block>>) {
        let blockid = block.lock().unwrap().id().clone();

        let index = self.pending_block_requests.iter().position(|(i, _)| i == &blockid).unwrap();
        self.pending_block_requests.swap_remove(index);

        self.allocated_ids.push((from, to, block, dummy_instant()));
    }

    fn request_block(&mut self, id: [u8; HL]) {
        self.pending_block_requests.push((id, dummy_instant())); // insta retry in next update phase
    }

    fn update(&mut self, dur: &mut Duration, pr: u32, socket: &UdpSocket, peer: &SocketAddr) -> IoResult<()> {
        for (ref id, ref mut last) in self.pending_block_requests.iter_mut() {
            if timeouted(dur, *last, Duration::from_secs(3)) { // constant 3s timeout for this
                send(&socket, &[4].iter().chain(id).cloned().collect::<Vec<_>>(), peer, pr)?;
                *last = Instant::now();
            }
        }

        let mut finished = Vec::new();
        for (i, &mut (from, _to, ref block, ref mut last)) in self.allocated_ids.iter_mut().enumerate() {
            if timeouted(dur, *last, Duration::from_secs(1)) { // status update every 1s
                let block = block.lock().unwrap();

                let mut start = 0;
                let mut cur = true;
                let mut ranges = Vec::new(); // missing
                for (i, &v) in block.avail.iter().enumerate() {
                    let i = i as u64;
                    if v != cur {
                        if v {
                            ranges.push((from + start, from + i))
                        }
                        start = i;
                        cur = v;
                    }
                }

                if ranges.is_empty() {
                    finished.push(i);
                } else {
                    send(&socket, &[1].iter().cloned().chain(ranges.into_iter().flat_map(|(from, to)| varlen_encode(from).into_iter().chain(varlen_encode(to)))).take(MTU).collect::<Vec<_>>(), peer, pr)?;
                    *last = Instant::now();
                }
            }
        }
        finished.sort();
        for i in finished.into_iter().rev() {
            let (from, to, block, last) = self.allocated_ids.swap_remove(i);
            // block done, todo check hash and shit
            let bblock = block.lock().unwrap();
            assert!(bblock.avail.iter().all(|&x| x)); // assert fully available

            // TODO parse block content; request data recursively
            // TODO detect when we are fully done (no more blocks to fetch) to commence root update
        }

        Ok(())
    }
}

// lol what a lie
fn varlen_encode(i: u64) -> Vec<u8> {
    let mut ret = vec![0; 8];
    unsafe {
        *(ret.as_mut_ptr() as *mut u64) = i;
    }
    ret
}
fn varlen_decode(x: &[u8]) -> u64 {
    assert_eq!(x.len(), 8);
    unsafe {
        *(x.as_ptr() as *const u64)
    }
}

const HL: usize = 32;
fn server(listen: &str, connect: Option<&SocketAddr>, blockdb: Arc<BlockDb>, pr: u32) -> IoResult<()> {
    let socket = UdpSocket::bind(listen)?;

    socket.set_read_timeout(Some(Duration::from_secs(0)))?;

    let mut last_poll = dummy_instant();

    let mut peers = HashMap::new();
    if let Some(server) = connect {
        peers.insert(server.clone(), PeerState::default());
    }

    loop {
        let mut buf = [0u8; MTU];
        match socket.recv_from(&mut buf) {
            Ok((len, addr)) => {
                let packet = &buf[..len];
                // handle packet
                let peer = if connect.is_none() {
                    // this is a server
                    Some(peers.entry(addr.clone()).or_insert_with(Default::default))
                } else {
                    peers.get_mut(&addr)
                };

                let peer = match peer {
                    Some(peer) => peer,
                    // else this is a stray packet to a client, ignore
                    None => continue
                };
                let kind = packet[0]; // TODO dont crash
                match kind {
                    0 => {
                        // transfer payload
                    }
                    1 => {
                        // transfer status update
                    }
                    2 => {
                        // root update

                        // TODO crypto

                        let root = blockdb.root();

                        let from_block = &packet[1..(1+HL)];
                        let to_block = &packet[1+HL .. 1+2*HL];

                        send(&socket, &[3].iter().chain(root.iter()).cloned().collect::<Vec<_>>(), &addr, pr)?;
                        if from_block == to_block {
                            // this is just a status query
                        } else if &root != from_block {
                            // an invalid update (broken chain)
                        } else if blockdb.pending_root().is_some() {
                            // update race, not supported
                        } else {
                            // a valid update
                            blockdb.set_pending_root(Some(to_bid(to_block)));
                            // launch transfer
                            peer.transfer_in.request_block(to_bid(to_block));
                        }
                    }
                    3 => {
                        // root update response
                        let our_root = blockdb.root();
                        let their_root = &packet[1..(1+HL)];
                        if our_root != their_root {
                            blockdb.set_pending_root(Some(to_bid(their_root)));
                            // launch transfer
                            peer.transfer_in.request_block(to_bid(their_root));
                        }
                    }
                    4 => {
                        // block request
                        //if let Some(ref mut t) = peer.transfer_out {
                        let t = &mut peer.transfer_out;
                        let blockid = &packet[1..(1+HL)];
                        let block = blockdb.block(&to_bid(blockid)).unwrap();
                        let len = block.lock().unwrap().len();
                        let (from, to) = t.alloc(block);
                        send(&socket, &[5].iter().chain(blockid).cloned().chain(varlen_encode(from)).chain(varlen_encode(to)).chain(varlen_encode(len)).collect::<Vec<_>>(), &addr, pr)?;
                        //}
                    }
                    5 => {
                        // block request response
                        let t = &mut peer.transfer_in;
                        //if let Some(ref mut t) = peer.transfer_in {
                        let blockid = &packet[1..(1+HL)];
                        let from = varlen_decode(&packet[(1+HL)..(1+HL+8)]);
                        let to = varlen_decode(&packet[(1+HL+8)..(1+HL+16)]);
                        let len = varlen_decode(&packet[(1+HL+16)..(1+HL+24)]);
                        let block = blockdb.block_new(&to_bid(blockid), len as usize);

                        t.transfer_allocated(from, to, block);
                        //t.allocated_ids.push((from, to, block));
                        //}
                    }
                    _ => (), // else ignore
                }
            }
            Err(_) => (), // TODO: error handling, right now we blindly assume read timeout
        }

        let mut next_timeout = Duration::from_secs(1); // ideally this would be forever
        // update state (and next timeout)
        if let Some(server) = connect {
            if timeouted(&mut next_timeout, last_poll, Duration::from_secs(1)) {
                // ping again
                //if peers.get(server).unwrap().transfer_out.is_none() { // only if no transfer active (else redundant (?))
                send(&socket, &[2].iter().chain(&blockdb.root()).chain(&blockdb.pending_root().unwrap_or(blockdb.root())).cloned().collect::<Vec<_>>(), server, pr)?;
                //}

                last_poll = Instant::now();
            }
        }
        for (addr, peer) in &mut peers {
            peer.update(&mut next_timeout, pr, &socket, addr)?;
        }

        socket.set_read_timeout(Some(next_timeout))?;
    }
}




fn main() {
}
