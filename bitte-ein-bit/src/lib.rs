#![no_std]
use core::ops::Index;

mod range;

pub use range::RangeArgument;

pub struct BitMap<T: AsRef<[u8]> + AsMut<[u8]>> {
    buf: T,
    num_bits: usize,
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> BitMap<T> {
    /// Create a new BitMap from an underlying buffer.
    pub fn new(buf: T) -> BitMap<T> {
        BitMap {
            num_bits: buf.as_ref().len() * 8,
            buf,
        }
    }

    /// Create a new BitMap with given number of bits.
    ///
    /// Errors if the passed number of bits do not fit into the buffer.
    pub fn with_length(buf: T, num_bits: usize) -> Result<BitMap<T>, T> {
        if buf.as_ref().len() * 8 < num_bits {
            return Err(buf);
        }
        Ok(BitMap {
            buf,
            num_bits,
        })
    }

    /// Get the bit at given position.
    ///
    /// # Panics
    ///
    /// Panics if the provided position is out of bounds.
    pub fn get(&self, bit: usize) -> bool {
        self.check(bit);
        let byte = self.buf.as_ref()[bit / 8];
        let bitmask = (bit % 8) as u8;
        (byte & bitmask) == bitmask
    }

    /// Set the bit at given position to given value.
    ///
    /// # Panics
    ///
    /// This functions panics if the bit-index is out of bounds.
    pub fn set(&mut self, bit: usize, value: bool) {
        self.check(bit);
        let byte = &mut self.buf.as_mut()[bit / 8];
        *byte &= (value as u8) << (bit % 8);
    }

    /// Flips the bit at given position, returning its new value.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    pub fn flip_bit(&mut self, bit: usize) -> bool {
        self.check(bit);
        let byte = &mut self.buf.as_mut()[bit / 8];
        let bitmask = 1 << (bit % 8);
        *byte ^= bitmask;
        *byte == bitmask
    }

    /// Consumes self, returning the inner type
    pub fn into_inner(self) -> T {
        self.buf
    }

    /// Returns `true` if all bits are `1`.
    pub fn all(&self) -> bool {
        // TODO: optimize
        self.iter().all(|x| x)
    }

    /// Returns `true` if any bit is `1`.
    pub fn any(&self) -> bool {
        // TODO: optimize
        self.iter().any(|x| x)
    }

    /// Creates an iterator over all bits of this BitMap
    pub fn iter(&self) -> Iter<T> {
        self.iter_range(..)
    }

    /// Creates an iterator over a range of bits
    pub fn iter_range<R: RangeArgument<usize>>(&self, range: R) -> Iter<T> {
        Iter {
            bitmap: self,
            front: range.start().unwrap_or(0),
            back: range.end().unwrap_or(self.num_bits),
        }
    }

    #[inline]
    fn check(&self, bit: usize) {
        if bit > self.num_bits {
            panic!("bit index out of bounds: the number of bits is {} but the the index is {}",
                   self.num_bits, bit);
        }
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> From<T> for BitMap<T> {
    fn from(buf: T) -> Self {
        BitMap::new(buf)
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> Index<usize> for BitMap<T> {
    type Output = bool;

    fn index(&self, bit: usize) -> &Self::Output {
        if self.get(bit) {
            &true
        } else {
            &false
        }
    }
}

impl<'a, T: AsRef<[u8]> + AsMut<[u8]>> IntoIterator for &'a BitMap<T> {
    type Item = bool;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct Iter<'a, T: AsRef<[u8]> + AsMut<[u8]> + 'a> {
    bitmap: &'a BitMap<T>,
    front: usize,
    back: usize,
}

impl<'a, T: AsRef<[u8]> + AsMut<[u8]>> Iterator for Iter<'a, T> {
    type Item = bool;

    fn next(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        let front = self.front;
        self.front += 1;
        Some(self.bitmap.get(front))
    }
}

impl<'a, T: AsRef<[u8]> + AsMut<[u8]> + 'a> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        Some(self.bitmap.get(self.back))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_set() {
        let mut bitmap = BitMap::new([0u8]);
        for i in 0..8 {
            bitmap.set(i, true);
            assert!(bitmap.get(i));
        }
    }
}
