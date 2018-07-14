use std::collections::HashMap;

pub type BlockId = [u8; 32];
pub type Key = [u8; 16];

#[derive(Debug)]
pub struct BlockDb {
    /// Current official root
    root: BlockRef,
    /// Client: Pending update to the server
    pending_root: Option<BlockRef>,
    blocks: HashMap<BlockId, Block>,
}

impl BlockDb {
    pub fn new(root: BlockRef, blocks: HashMap<BlockId, Block>) -> BlockDb {
        BlockDb {
            root,
            pending_root: None,
            blocks,
        }
    }

    pub fn root(&self) -> &BlockRef {
        &self.root
    }

    pub fn pending_root(&self) -> Option<&BlockRef> {
        self.pending_root.as_ref()
    }

    pub fn set_pending_root(&mut self, pending_root: BlockRef) {
        self.pending_root = Some(pending_root);
    }

    pub fn get(&self, id: BlockId) -> &Block {
        &self.blocks[&id]
    }

    pub fn add(&mut self, block: Block) {
        self.blocks.entry(block.id()).or_insert(block);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BlockRef {
    pub blockid: BlockId,
    pub key: Key,
    pub hints: Vec<Hint>,
}

impl BlockRef {
    pub fn new(blockid: BlockId, key: Key, hints: Vec<Hint>) -> BlockRef {
        BlockRef {
            blockid,
            key,
            hints,
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Hint {
    pub blockref: BlockRef,
    pub offset: u64,
    pub length: u64,
}

#[derive(Debug)]
pub enum Block {
    Partial(Partial),
    Full(Full),
}

impl Block {
    pub fn id(&self) -> BlockId {
        match self {
            Block::Partial(p) => p.id,
            Block::Full(f) => f.id,
        }
    }

    pub fn full(&self) -> &Full {
        match self {
            Block::Full(full) => full,
            Block::Partial(_) => panic!("expected full block, got partial one")
        }
    }

    pub fn len(&self) -> u64 {
        match self {
            Block::Full(f) => f.data.len() as u64,
            Block::Partial(p) => p.data.len() as u64,
        }
    }
}

#[derive(Debug)]
pub struct Partial {
    pub id: BlockId,
    pub data: Vec<u8>,
    /// one bool per byte
    pub available: Vec<bool>,
}

#[derive(Debug)]
pub struct Full {
    pub id: BlockId,
    pub data: Vec<u8>,
}
