use std::io::{self, Cursor, Write, ErrorKind};
use std::cmp;
use std::str::{self, Utf8Error};
use varmint::{self, ReadVarInt, WriteVarInt};
use byteorder::{ReadBytesExt, WriteBytesExt, LE};
use itertools::Itertools;
use bitte_ein_bit::BitMap;

pub const MTU: usize = 1460;

#[derive(Debug)]
pub struct Login<'a> {
    pub client_token: &'a [u8],
    pub command: Command<'a>,
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

fn read_bytes<'a>(cursor: &mut Cursor<&'a [u8]>) -> &'a [u8] {
    let size = cursor.read_usize_varint().unwrap();
    let pos = cursor.position() as usize;
    cursor.set_position(pos as u64 + size as u64);
    &cursor.get_ref()[pos..][..size]
}

impl<'a> Login<'a> {
    pub fn encode<W: Write>(&self, mut dst: W) {
        dst.write_usize_varint(self.client_token.len()).unwrap();
        dst.write_all(self.client_token).unwrap();
        self.command.encode(dst).unwrap();
    }

    pub fn decode(src: &'a [u8]) -> Result<Login<'a>, io::Error> {
        let mut cursor = Cursor::new(src);

        let client_token = read_bytes(&mut cursor);
        let command = Command::decode(&mut cursor)?;

        Ok(Login {
            client_token,
            command,
        })
    }
}

impl<'a> Command<'a> {
    pub fn encode<W: Write + WriteVarInt + WriteBytesExt>(&self, mut dst: W) -> Result<usize, io::Error> {
        match self {
            &Command::UploadRequest(ref req) => {
                dst.write_u8(0).unwrap();
                Ok(req.encode(dst)? + 1)
            }
        }
    }

    pub fn decode(src: &mut Cursor<&'a [u8]>) -> Result<Self, io::Error> {
        Ok(match src.read_u8()? {
            0 => Command::UploadRequest(UploadRequest::decode(src)?),
            c => panic!("Unknown Command {}", c)
        })
    }
}

impl<'a> UploadRequest<'a> {
    pub fn encode<W: Write>(&self, mut dst: W) -> Result<usize, io::Error> {
        dst.write_usize_varint(self.path.len())?;
        dst.write_all(self.path.as_bytes())?;
        dst.write_u64_varint(self.length)?;
        Ok(varmint::len_usize_varint(self.path.len()) + self.path.as_bytes().len()
            + varmint::len_u64_varint(self.length))
    }

    pub fn decode(src: &mut Cursor<&'a [u8]>) -> Result<UploadRequest<'a>, io::Error> {
        let path = str::from_utf8(read_bytes(src))
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let length = src.read_u64_varint()?;
        Ok(UploadRequest {
            path,
            length,
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

    pub fn decode(src: Vec<u8>, index_field_size: u64) -> Self {
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
    let length = length + 1; // additional space for extension messages

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
    for bit in bitmap.iter() {
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

pub struct RunlengthIter<T: AsRef<[u8]>>(Cursor<T>);

impl<T: AsRef<[u8]>> RunlengthIter<T> {
    pub fn new(t: T) -> RunlengthIter<T> {
        RunlengthIter(Cursor::new(t))
    }
}

impl<T: AsRef<[u8]>> Iterator for RunlengthIter<T> {
    type Item = u64;

    fn next(&mut self) -> Option<u64> {
        match self.0.read_u64_varint() {
            Ok(x) => Some(x),
            Err(ref e) if e.kind() == ErrorKind::UnexpectedEof => None,
            Err(e) => panic!("unexpected read error {:?}", e),
        }
    }
}

#[derive(Debug, PartialEq)]
struct MissingRange(pub u64, pub u64);

#[derive(Debug, Default)]
pub struct MissingRanges {
    missing: Vec<MissingRange>,
    cursor: u64,
}

impl MissingRanges {
    pub fn parse_status_update(&mut self, update: &[u8]) -> bool {
        self.missing.clear();
        self.missing.extend(RunlengthIter::new(update).scan(0, |a, x| { *a += x; Some(*a) })
                            .tuples().map(|(from, to)| MissingRange(from, to)));
        self.cursor = 0;
        println!("updated status {:?}", self);
        self.missing.is_empty()
    }

    pub fn advance_cursor(&self, cursor: u64) -> Option<u64> {
        let cursor = cursor + 1;
        let &MissingRange(from, _) = self.missing.iter().find(|&&MissingRange(from, to)| to > cursor)?;
        Some(cmp::max(cursor, from))
    }

    pub fn next_chunk(&mut self) -> Option<u64> {
        let &MissingRange(from, _) = self.missing.iter().find(|&&MissingRange(from, to)| to > self.cursor)?;
        self.cursor = cmp::max(self.cursor, from);
        let ret = self.cursor;
        println!("advancing cursor {:?}", self);
        self.cursor += 1;
        Some(ret)
    }
}


#[cfg(test)]
mod test {
    use super::*;

    fn test_rle_enc(bitmap: &[u8], result: &[u8]) {
        let mut enc = Vec::new();
        let mut bitmap = BitMap::new(bitmap);
        let written = write_runlength_encoded(&bitmap, &mut enc).unwrap();
        assert_eq!(enc, result);
        assert_eq!(written, result.len());
    }

    #[test]
    fn test_runlength_encode() {
        test_rle_enc(&[0b1111_1111], &[8]);
        test_rle_enc(&[0b0000_0000], &[0, 8]);
        test_rle_enc(&[0b0000_0001], &[1, 7]);
        test_rle_enc(&[0b1000_0000], &[0, 7, 1]);
        test_rle_enc(&[0b0000_1011], &[2, 1, 1, 4]);
        test_rle_enc(&[0b1000_1011], &[2, 1, 1, 3, 1]);
        test_rle_enc(&[0b1000_1011, 0b0000_1111], &[2, 1, 1, 3, 5, 4]);
    }

    #[test]
    fn test_runlength_decode() {
        let vector: &[(&[u8], &[u64])] = &[
            (&[], &[]),
            (&[0], &[0]),
            (&[1, 2, 3], &[1, 2, 3]),
            (&[1, 0x80, 1, 2], &[1, 0x80, 2]),
            (&[1, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01, 2], &[1, 0xffffffff_ffffffff, 2]),
            (&[1, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x40, 2], &[1, 0x40000000_00000000, 2]),
        ];

        for &(message, numbers) in vector {
            let decoded: Vec<_> = RunlengthIter::new(message).collect();
            assert_eq!(decoded, numbers);
        }
    }

    #[test]
    fn test_missing_ranges() {
        let mut mr = MissingRanges::default();
        mr.parse_status_update(&[2, 2, 3, 4]);
        // index:  0123456789a
        // bitmap: 11001110000
        // missing:  --   ----
        //          2,4   7,11
        assert_eq!(mr.0.as_slice(), [MissingRange(2, 4), MissingRange(7, 11)]);
        assert_eq!(mr.advance_cursor(0), Some(2));
        assert_eq!(mr.advance_cursor(1), Some(2));
        assert_eq!(mr.advance_cursor(2), Some(3));
        assert_eq!(mr.advance_cursor(3), Some(7));
        assert_eq!(mr.advance_cursor(6), Some(7));
        assert_eq!(mr.advance_cursor(7), Some(8));
        assert_eq!(mr.advance_cursor(8), Some(9));
        assert_eq!(mr.advance_cursor(9), Some(10));
        assert_eq!(mr.advance_cursor(10), None);
        assert_eq!(mr.advance_cursor(11), None);
    }
}
