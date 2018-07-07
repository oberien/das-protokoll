use std::io::Cursor;
use std::iter;

use tokio_io::codec::{Encoder, Decoder};
use bytes::{Bytes, BytesMut};
use serde_cbor;
use varmint::{self, ReadVarInt};

use blockdb::{BlockId, BlockRef, Key};

pub struct MyCodec;

impl Decoder for MyCodec {
    type Item = Msg;
    type Error = ::std::io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Msg>, Self::Error> {
        // we can assume to get a full frame every time
        if src.is_empty() {
            return Ok(None);
        }
        let mut bytes = src.take().freeze();
        bytes.advance(1);
        let mut buf = Cursor::new(bytes);
        Ok(Some(match src[0] {
            0 => Msg::TransferPayload({
                let chunkid =  buf.read_u64_varint()?;
                let pos = buf.position();
                let mut data = buf.into_inner();
                data.advance(pos as usize);
                TransferPayload {
                    chunkid,
                    data,
                }
            }),
            1 => Msg::TransferStatus({
                let starting_from = buf.read_u64_varint().unwrap();
                let sum: u64 = iter::repeat(())
                    .map(|()| buf.read_u64_varint().ok())
                    .take_while(|x| x.is_some())
                    .map(|o| o.unwrap())
                    .sum();
                let mut vec = Vec::with_capacity(sum as usize);
                buf.set_position(0);
                let iter = iter::repeat(())
                    .map(|()| buf.read_u64_varint().ok())
                    .skip(1)
                    .take_while(|x| x.is_some())
                    .map(|o| o.unwrap());
                for (i, num) in iter.enumerate() {
                    for _ in 0..num {
                        vec.push(i % 2 == 0);
                    }
                }
                TransferStatus {
                    bitmap: vec,
                    starting_from,
                }
            }),
            2 => Msg::RootUpdate(serde_cbor::from_reader(buf).unwrap()),
            3 => Msg::RootUpdateResponse(serde_cbor::from_reader(buf).unwrap()),
            4 => Msg::BlockRequest(serde_cbor::from_reader(buf).unwrap()),
            5 => Msg::BlockRequestResponse(serde_cbor::from_reader(buf).unwrap()),
            _ => unimplemented!()
        }))
    }
}

impl Encoder for MyCodec {
    type Item = Msg;
    type Error = ::std::io::Error;

    fn encode(&mut self, item: Self::Item, src: &mut BytesMut) -> Result<(), Self::Error> {
        unimplemented!();
    }
}

#[derive(Debug)]
pub enum Msg {
    TransferPayload(TransferPayload),
    TransferStatus(TransferStatus),
    RootUpdate(RootUpdate),
    RootUpdateResponse(RootUpdateResponse),
    BlockRequest(BlockRequest),
    BlockRequestResponse(BlockRequestResponse),
    // use Bytes for payload data
}

#[derive(Debug)]
pub struct TransferPayload {
    chunkid: u64,
    data: Bytes,
}

#[derive(Debug)]
pub struct TransferStatus {
    bitmap: Vec<bool>,
    starting_from: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RootUpdate {
    nonce: [u8; 12],
    from_blockid: BlockId,
    to_blockref: BlockRef,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RootUpdateResponse {
    from_blockid: BlockId,
    to_blockid: BlockId,
    to_key: Key,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockRequest {
    blockid: BlockId,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockRequestResponse {
    blockid: BlockId,
    start_id: u64,
    end_id: u64,
}
