use std::io;

use tokio_io::codec::{Encoder, Decoder};
use bytes::{BytesMut};

pub struct Codec;

impl Encoder for Codec {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn encode(&mut self, item: Vec<u8>, dst: &mut BytesMut) -> Result<(), Self::Error> {
        dst.extend(&item);
        Ok(())
    }
}

impl Decoder for Codec {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        Ok(Some(src.take().to_vec()))
    }
}
