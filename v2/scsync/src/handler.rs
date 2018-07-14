use std::rc::Rc;
use std::cell::RefCell;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::{io, mem, cmp};
use std::time::{Instant, Duration};

use futures::{Future, IntoFuture, Sink};
use futures::future::{self, Either};
use futures::unsync::{mpsc, oneshot};
use tokio::timer::Delay;
use rand;

use blockdb::{BlockDb, BlockId};
use codec::{RootUpdate, RootUpdateResponse, Msg, BlockRequest, BlockRequestResponse,
    TransferPayload, TransferStatus};

#[derive(Default)]
struct Client {
    pending_block_requests: HashMap<BlockId, oneshot::Sender<Msg>>,
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

#[derive(Default)]
struct TransferIn {
}


pub enum ClientState {
    Known,
    New,
}

pub struct Handler<'a> {
    blockdb: Rc<RefCell<BlockDb>>,
    clients: Rc<RefCell<HashMap<SocketAddr, Client>>>,
    tx: &'a mpsc::Sender<(Msg, SocketAddr)>,
    packet_delay: Duration,
}

impl<'a> Handler<'a> {
    pub fn new(packet_delay: Duration, blockdb: BlockDb, tx: &'a mpsc::Sender<(Msg, SocketAddr)>) -> Handler {
        Handler {
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

    pub fn unconnected_root_update(&self, addr: SocketAddr, update: RootUpdate) -> impl Future<Item = (), Error = io::Error> {
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();
        let response = if update.to_blockref.blockid == self.blockdb.borrow().root().blockid {
            // nothing to do, just respond
            let blockid = self.blockdb.borrow().root().blockid;
            let key = self.blockdb.borrow().root().key;
            Msg::RootUpdateResponse(RootUpdateResponse {
                from_blockid: blockid,
                to_blockid: blockid,
                to_key: key,
            })
        } else {
            // open connection
            self.clients.borrow_mut().insert(addr, Client::default());
            // TODO: traverse tree, request packets
            unimplemented!()
        };
        self.tx.clone().send((response, addr))
            .map(|_sender| ()).map_err(|_| unreachable!())
    }

    pub fn needs_update(&self, res: &RootUpdateResponse) -> bool {
        self.blockdb.borrow().root().blockid != res.to_blockid
    }

    pub fn root_update_response(&self, addr: SocketAddr, res: RootUpdateResponse) -> impl Future<Item = (), Error = io::Error> {
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();
        // prepare to receive a block request response, then send one out
        let (otx, orx) = oneshot::channel();
        client.pending_block_requests.insert([0; 32], otx);
        // todo: be "smart" about concurrency
        // consider: A-B, A-C, B-D, C-D
        // an update from A goes to B and C perhaps simultaneously,
        // leading to B and C concurrently advertising the same state to D
        // ideally D would receive half of the blocks from B and the other half from C
        // however this requires cross-connection reasoning

        let tx = self.tx.clone();
        request_retry(orx, move || {
            (&tx).clone().send((Msg::BlockRequest(unimplemented!()), addr.clone()))
                .map(|_| ())
        }).map(|msg| ())
    }

    pub fn block_request(&self, addr: SocketAddr, req: BlockRequest) -> impl Future<Item = (), Error = io::Error> {
        let mut bdb = self.blockdb.borrow_mut();
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();

        let out = &mut client.transfer_out;
        // we are idempotent: is this block already allocated for transfer?
        let transfer = out.transfers.iter().find(|&(from, to, ref id)| id == &req.blockid).map(Clone::clone).unwrap_or_else(|| {
            // allocate transfer ids
            let id = out.transfer_cursor;
            let len = bdb.get(req.blockid.clone()).len();
            out.transfer_cursor = id + len;
            out.transfers.push((id, id + len, req.blockid.clone()));
            out.transfers.last().unwrap().clone() // clone cause i dont wanna fight with borrowck about this
        });

        self.tx.clone().send((Msg::BlockRequestResponse(BlockRequestResponse {
            blockid: req.blockid,
            start_id: transfer.0,
            end_id: transfer.1,
        }), addr))
            .map(|_sender| ()).map_err(|_| unreachable!())
    }

    pub fn block_request_response(&self, addr: SocketAddr, res: BlockRequestResponse) {
        let mut r = self.clients.borrow_mut();
        let client = r.get_mut(&addr).unwrap();

        if let Some(task) = client.pending_block_requests.remove(&[0; 32]) {
            task.send(Msg::BlockRequestResponse(res)).unwrap();
        }
    }

    pub fn transfer_payload(&self, addr: SocketAddr, payload: TransferPayload) {
        unimplemented!()
    }

    pub fn transfer_status(&self, addr: SocketAddr, status: TransferStatus) -> impl Future<Item = (), Error = io::Error> {
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
}

// TODO dynamic chunk size scaling
// not yet implented
// for now small constant size and hope for the best
const CHUNK_SIZE: usize = 1000;

fn request_retry<T, F, B>(rx: oneshot::Receiver<T>, mut f: F) -> impl Future<Item = T, Error = io::Error>
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
