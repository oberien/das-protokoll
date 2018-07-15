use std::ops::Range;
use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::{io, mem, cmp};
use std::time::{Instant, Duration};
use std::path::PathBuf;

use futures::{Future, IntoFuture, Sink, Stream};
use futures::future::{self, Either, Loop};
use futures::unsync::{mpsc, oneshot};
use tokio;
use tokio::timer::Delay;
use rand;
use itertools::Itertools;

use blockdb::{BlockDb, BlockId, Block, Partial};
use codec::{RootUpdate, RootUpdateResponse, Msg, BlockRequest, BlockRequestResponse,
    TransferPayload, TransferStatus};
use frontend::{self, Decoded, Frontend};

// TODO dynamic chunk size scaling
// not yet implemented
// for now maximal constant size given u64 varints and hope for the best
const CHUNK_SIZE: usize = 1450;

#[derive(Default)]
struct Client {
    pending_block_requests: HashMap<BlockId, oneshot::Sender<()>>,
    transfer_out: TransferOut,
    transfer_in: TransferIn,
}

struct TransferOut {
    todo: Vec<(u64, u64)>, // [(from, to)] in chunkids
    cursor: Option<u64>,

    transfers: Vec<(u64, u64, BlockId)>, // [(from, to, block)] in chunks
    transfer_cursor: u64,
}

impl Default for TransferOut {
    fn default() -> TransferOut {
        TransferOut {
            todo: Vec::new(), // no todo
            cursor: None, // transfer idle
            transfers: Vec::new(), // no transfers
            transfer_cursor: rand::random::<u32>() as u64, // random start id
        }
    }
}

struct TransferIn {
    transfers: Vec<Transfer>,
    status_tx: Option<mpsc::UnboundedSender<()>>,
}

impl Default for TransferIn {
    fn default() -> Self {
        TransferIn {
            transfers: Vec::new(),
            status_tx: None,
        }
    }
}

#[derive(Clone)]
struct Transfer {
    /// BlockID for lookup in the BlockDB
    blockid: BlockId,
    /// Range of ChunkIDs assigned for this transfer
    id_range: Range<u64>,
}

impl TransferIn {
    fn transfer(&self, chunkid: u64) -> Transfer {
        self.transfers.iter().find(|t| t.id_range.start <= chunkid && t.id_range.end < chunkid).unwrap().clone()
    }
}


pub enum ClientState {
    Known,
    New,
}

pub struct Handler<'a> {
    folder: PathBuf,
    blockdb: Rc<RefCell<BlockDb>>,
    clients: Rc<RefCell<HashMap<SocketAddr, Client>>>,
    tx: &'a mpsc::Sender<(Msg, SocketAddr)>,
    packet_delay: Duration,
}

impl<'a> Handler<'a> {
    pub fn new(folder: PathBuf, packet_delay: Duration, blockdb: BlockDb, tx: &'a mpsc::Sender<(Msg, SocketAddr)>) -> Handler {
        Handler {
            folder,
            blockdb: Rc::new(RefCell::new(blockdb)),
            clients: Rc::new(RefCell::new(HashMap::new())),
            tx,
            packet_delay,
        }
    }

    // TODO: it mega sucks how this structure runs the hashmap lookup twice for absolutely no reason
    pub fn client_state(&self, addr: &SocketAddr) -> ClientState {
        match self.clients.borrow().contains_key(addr) {
            true => ClientState::Known,
            false => ClientState::New,
        }
    }

    pub fn connect(&self, srv: SocketAddr) -> impl Future<Item = (), Error = io::Error> {
        trace!("try to connect to server");
        let mut r = self.clients.borrow_mut();
        r.insert(srv.clone(), Client::default());

        // TODO: if this is lost we fucking lost
        self.tx.clone().send((Msg::RootUpdate(RootUpdate {
            from_blockid: [0; 32], // TODO: what is the empty state?
            to_blockref: self.blockdb.borrow().root().clone(),
            nonce: rand::random(),
        }), srv)).map(|_sender| trace!("initial rootupdate sent")).map_err(|_| unreachable!())
    }

