use std::fs::{self, File};
use std::collections::HashMap;
use std::path::Path;
use std::io::{Cursor, Write};

use serde::{Serialize, Deserialize};
use walkdir::WalkDir;
use serde_cbor;
use tiny_keccak;
use crypto::aessafe::{AesSafe128Encryptor, AesSafe128Decryptor};
use rand::{Rng, OsRng};
use aesstream::{AesWriter, AesReader};

use blockdb::{Full, BlockRef, BlockDb, Block, Key, Hint};

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
                let (key, block) = Leaf {
                    data: content,
                }.to_block();
                let parent = file.path().parent().unwrap().to_owned();
                let child = Child {
                    name: file.path().file_name().unwrap().to_str().unwrap().to_string(),
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

                if file.path() == folder {
                    root = Some(BlockRef::new(id, key, Vec::new()));
                }
            }
        }
        Frontend {
            blockdb: BlockDb::new(root.unwrap(), blocks)
        }
    }

    pub fn write_to_dir<P: AsRef<Path>>(&self, folder: P) {
        // clear folder
        let folder = folder.as_ref();
        fs::remove_dir_all(folder).unwrap();
        fs::create_dir(folder).unwrap();
        let BlockRef { blockid, key, .. } = *self.blockdb.root();
        let dir = Dir::from_full(self.blockdb.get(blockid).full(), &key);
        self.write_to_dir_rec(folder, dir);
    }

    fn write_to_dir_rec<P: AsRef<Path>>(&self, folder: P, dir: Dir) {
        let folder = folder.as_ref();
        for Child { name, _type, blockref: BlockRef { blockid, key, .. }, .. } in dir.children {
            let path = folder.join(name);
            match _type {
                BlockType::Leaf => {
                    let mut file = File::create(path).unwrap();
                    let leaf = Leaf::from_full(self.blockdb.get(blockid).full(), &key);
                    file.write_all(&leaf.data).unwrap();
                }
                BlockType::Directory => {
                    fs::create_dir(&path).unwrap();
                    let dir = Dir::from_full(self.blockdb.get(blockid).full(), &key);
                    self.write_to_dir_rec(path, dir);
                }
                BlockType::FileMeta => {
                    let mut file = File::create(path).unwrap();
                    let meta = Meta::from_full(self.blockdb.get(blockid).full(), &key);
                    for Hint { blockref: BlockRef { blockid, key, .. }, offset, length } in meta.blocks {
                        let leaf = Leaf::from_full(self.blockdb.get(blockid).full(), &key);
                        file.write_all(&leaf.data[offset as usize..offset as usize + length as usize]).unwrap();
                    }
                }
            }
        }
    }

    // TODO: inotify with diff
}

pub struct Leaf {
    pub data: Vec<u8>,
}

impl Leaf {
    fn from_full(full: &Full, key: &Key) -> Self {
        Leaf {
            data: from_full(full, key),
        }
    }

    fn to_block(&self) -> (Key, Block) {
        to_block(&self.data)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Meta {
    pub blocks: Vec<Hint>,
}

impl Meta {
    fn from_full(full: &Full, key: &Key) -> Self {
        from_full(full, key)
    }

    fn to_block(&self) -> (Key, Block) {
        to_block(self)
    }
}

#[derive(Serialize, Deserialize)]
pub struct Dir {
    pub children: Vec<Child>,
}

impl Dir {
    fn from_full(full: &Full, key: &Key) -> Self {
        from_full(full, key)
    }

    fn to_block(&self) -> (Key, Block) {
        to_block(self)
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

fn from_full<T: Deserialize<'static>>(full: &Full, key: &Key) -> T {
    let decryptor = AesSafe128Decryptor::new(key);
    let reader = AesReader::new(Cursor::new(&full.data), decryptor).unwrap();
    serde_cbor::from_reader(reader).unwrap()
}

fn to_block<T: Serialize>(t: &T) -> (Key, Block) {
    let mut data = Vec::new();
    let key: Key = OsRng::new().unwrap().gen();
    let encryptor = AesSafe128Encryptor::new(&key);
    {
        let mut writer = AesWriter::new(&mut data, encryptor).unwrap();
        serde_cbor::to_writer(&mut writer, t).unwrap();
    }
    (key, Block::Full(Full {
        id: tiny_keccak::keccak256(&data),
        data,
    }))
}