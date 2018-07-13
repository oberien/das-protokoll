use std::rc::Rc;
use std::cell::{RefCell, Ref};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::io;
use std::time::{Instant, Duration};

use futures::{Future, IntoFuture, Sink};
use futures::future::{self, Either};

use blockdb::{BlockDb, BlockId};
use codec::{RootUpdate, RootUpdateResponse, Msg, BlockRequest, BlockRequestResponse,
    TransferPayload, TransferStatus};
use futures::unsync::{mpsc, oneshot};
use tokio::timer::Delay;

#[derive(Default)]
struct Client {
    pending_block_requests: HashMap<BlockId, oneshot::Sender<Msg>>,
}

pub enum ClientState {
    Known,
    New,
}

pub struct Handler<'a> {
    blockdb: Rc<RefCell<BlockDb>>,
    clients: Rc<RefCell<HashMap<SocketAddr, Client>>>,
    tx: &'a mpsc::Sender<(Msg, SocketAddr)>,
}

impl<'a> Handler<'a> {
    pub fn new(blockdb: BlockDb, tx: &'a mpsc::Sender<(Msg, SocketAddr)>) -> Handler {
        Handler {
            blockdb: Rc::new(RefCell::new(blockdb)),
            clients: Rc::new(RefCell::new(HashMap::new())),
            tx,
        }
    }

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
        self.tx.clone().send((Msg::BlockRequestResponse(unimplemented!()), addr))
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

    pub fn transfer_status(&self, addr: SocketAddr, status: TransferStatus) {
        unimplemented!()
    }
}

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