    pub fn unconnected_root_update(&self, addr: SocketAddr, update: RootUpdate) {
        trace!("Got unconnected RootUpdate: {:?}", update);
        if update.to_blockref.blockid == self.blockdb.borrow().root().blockid {
            // nothing to do, just respond
            let blockid = self.blockdb.borrow().root().blockid;
            let key = self.blockdb.borrow().root().key;
            let msg = Msg::RootUpdateResponse(RootUpdateResponse {
                from_blockid: blockid,
                to_blockid: blockid,
                to_key: key,
            });
            trace!("send RootUpdateResponse to unconnected client");
            // if this is lost we don't care, it's just a hint, clients should poll regardless
            tokio::executor::current_thread::spawn(self.tx.clone().send((msg, addr))
                .map(|_sender| ()).map_err(|_| unreachable!()))
        } else {
            let mut blockdb = self.blockdb.borrow_mut();
            if blockdb.pending_root().is_some() {
                // ignore request until new root is done
                return;
            }
            // open connection
            self.clients.borrow_mut().insert(addr, Client::default());
            let blockid = update.to_blockref.blockid;
            blockdb.set_pending_root(update.to_blockref);
            // request new root
            trace!("register client");
            self.send_block_request(self.clients.borrow_mut().get_mut(&addr).unwrap(), BlockRequest { blockid }, addr);
        }
    }

    pub fn needs_update(&self, res: &RootUpdateResponse) -> bool {
        self.blockdb.borrow().root().blockid != res.to_blockid
    }

    pub fn root_update_response(&self, addr: SocketAddr, res: RootUpdateResponse) -> impl Future<Item = (), Error = io::Error> {
        trace!("got RootUpdateResponse: {:?}", res);
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();
        // prepare to receive a block request response, then send one out
        let (otx, orx) = oneshot::channel();
        client.pending_block_requests.insert(res.to_blockid, otx);
        // todo: be "smart" about concurrency
        // consider: A-B, A-C, B-D, C-D
        // an update from A goes to B and C perhaps simultaneously,
        // leading to B and C concurrently advertising the same state to D
        // ideally D would receive half of the blocks from B and the other half from C
        // however this requires cross-connection reasoning

        let tx = self.tx.clone();
        request_retry(orx, move || {
            (&tx).clone().send((Msg::BlockRequest(BlockRequest { blockid: res.to_blockid }), addr.clone()))
                .map(|_| ())
        })
    }

    pub fn block_request(&self, addr: SocketAddr, req: BlockRequest) -> impl Future<Item = (), Error = io::Error> {
        trace!("got BlockRequest: {:?}", req);
        let bdb = self.blockdb.borrow_mut();
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();

        let out = &mut client.transfer_out;
        // we are idempotent: is this block already allocated for transfer?
        let transfer = out.transfers.iter().find(|&(_from, _to, ref id)| id == &req.blockid).map(Clone::clone).unwrap_or_else(|| {
            // allocate transfer ids
            let id = out.transfer_cursor;
            let len = bdb.get(req.blockid.clone()).len();
            out.transfer_cursor = id + len;
            out.transfers.push((id, id + len, req.blockid.clone()));
            out.transfers.last().unwrap().clone() // clone cause i dont wanna fight with borrowck about this
        });

        // if this is lost it doesn't matter, the client will request again
        self.tx.clone().send((Msg::BlockRequestResponse(BlockRequestResponse {
            blockid: req.blockid,
            start_id: transfer.0,
            end_id: transfer.1,
            len: bdb.get(req.blockid).len(),
        }), addr))
            .map(|_sender| ()).map_err(|_| unreachable!())
    }

