use std::io::{Cursor, Write};

use std::str::{self, Utf8Error};
use varmint::{self, ReadVarInt, WriteVarInt};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};

pub const MTU: usize = 1500;

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

#[derive(Debug)]
pub struct Chunk<'a> {
    pub index: u64,
    pub data: &'a [u8],
}

impl<'a> Login<'a> {
    pub fn encode(&self, dst: &mut [u8]) -> usize {
        debug_assert!(dst.len() >= self.client_token.len());
        (&mut dst[..]).write_all(self.client_token).unwrap();
        self.client_token.len()
    }

    pub fn decode(src: &'a [u8]) -> Self {
        Login {
            client_token: src,
        }
    }
}

impl<'a> Command<'a> {
    pub fn encode(&self, dst: &mut [u8]) -> usize {
        match self {
            Command::UploadRequest(req) => {
                dst[0] = 0;
                req.encode(&mut dst[1..])
            }
        }
    }

    pub fn decode(src: &'a [u8]) -> Result<Self, Utf8Error> {
        Ok(match src[0] {
            0 => Command::UploadRequest(UploadRequest::decode(&src[1..])?),
            _ => panic!("Unknown Command")
        })
    }
}

impl<'a> UploadRequest<'a> {
    fn encode(&self, dst: &mut [u8]) -> usize {
        let path_len = varmint::len_u64_varint(self.path.len() as u64);
        let length_len = varmint::len_u64_varint(self.length as u64);
        let total_len = path_len + length_len + self.path.len();
        debug_assert!(dst.len() >= total_len);
        let mut dst = Cursor::new(dst);

        dst.write_u64_varint(self.length as u64).unwrap();
        dst.write_u64_varint(self.path.len() as u64).unwrap();
        dst.write_all(self.path.as_bytes()).unwrap();
        total_len
    }

    fn decode(src: &'a [u8]) -> Result<Self, Utf8Error> {
        let mut src = Cursor::new(src);
        let length = src.read_u64_varint().unwrap();
        let path_len = src.read_u64_varint().unwrap();
        debug_assert_eq!(src.get_ref().len() as u64, src.position() + path_len);
        let pos = src.position() as usize;
        let path = &src.into_inner()[pos..pos + path_len as usize];
        Ok(UploadRequest {
            path: str::from_utf8(path)?,
            length: length as usize,
        })
    }
}

impl<'a> Chunk<'a> {
    pub fn encode(&self, dst: &mut [u8], index_field_size: usize) -> usize {
        debug_assert!(0 < index_field_size && index_field_size <= 8);
        debug_assert!(self.data.len() as u64 + index_field_size as u64 <= MTU as u64);
        debug_assert!(dst.len() >= index_field_size + self.data.len());

        let mut dst = Cursor::new(dst);
        let mut arr = [0u8; 8];
        (&mut arr[..]).write_u64::<LE>(self.index as u64).unwrap();
        dst.write_all(&arr[..index_field_size]).unwrap();
        dst.write_all(self.data).unwrap();

        index_field_size + self.data.len()
    }

    pub fn decode(src: &'a [u8], index_field_size: usize) -> Self {
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&src[..index_field_size]);
        let index = (&buf[..]).read_u64::<LE>().unwrap();

        Chunk {
            index,
            data: &src[index_field_size..],
        }
    }
}

