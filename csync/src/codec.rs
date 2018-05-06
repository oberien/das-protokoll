use std::io::{self, Cursor, Write};

use std::str::{self, Utf8Error};
use varmint::{self, ReadVarInt, WriteVarInt};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};
use bitte_ein_bit::BitMap;

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
    pub length: u64,
}

pub struct Chunk {
    index_field_size: u64,
    /// buffered for easy access
    pub index: u64,
    /// serialized data with index in the front
    pub buf: Vec<u8>,
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
            &Command::UploadRequest(ref req) => {
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
            length: length,
        })
    }
}

impl Chunk {
    pub fn new(mut buf: Vec<u8>, index: u64, index_field_size: u64, data_size: usize) -> Chunk {
        buf.clear();
        debug_assert!(0 < index_field_size && index_field_size <= 8);

        (&mut buf).write_u64::<LE>(index).unwrap();
        buf.resize(index_field_size as usize + data_size, 0);

        Chunk {
            index_field_size,
            index,
            buf,
        }
    }

    pub fn decode(src: Vec<u8>, size: usize, index_field_size: u64) -> Self {
        let mut buf = [0u8; 8];
        (&mut buf[..index_field_size as usize]).copy_from_slice(&src[..index_field_size as usize]);
        let index = (&buf[..]).read_u64::<LE>().unwrap();

        Chunk {
            index_field_size,
            index,
            buf: src,
        }
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}

impl AsRef<[u8]> for Chunk {
    fn as_ref(&self) -> &[u8] {
        &self.buf[self.index_field_size as usize..]
    }
}

impl AsMut<[u8]> for Chunk {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf[self.index_field_size as usize..]
    }
}

pub struct ChunkInfo {
    pub index_field_size: u64,
    pub chunk_size: u64,
    pub num_chunks: u64,
    pub last_chunk_size: u64,
}

/// Calculates and returns the ChunkInfo for the given file length.
pub fn index_field_size(length: u64) -> ChunkInfo {
    let mut index_field_size = 1;
    let mut chunk_size;
    let mut num_chunks;
    loop {
        chunk_size = MTU as u64 - index_field_size;
        // prevent overflow
        num_chunks = length / chunk_size + (length % chunk_size != 0) as u64;
        if num_chunks <= 1 << (index_field_size * 8) {
            break;
        }
        index_field_size += 1;
    }

    ChunkInfo {
        index_field_size,
        chunk_size,
        num_chunks,
        last_chunk_size: length % chunk_size,
    }
}

pub fn write_runlength_encoded<T, W>(bitmap: &BitMap<T>, mut w: W) -> io::Result<usize>
where
    T: AsRef<[u8]>,
    W: Write,
{
    let mut count = 0;
    let mut prev_val = true;
    let mut written = 0;
    for (i, bit) in bitmap.iter().enumerate() {
        if bit == prev_val {
            count += 1;
            continue;
        }
        match w.write_u64_varint(count) {
            Ok(()) => written += varmint::len_u64_varint(count),
            Err(ref e) if e.kind() == io::ErrorKind::WriteZero => return Ok(written),
            e => e?
        }
        prev_val = bit;
        count = 1;
    }
    match w.write_u64_varint(count) {
        Ok(()) => written += varmint::len_u64_varint(count),
        Err(ref e) if e.kind() == io::ErrorKind::WriteZero => return Ok(written),
        e => e?
    }
    Ok(written)
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_runlength() {
        let mut bitmap = BitMap::new([0b0000_1011u8]);
        let mut enc = [0u8; 5];
        let written = write_runlength_encoded(&bitmap, Cursor::new(&mut enc[..])).unwrap();
        assert_eq!(written, 4);
        assert_eq!(enc, [2, 1, 1, 4, 0]);

        let mut bitmap = BitMap::new([0b1000_1011u8]);
        let mut enc = [0u8; 5];
        let written = write_runlength_encoded(&bitmap, Cursor::new(&mut enc[..])).unwrap();
        assert_eq!(written, 5);
        assert_eq!(enc, [2, 1, 1, 3, 1]);
    }
}