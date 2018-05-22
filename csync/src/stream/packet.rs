use varmint::{self, WriteVarInt, ReadVarInt};

use codec;

pub struct Packet {
    id_field_size: usize,
    /// buffered for easy access
    id: u64,
    /// serialized data with index in the front
    buf: Vec<u8>,
}

impl Packet {
    pub fn new(id: u64, mut buf: Vec<u8>) -> Packet {
        buf.clear();
        let id_field_size = varmint::len_u64_varint(id);
        (&mut buf).write_u64_varint(id).unwrap();
        buf.resize(codec::MTU, 0);

        Packet {
            id_field_size,
            id,
            buf,
        }
    }

    pub fn decode(src: Vec<u8>) -> Self {
        let id = (&src[..]).read_u64_varint().unwrap();

        Packet {
            id_field_size: varmint::len_u64_varint(id),
            id,
            buf: src,
        }
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn id_field_size(&self) -> usize {
        self.id_field_size
    }

    pub fn set_data_size(&mut self, data_size: usize) {
        self.buf.resize(self.id_field_size + data_size, 0);
    }

    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
}
impl AsRef<[u8]> for Packet {
    fn as_ref(&self) -> &[u8] {
        &self.buf[self.id_field_size as usize..]
    }
}

impl AsMut<[u8]> for Packet {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.buf[self.id_field_size as usize..]
    }
}