    pub fn block_request_response(&self, addr: SocketAddr, res: BlockRequestResponse) {
        trace!("got BlockRequestResponse: {:?}", res);
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();
        if let Some(task) = client.pending_block_requests.remove(&res.blockid) {
            task.send(()).unwrap();
        }
        let tin = &mut client.transfer_in;
        tin.transfers.push(Transfer {
            blockid: res.blockid,
            id_range: res.start_id..res.end_id,
        });
        self.blockdb.borrow_mut().add(Block::Partial(Partial {
            id: res.blockid,
            data: vec![0; res.len as usize],
            available: vec![false; (res.len as usize + CHUNK_SIZE - 1) / CHUNK_SIZE],
        }));

        if tin.transfers.len() == 1 {
            // launch new status-update sender
            let (tx, rx) = mpsc::unbounded();
            tin.status_tx = Some(tx);

            struct State {
                /// last packet time
                #[allow(unused)]
                lpt: Instant,
                /// packets since last statup update
                #[allow(unused)]
                pslu: u64,
                rx: mpsc::UnboundedReceiver<()>,
                tx: mpsc::Sender<(Msg, SocketAddr)>,
                blockdb: Rc<RefCell<BlockDb>>,
                clients: Rc<RefCell<HashMap<SocketAddr, Client>>>,
                addr: SocketAddr,
            }

            let state = State {
                lpt: Instant::now(),
                pslu: 0,
                rx,
                tx: self.tx.clone(),
                blockdb: Rc::clone(&self.blockdb),
                clients: Rc::clone(&self.clients),
                addr,
            };

            let loop_fn = future::loop_fn(state, move |state| {
                let State { lpt: _, pslu: _, rx, tx, blockdb, clients, addr } = state;
                let receiver = rx.into_future()
                    .and_then(move |(opt, rx)| {
                        if opt.is_none() {
                            return Either::A(future::ok(Loop::Break(())));
                        }

                        // convert to rle
                        let mut rle = Vec::new();
                        for t in &clients.borrow()[&addr].transfer_in.transfers {
                            let mut start = t.id_range.start;
                            for (b, group) in blockdb.borrow().get(t.blockid).partial().available.iter().cloned().group_by(|&x| x).into_iter() {
                                let count = group.count();
                                if !b {
                                    rle.push((start, start + count as u64));
                                }
                                start += count as u64;
                            }
                        }
                        trace!("sending status update: {:?}", rle);
                        // TODO: proper stuff
                        Either::B(tx.clone().send((Msg::TransferStatus(TransferStatus {
                            missing_ranges: rle,
                        }), addr))
                        .map(move |_| Loop::Continue(State {
                            lpt: Instant::now(),
                            pslu: 0,
                            rx,
                            tx,
                            blockdb,
                            clients,
                            addr,
                        })).map_err(|_| unreachable!()))
                    }).then(|res| match res {
                        Ok(state) => future::ok(state),
                        Err(_) => future::ok(Loop::Break(())),
                    });
                receiver
            });

            tokio::executor::current_thread::spawn(loop_fn);
        }
    }

    pub fn transfer_payload(&self, addr: SocketAddr, chunk: TransferPayload) {
        trace!("got TransferPayload, len {}", chunk.data.len());
        let mut clients = self.clients.borrow_mut();
        let mut blockdb = self.blockdb.borrow_mut();
        let transfer = {
            let tin = &clients[&addr].transfer_in;
            tin.status_tx.as_ref().unwrap().unbounded_send(()).unwrap();
            tin.transfer(chunk.chunkid)
        };
        {
            let partial = blockdb.get_mut(transfer.blockid).partial_mut();
            let id = (chunk.chunkid - transfer.id_range.start) as usize;
            partial.data[id * CHUNK_SIZE..(id + 1) * CHUNK_SIZE].copy_from_slice(&chunk.data);
            partial.available[id] = true;
        }
        if !blockdb.try_promote(transfer.blockid) {
            return;
        }
        // block transfer complete, maybe request further blocks
        trace!("block transfer complete");
        let dec = frontend::resolve(&*blockdb, blockdb.get(transfer.blockid).full().id, true);
        // TODO: check if block is valid
        match dec {
            Decoded::Dir(dir) => for child in dir.children {
                if !blockdb.contains(child.blockref.blockid) {
                    self.send_block_request(clients.get_mut(&addr).unwrap(), BlockRequest { blockid: child.blockref.blockid }, addr);
                }
            }
            Decoded::Meta(meta) => for leaf in meta.blocks {
                if !blockdb.contains(leaf.blockid) {
                    self.send_block_request(clients.get_mut(&addr).unwrap(), BlockRequest { blockid: leaf.blockid }, addr);
                }
            }
            Decoded::Leaf(_) => (),
        }

        if !frontend::verify(&*blockdb, true) {
            return;
        }
        // all transfers complete
        trace!("all transfers complete");
        { Frontend::from_blockdb(&*blockdb).write_to_dir(&self.folder); }
        let from = blockdb.apply_pending();
        // if this is lost we don't care, it's just a hint, clients should poll regardless
        // TODO: send to all clients
        let msg = Msg::RootUpdateResponse(RootUpdateResponse {
            from_blockid: from.blockid,
            to_blockid: blockdb.root().blockid,
            to_key: blockdb.root().key,
        });
        for &client_addr in self.clients.borrow().keys() {
            let req = self.tx.clone().send((msg.clone(), client_addr))
                .map(|_| ())
                .map_err(|_| unreachable!());
            tokio::executor::current_thread::spawn(req);
        }
        self.clients.borrow_mut().remove(&addr);
    }

