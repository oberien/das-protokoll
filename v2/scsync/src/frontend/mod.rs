use std::fs::{self, File};
use std::collections::HashMap;
use std::path::Path;
use std::io::{Cursor, Write};

use serde::{Serialize, Deserialize};
use walkdir::WalkDir;
use serde_cbor::{self, error::Result};
use tiny_keccak;
use crypto::aessafe::{AesSafe128Encryptor, AesSafe128Decryptor};
use crypto::util;
use crypto::aesni::{AesNiEncryptor, AesNiDecryptor};
use crypto::aes::KeySize;
use rand::{Rng, OsRng};
use aesstream::{AesWriter, AesReader};

use blockdb::{Full, BlockRef, BlockDb, Block, Key, BlockId};

mod visitor;

use self::visitor::{ResolveBlockVisitor, VerifyVisitor};

#[derive(Debug)]
pub struct Frontend<'a> {
    blockdb: &'a BlockDb,
}

impl<'a> Frontend<'a> {
    pub fn from_blockdb(blockdb: &'a BlockDb) -> Frontend<'a> {
        Frontend {
            blockdb,
        }
    }

    pub fn blockdb_from_folder<P: AsRef<Path>>(folder: P) -> BlockDb {
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
                let (key, block) = Leaf {
                    data: content,
                }.to_block();
                let parent = file.path().parent().unwrap().to_owned();
                let child = Child {
                    name: file.file_name().to_string_lossy().into_owned(),
                    metadata: (),
                    _type: BlockType::Leaf,
                    blockref: BlockRef {
                        blockid: block.id(),
                        key,
                        hints: Vec::new(),
                    },
                };
                map.entry(parent).or_insert_with(Default::default).push(child);
                blocks.insert(block.id(), block);
            }

            if file.file_type().is_dir() {
                let (key, block) = Dir {
                    children: map.remove(file.path()).unwrap_or_default(),
                }.to_block();
                let id = block.id();
                blocks.insert(id, block);
                map.entry(file.path().parent().unwrap().to_owned()).or_insert_with(Default::default).push(Child {
                    name: file.file_name().to_string_lossy().into_owned(),
                    metadata: (),
                    _type: BlockType::Directory,
                    blockref: BlockRef {
                        blockid: id,
                        key: key,
                        hints: Vec::new(),
                    }
                });

                if file.path() == folder {
                    root = Some(BlockRef::new(id, key, Vec::new()));
                }
            }
        }
        BlockDb::new(root.unwrap(), blocks)
    }

    pub fn write_to_dir<P: AsRef<Path>>(&self, folder: P) {
        // clear folder
        let folder = folder.as_ref();
        fs::remove_dir_all(folder).unwrap();
        fs::create_dir(folder).unwrap();
        let BlockRef { blockid, key, .. } = *self.blockdb.root();
        let dir = Dir::from_full(self.blockdb.get(blockid).full(), &key).unwrap();
        self.write_to_dir_rec(folder, dir);
    }

    fn write_to_dir_rec<P: AsRef<Path>>(&self, folder: P, dir: Dir) {
        let folder = folder.as_ref();
        for Child { name, _type, blockref: BlockRef { blockid, key, .. }, .. } in dir.children {
            let path = folder.join(name);
            match _type {
                BlockType::Leaf => {
                    let mut file = File::create(path).unwrap();
                    let leaf = Leaf::from_full(self.blockdb.get(blockid).full(), &key).unwrap();
                    file.write_all(&leaf.data).unwrap();
                }
                BlockType::Directory => {
                    fs::create_dir(&path).unwrap();
                    let dir = Dir::from_full(self.blockdb.get(blockid).full(), &key).unwrap();
                    self.write_to_dir_rec(path, dir);
                }
                BlockType::FileMeta => {
                    let mut file = File::create(path).unwrap();
                    let meta = Meta::from_full(self.blockdb.get(blockid).full(), &key).unwrap();
                    for BlockRef { blockid, key, .. } in meta.blocks {
                        let leaf = Leaf::from_full(self.blockdb.get(blockid).full(), &key).unwrap();
                        file.write_all(&leaf.data).unwrap();
                    }
                }
            }
        }
    }

    // TODO: inotify with diff
}

