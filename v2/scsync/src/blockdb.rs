use std::collections::HashMap;

type BlockId = [u8; 32];
type Key = [u8; 16];

pub struct BlockDb {
    /// Current official root
    root: BlockRef,
    /// Client: Pending update to the server
    pending_root: Option<BlockRef>,
    blocks: HashMap<BlockId, Block>,
}

impl BlockDb {
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
        self.blocks.entry(block.id).or_insert(block);
    }
}

pub struct BlockRef {
    pub blockid: BlockId,
    pub key: Key,
    pub hints: Vec<Hint>,
}

pub struct Hint {
    pub blockref: BlockRef,
    pub offset: u64,
    pub length: u64,
}

pub struct Block {
    pub id: BlockId,
    pub data: Vec<u8>,
    pub available: Vec<bool>,
}