    pub fn transfer_status(&self, addr: SocketAddr, status: TransferStatus) -> impl Future<Item = (), Error = io::Error> {
        trace!("got transfer status: {:?}", status);
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();

        // TODO: don't reset cursor if RTT estimate reveals that missing packets are still inflight
        let new_cursor = status.missing_ranges.first().map(|x| x.0);

        // update cursor. was transfer idle before? if yes we need to (re)start the task
        let launch_transfer = mem::replace(&mut client.transfer_out.cursor, new_cursor).is_none() && new_cursor.is_some();

        client.transfer_out.todo = status.missing_ranges;

        if launch_transfer {
            let clients = Rc::clone(&self.clients);
            let bdb = Rc::clone(&self.blockdb);
            let sender = self.tx.clone();
            let packet_delay = self.packet_delay.clone();
            Either::A(future::loop_fn(sender, move |sender| {
                let mut r = clients.borrow_mut();
                let client = r.get_mut(&addr).unwrap();

                let out = &mut client.transfer_out;

                // need an active cursor
                if let Some(cursor) = out.cursor {
                    // assumption: cursor points at something that is todo
                    // if we send, we update the cursor to the next valid todo

                    // need an allocated transfer where the cursor points
                    if let Some(&(from, _, bid)) = out.transfers.iter().find(|&(from, to, _)| cursor >= *from && cursor < *to) {
                        let cib = (cursor - from) as usize; // todo checked cast

                        // grab data from block so we can send
                        let mut bdb = bdb.borrow_mut();
                        let payload: Vec<_> = bdb.get(bid.clone()).full().data.iter().cloned().skip(cib * CHUNK_SIZE).take(CHUNK_SIZE).collect();

                        // find next valid chunk
                        let next = cursor + 1;
                        out.cursor = out.todo.iter().find(|&(_, t)| next < *t).map(|&(f, _)| cmp::max(f, next));

                        // return send task so we loop
                        let delay = Delay::new(Instant::now() + packet_delay);
                        return Either::B(sender.send((Msg::TransferPayload(TransferPayload { chunkid: cursor, data: payload.into() }), addr.clone())).map(|sender| future::Loop::Continue(sender)).map_err(|_| unreachable!()).then(move |x| delay.then(move |_| x)));
                    } else {
                        // TODO: kick the client, don't kill the server
                        panic!("client requested unallocated transfer blocks (hi frank)");
                    }
                }

                // transfer idle
                assert!(out.cursor.is_none());
                Either::A(future::ok(future::Loop::Break(())))
            }))
        } else {
            Either::B(future::ok(()))
        }
    }

    fn send_block_request(&self, client: &mut Client, req: BlockRequest, addr: SocketAddr) {
        let (otx, orx) = oneshot::channel();
        client.pending_block_requests.insert(req.blockid, otx);
        let tx = self.tx.clone();
        tokio::executor::current_thread::spawn(request_retry(orx, move || {
            (&tx).clone().send((Msg::BlockRequest(req.clone()), addr))
                .map(|_| ())
        }).map_err(|_| unreachable!()))
    }
}

fn request_retry<T, F, B>(rx: oneshot::Receiver<T>, mut f: F) -> impl Future<Item = T, Error = io::Error>
    where F: FnMut() -> B, B: IntoFuture<Item = ()>, T: ::std::fmt::Debug {
    future::loop_fn(rx, move |rx| {
        f().into_future()
            .map_err(|_| unreachable!())
            .and_then(move |()| rx.select2(Delay::new(Instant::now() + Duration::from_secs(1))))
            .map(|x| match x {
                Either::A((response, _delay)) => future::Loop::Break(response),
                Either::B(((), orx)) => future::Loop::Continue(orx),
            })
            .map_err(|e| unreachable!("{:?}", e))
    })
}
