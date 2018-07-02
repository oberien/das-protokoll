use tokio_io::codec::{Encoder, Decoder};
use bytes::{Bytes, BytesMut};

use blockdb::{BlockId, BlockRef, Hint, Key};

pub struct MyCodec;

impl Decoder for MyCodec {
    type Item = Msg;
    type Error = std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Msg>, Self::Error> {
        unimplemented!();
    }
}

impl Encoder for MyCodec {
    type Item = Msg;
    type Error = std::io::Error;

    fn encode(&mut self, item: Self::Item, src: &mut BytesMut) -> Result<(), Self::Error> {
        unimplemented!();
    }
}

#[derive(Debug)]
pub enum Msg {
    TransferPayload,
    TransferStatus,
    RootUpdate,
    RootUpdateResponse,
    BlockRequest,
    BlockRequestResponse,
    // use Bytes for payload data
}

pub struct TransferPayload {
    chunkid: u64,
    data: Bytes,
}

pub struct TransferStatus {
    bitmap_rle: Vec<bool>,
}

pub struct RootUpdate {
    nonce: [u8; 96],
    from_blockid: Blockid,
    to_blockref: BlockRef,
}

pub struct RootUpdateResponse {
    from_blockid: BlockId,
    to_blockid: BlockId,
    to_key: Key,
}

pub struct BlockRequest {
    blockid: BlockId,
}

pub struct BlockRequestResponse {
    blockid: BlockId,
    start_id: u64,
    end_id: u64,
}