#[derive(Debug)]
pub struct Leaf {
    pub data: Vec<u8>,
}

impl Leaf {
    pub fn from_full(full: &Full, key: &Key) -> Result<Self> {
        Ok(Leaf {
            data: from_full(full, key)?,
        })
    }

    fn to_block(&self) -> (Key, Block) {
        to_block(&self.data, self.data.len() + 100)
    }
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Meta {
    pub blocks: Vec<BlockRef>,
}

impl Meta {
    pub fn from_full(full: &Full, key: &Key) -> Result<Self> {
        from_full(full, key)
    }

    #[allow(unused)]
    fn to_block(&self) -> (Key, Block) {
        to_block(self, self.blocks.len() * 200)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Dir {
    pub children: Vec<Child>,
}

impl Dir {
    pub fn from_full(full: &Full, key: &Key) -> Result<Self> {
        from_full(full, key)
    }

    fn to_block(&self) -> (Key, Block) {
        to_block(self, self.children.len() * 1024)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Child {
    pub name: String,
    pub metadata: (),
    #[serde(rename = "type")]
    pub _type: BlockType,
    pub blockref: BlockRef,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug)]
#[repr(u8)]
pub enum BlockType {
    Directory = 1,
    FileMeta = 2,
    Leaf = 3,
}

#[derive(Debug)]
pub enum Decoded {
    Dir(Dir),
    Meta(Meta),
    Leaf(Leaf),
}

pub fn resolve(blockdb: &BlockDb, blockid: BlockId, pending: bool) -> Decoded {
    let root = if pending {
        blockdb.pending_root().unwrap()
    } else {
        blockdb.root()
    };
    visitor::traverse(blockdb, root, &mut ResolveBlockVisitor(blockid)).unwrap()
}

pub fn verify(blockdb: &BlockDb, pending: bool) -> bool {
    let root = if pending {
        blockdb.pending_root().unwrap()
    } else {
        blockdb.root()
    };
    visitor::traverse(blockdb, root, &mut VerifyVisitor).is_none()
}

fn from_full<T: Deserialize<'static>>(full: &Full, key: &Key) -> Result<T> {
    trace!("start decryption: {:?}", full.id);
    let enc = if util::supports_aesni() {
        let decryptor = AesNiDecryptor::new(KeySize::KeySize128, key);
        let reader = AesReader::new(Cursor::new(&full.data), decryptor).unwrap();
        serde_cbor::from_reader(reader)
    } else {
        let decryptor = AesSafe128Decryptor::new(key);
        let reader = AesReader::new(Cursor::new(&full.data), decryptor).unwrap();
        serde_cbor::from_reader(reader)
    };
    trace!("end decryption: {:?}", full.id);
    enc
}

fn to_block<T: Serialize>(t: &T, cap_hint: usize) -> (Key, Block) {
    let mut data = Vec::with_capacity(cap_hint);
    let key: Key = OsRng::new().unwrap().gen();

    trace!("start encryption");
    if util::supports_aesni() {
        let encryptor = AesNiEncryptor::new(KeySize::KeySize128, &key);
        let mut writer = AesWriter::new(&mut data, encryptor).unwrap();
        serde_cbor::to_writer(&mut writer, t).unwrap();
    } else {
        let encryptor = AesSafe128Encryptor::new(&key);
        let mut writer = AesWriter::new(&mut data, encryptor).unwrap();
        serde_cbor::to_writer(&mut writer, t).unwrap();
    }
    trace!("end encryption");
    (key, Block::Full(Full {
        id: tiny_keccak::keccak256(&data),
        data,
    }))
}