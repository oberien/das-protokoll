use std::io::Cursor;
use std::io::ErrorKind;
use std::iter;

use tokio_io::codec::{Encoder, Decoder};
use bytes::{Bytes, BytesMut, BufMut};
use serde_cbor;
use varmint::{self, ReadVarInt, WriteVarInt};
use itertools::Itertools;

use blockdb::{BlockId, BlockRef, Key};

pub const MTU: usize = 1460;

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
                TransferStatus { missing_ranges: VarintIter(buf).scan(0, |a, x| { *a += x; Some(*a) }).tuples().collect() }
            }),
            2 => Msg::RootUpdate(serde_cbor::from_reader(buf).unwrap()),
            3 => Msg::RootUpdateResponse(serde_cbor::from_reader(buf).unwrap()),
            4 => Msg::BlockRequest(serde_cbor::from_reader(buf).unwrap()),
            5 => Msg::BlockRequestResponse(serde_cbor::from_reader(buf).unwrap()),
            _ => unimplemented!()
        }))
    }
}

pub struct VarintIter<T: AsRef<[u8]>>(Cursor<T>);

impl<T: AsRef<[u8]>> VarintIter<T> {
    pub fn new(t: T) -> VarintIter<T> {
        VarintIter(Cursor::new(t))
    }
}

impl<T: AsRef<[u8]>> Iterator for VarintIter<T> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        match self.0.read_u64_varint() {
            Ok(x) => Some(x),
            Err(ref e) if e.kind() == ErrorKind::UnexpectedEof => None,
            Err(e) => panic!("unexpected read error {:?}", e),
        }
    }
}


impl Encoder for MyCodec {
    type Item = Msg;
    type Error = ::std::io::Error;

    fn encode(&mut self, item: Msg, dst: &mut BytesMut) -> Result<(), Self::Error> {
        // can't be more than MTU anyways
        dst.reserve(MTU);
        match item {
            Msg::TransferPayload(payload) => {
                let varint_len = varmint::len_u64_varint(payload.chunkid);
                let len = 1 + varint_len + payload.data.len();
                assert!(len <= MTU);
                dst.put_u8(0);
                dst.writer().write_u64_varint(payload.chunkid).unwrap();
                dst.extend_from_slice(&payload.data);
            }
            Msg::TransferStatus(status) => {
                use std::iter::once;
                dst.put_u8(1);
                for value in status.missing_ranges.into_iter().flat_map(|(from, to)| once(from).chain(once(to)))
                    .scan(0, |a, mut x| { x -= *a; *a += x; Some(x) }) {
                    dst.writer().write_u64_varint(value)?;
                }
                dst.truncate(MTU);
            }
            Msg::RootUpdate(update) => serde_cbor::to_writer(&mut dst.writer(), &update).unwrap(),
            Msg::RootUpdateResponse(res) => serde_cbor::to_writer(&mut dst.writer(), &res).unwrap(),
            Msg::BlockRequest(req) => serde_cbor::to_writer(&mut dst.writer(), &req).unwrap(),
            Msg::BlockRequestResponse(res) => serde_cbor::to_writer(&mut dst.writer(), &res).unwrap(),
        }
        Ok(())
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
    pub chunkid: u64,
    pub data: Bytes,
}

#[derive(Debug)]
pub struct TransferStatus {
    pub missing_ranges: Vec<(u64, u64)>, // from, to
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RootUpdate {
    pub nonce: [u8; 12],
    pub from_blockid: BlockId,
    pub to_blockref: BlockRef,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RootUpdateResponse {
    pub from_blockid: BlockId,
    pub to_blockid: BlockId,
    pub to_key: Key,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockRequest {
    pub blockid: BlockId,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockRequestResponse {
    pub blockid: BlockId,
    pub start_id: u64,
    pub end_id: u64,
    pub len: u64,
}
