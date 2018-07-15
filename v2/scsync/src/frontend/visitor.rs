use std::path::{Path, PathBuf};

use blockdb::{BlockDb, BlockRef, BlockId, Full, Partial, Block};
use super::{Dir, Meta, Leaf, Child, Decoded, BlockType};

pub trait Visitor<T> {
    fn visit_dir<P: AsRef<Path>>(&mut self, path: P, block: &Full, dir: &Dir) -> Option<T>;
    fn visit_meta<P: AsRef<Path>>(&mut self, blockdb: &BlockDb, path: P, block: &Full, meta: Meta) -> Option<T>;
    /// doesn't visit meta-leaves
    fn visit_leaf<P: AsRef<Path>>(&mut self, path: P, block: &Full, leaf: Leaf) -> Option<T>;
    fn visit_partial<P: AsRef<Path>>(&mut self, path: P, block: &Partial) -> Option<T>;
    fn visit_missing<P: AsRef<Path>>(&mut self, path: P, blockid: BlockId) -> Option<T>;
}

pub fn traverse<T, V: Visitor<T>>(blockdb: &BlockDb, root: &BlockRef, visitor: &mut V) -> Option<T> {
    let full = blockdb.get(root.blockid).full();
    let dir = Dir::from_full(full, &root.key).unwrap();
    let path = PathBuf::new();
    if let Some(t) = visitor.visit_dir(&path, full, &dir) {
        return Some(t);
    }
    traverse_rec(blockdb, path, root, Decoded::Dir(dir), visitor)
}

fn traverse_rec<T, V: Visitor<T>>(blockdb: &BlockDb, path: PathBuf, bref: &BlockRef, dec: Decoded, visitor: &mut V) -> Option<T> {
    if !blockdb.contains(bref.blockid) {
        return visitor.visit_missing(path, bref.blockid);
    }
    let block = blockdb.get(bref.blockid);
    if let Block::Partial(partial) = block {
        return visitor.visit_partial(path, partial);
    }
    let full = block.full();

    match dec {
        Decoded::Dir(dir) => {
            if let Some(t) = visitor.visit_dir(&path, full, &dir) {
                return Some(t);
            }

            for Child { name, _type, blockref, .. } in dir.children {
                if !blockdb.contains(blockref.blockid) {
                    if let Some(t) = visitor.visit_missing(&path, bref.blockid) {
                        return Some(t);
                    }
                    continue;
                }
                let block = blockdb.get(blockref.blockid);
                if let Block::Partial(partial) = block {
                    if let Some(t) = visitor.visit_partial(&path, partial) {
                        return Some(t);
                    }
                    continue;
                }

                let full = block.full();
                let path = path.join(name);
                let dec = match _type {
                    BlockType::Directory => Decoded::Dir(Dir::from_full(full, &blockref.key).unwrap()),
                    BlockType::FileMeta => Decoded::Meta(Meta::from_full(full, &blockref.key).unwrap()),
                    BlockType::Leaf => Decoded::Leaf(Leaf::from_full(full, &blockref.key).unwrap()),
                };
                if let Some(t) = traverse_rec(blockdb, path, &blockref, dec, visitor) {
                    return Some(t);
                }
            }
        }
        Decoded::Meta(meta) => if let Some(t) = visitor.visit_meta(blockdb, path,full, meta) {
            return Some(t);
        }
        Decoded::Leaf(leaf) => if let Some(t) = visitor.visit_leaf(path, full, leaf) {
            return Some(t);
        }
    }

    None
}

// TODO: pub struct WriteToDirVisitor;

pub struct ResolveBlockVisitor(pub BlockId);

impl Visitor<Decoded> for ResolveBlockVisitor {
    fn visit_dir<P: AsRef<Path>>(&mut self, _path: P, block: &Full, dir: &Dir) -> Option<Decoded> {
        if block.id == self.0 {
            Some(Decoded::Dir(dir.clone()))
        } else {
            None
        }
    }

    fn visit_meta<P: AsRef<Path>>(&mut self, _blockdb: &BlockDb, _path: P, block: &Full, meta: Meta) -> Option<Decoded> {
        if block.id == self.0 {
            Some(Decoded::Meta(meta))
        } else {
            None
        }
    }

    fn visit_leaf<P: AsRef<Path>>(&mut self, _path: P, block: &Full, leaf: Leaf) -> Option<Decoded> {
        if block.id == self.0 {
            Some(Decoded::Leaf(leaf))
        } else {
            None
        }
    }

    fn visit_partial<P: AsRef<Path>>(&mut self, _path: P, _block: &Partial) -> Option<Decoded> {
        unreachable!()
    }

    fn visit_missing<P: AsRef<Path>>(&mut self, _path: P, _blockid: [u8; 32]) -> Option<Decoded> {
        // ignore
        None
    }
}

pub struct VerifyVisitor;

impl Visitor<()> for VerifyVisitor {
    fn visit_dir<P: AsRef<Path>>(&mut self, _path: P, _block: &Full, _dir: &Dir) -> Option<()> {
        None
    }

    fn visit_meta<P: AsRef<Path>>(&mut self, _blockdb: &BlockDb, _path: P, _block: &Full, _meta: Meta) -> Option<()> {
        None
    }

    fn visit_leaf<P: AsRef<Path>>(&mut self, _path: P, _block: &Full, _leaf: Leaf) -> Option<()> {
        None
    }

    fn visit_partial<P: AsRef<Path>>(&mut self, _path: P, _block: &Partial) -> Option<()> {
        Some(())
    }

    fn visit_missing<P: AsRef<Path>>(&mut self, _path: P, _blockid: [u8; 32]) -> Option<()> {
        Some(())
    }
}