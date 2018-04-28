use std::io::{Cursor, Write};

use std::str::{self, Utf8Error};
use varmint::{self, ReadVarInt, WriteVarInt};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};

pub const MTU: usize = 1460;

#[derive(Debug)]
pub struct Login<'a> {
    pub client_token: &'a [u8],
}

#[derive(Debug)]
pub enum Command<'a> {
    UploadRequest(UploadRequest<'a>),
}

#[derive(Debug)]
pub struct UploadRequest<'a> {
    pub path: &'a str,
    pub length: usize,
}

pub struct Chunk {
    pub index: u64,
    pub data: [u8; MTU],
    pub offset: usize,
    pub size: usize,
}

impl<'a> Login<'a> {
    pub fn encode<W: Write>(&self, mut dst: W) -> usize {
        dst.write_all(self.client_token).unwrap();
        self.client_token.len()
    }

    pub fn decode(src: &'a [u8]) -> Self {
        Login {
            client_token: src,
        }
    }
}

impl<'a> Command<'a> {
    pub fn encode<W: Write + WriteVarInt + WriteBytesExt>(&self, mut dst: W) -> usize {
        match self {
            Command::UploadRequest(req) => {
                dst.write_u8(0).unwrap();
                req.encode(dst)
            }
        }
    }

    pub fn decode(src: &'a [u8]) -> Result<Self, Utf8Error> {
        Ok(match src[0] {
            0 => Command::UploadRequest(UploadRequest::decode(&src[1..])?),
            c => panic!("Unknown Command {}", c)
        })
    }
}

impl<'a> UploadRequest<'a> {
    fn encode<W: Write + WriteVarInt>(&self, mut dst: W) -> usize {
        let length_len = varmint::len_u64_varint(self.length as u64);
        let total_len = length_len + self.path.len();

        dst.write_u64_varint(self.length as u64).unwrap();
        dst.write_all(self.path.as_bytes()).unwrap();
        total_len
    }

    fn decode(src: &'a [u8]) -> Result<Self, Utf8Error> {
        let mut src = Cursor::new(src);
        let length = src.read_u64_varint().unwrap();
        let pos = src.position() as usize;
        let path = &src.into_inner()[pos..];
        Ok(UploadRequest {
            path: str::from_utf8(path)?,
            length: length as usize,
        })
    }
}

impl Chunk {
    pub fn encode<W: Write>(&self, mut dst: W, index_field_size: usize) -> usize {
        debug_assert!(0 < index_field_size && index_field_size <= 8);
        debug_assert!(self.data.len() as u64 + index_field_size as u64 <= MTU as u64);

        let mut arr = [0u8; 8];
        (&mut arr[..]).write_u64::<LE>(self.index as u64).unwrap();
        dst.write_all(&arr[..index_field_size]).unwrap();
        dst.write_all(&self.data[self.offset..self.size]).unwrap();

        index_field_size + self.data.len()
    }

    pub fn decode(src: [u8; MTU], size: usize, index_field_size: usize) -> Self {
        let mut buf = [0u8; 8];
        (&mut buf[..index_field_size]).copy_from_slice(&src[..index_field_size]);
        let index = (&buf[..]).read_u64::<LE>().unwrap();

        Chunk {
            index,
            data: src,
            offset: index_field_size,
            size,
        }
    }
}

pub fn index_field_size(mtu: usize, length: usize) -> usize {
    let mut index_field_size = 1;
    loop {
        let size = mtu - index_field_size;
        let num = (length + size - 1) / size;
        if num <= 1 << (index_field_size * 8 - 1) {
            return index_field_size;
        }
        index_field_size += 1;
    }
}
