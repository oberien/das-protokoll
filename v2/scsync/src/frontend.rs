use std::fs::{self, FileType};
use std::collections::HashMap;
use std::path::Path;

use walkdir::WalkDir;
use serde_cbor;

use blockdb::{Full, BlockRef, BlockDb, Block, Key};
use tiny_keccak;

#[derive(Debug)]
pub struct Frontend {
    blockdb: BlockDb,
}

impl Frontend {
    pub fn from_folder<P: AsRef<Path>>(folder: P) -> Frontend {
        let folder = folder.as_ref();
        assert!(folder.is_dir(), "root is not a folder");
        let walkdir = WalkDir::new(folder).contents_first(true);
        let mut map: HashMap<_, Vec<_>> = HashMap::new();
        let mut blocks = HashMap::new();

        let mut root = None;
        for file in walkdir {
            assert!(root.is_none());
            let file = file.unwrap();
            // ignore symlinks, they are always hard
            if file.file_type().is_symlink() {
                continue;
            }

            if file.file_type().is_file() {
                let content = fs::read(file.path()).unwrap();
                let block: Block = Leaf {
                    data: content,
                }.into();
                let parent = file.path().parent().unwrap().to_owned();
                let child = Child {
                    name: file.path().file_name().unwrap().to_str().unwrap().to_string(),
                    metadata: (),
                    _type: BlockType::Leaf,
                    blockref: BlockRef {
                        blockid: block.id(),
                        // TODO
                        key: Default::default(),
                        hints: Vec::new(),
                    },
                };
                map.entry(parent).or_insert_with(Default::default).push(child);
                blocks.insert(block.id(), block);
            }

            if file.file_type().is_dir() {
                let block: Block = Dir {
                    children: map.remove(file.path()).unwrap_or_default(),
                }.into();
                let id = block.id();
                blocks.insert(id, block);

                if file.path() == folder {
                    root = Some(BlockRef::new(id, Default::default(), Vec::new()));
                }
            }
        }
        Frontend {
            blockdb: BlockDb::new(root.unwrap(), blocks)
        }
    }
}

pub struct Leaf {
    pub data: Vec<u8>,
}

impl<'a> From<&'a Full> for Leaf {
    fn from(full: &'a Full) -> Self {
        let data: Vec<u8> = serde_cbor::from_slice(&full.data).unwrap();
        Leaf {
            data,
        }
    }
}

impl Into<Block> for Leaf {
    fn into(self) -> Block {
        let data = serde_cbor::to_vec(&self.data).unwrap();
        Block::Full(Full {
            id: tiny_keccak::keccak256(&data),
            data,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct Meta {
    pub blocks: Vec<BlockRef>,
}

impl<'a> From<&'a Full> for Meta {
    fn from(full: &'a Full) -> Self {
        serde_cbor::from_slice(&full.data).unwrap()
    }
}

impl Into<Block> for Meta {
    fn into(self) -> Block {
        let data = serde_cbor::to_vec(&self).unwrap();
        Block::Full(Full {
            id: tiny_keccak::keccak256(&data),
            data,
        })
    }
}

#[derive(Serialize, Deserialize)]
pub struct Dir {
    pub children: Vec<Child>,
}

impl<'a> From<&'a Full> for Dir {
    fn from(full: &'a Full) -> Self {
        serde_cbor::from_slice(&full.data).unwrap()
    }
}

impl Into<Block> for Dir {
    fn into(self) -> Block {
        let data = serde_cbor::to_vec(&self).unwrap();
        Block::Full(Full {
            id: tiny_keccak::keccak256(&data),
            data,
        })
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
