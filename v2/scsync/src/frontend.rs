use blockdb::{Full, BlockRef};

pub struct Leaf {
    pub data: Vec<u8>,
}

impl<'a> From<&'a Full> for Leaf {
    fn from(full: &'a Full) -> Self {
        let data: Vec<u8> = ::serde_cbor::from_slice(&full.data).unwrap();
        Leaf {
            data,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct Meta {
    pub blocks: Vec<BlockRef>,
}

impl<'a> From<&'a Full> for Meta {
    fn from(full: &'a Full) -> Self {
        ::serde_cbor::from_slice(&full.data).unwrap()
    }
}

#[derive(Serialize, Deserialize)]
pub struct Dir {
    pub children: Vec<Child>,
}

impl<'a> From<&'a Full> for Dir {
    fn from(full: &'a Full) -> Self {
        ::serde_cbor::from_slice(&full.data).unwrap()
    }
}

#[derive(Serialize, Deserialize)]
pub struct Child {
    pub name: String,
    pub metadata: (),
    #[serde(rename = "type")]
    pub _type: BlockType,
    pub blockref: BlockRef,
}

#[derive(Serialize, Deserialize)]
#[repr(u8)]
pub enum BlockType {
    Directory = 1,
    FileMeta = 2,
    Leaf = 3,
}
