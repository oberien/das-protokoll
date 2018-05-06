#![no_std]
use core::ops::Index;
use core::fmt;

mod range;

pub use range::RangeArgument;

pub struct BitMap<T> {
    buf: T,
    num_bits: u64,
}

impl<T: AsRef<[u8]>> BitMap<T> {
    /// Create a new BitMap from an underlying buffer.
    pub fn new(buf: T) -> BitMap<T> {
        let num_bits = buf.as_ref().len() as u64 * 8;
        BitMap::with_length(buf, num_bits)
    }

    /// Create a new BitMap with given number of bits.
    ///
    /// # Panics
    ///
    /// Panics if the passed number of bits do not fit into the buffer.
    pub fn with_length(buf: T, num_bits: u64) -> BitMap<T> {
        if buf.as_ref().len() as u64 * 8 < num_bits {
            panic!("Buf too small for {} bits", num_bits);
        }
        BitMap {
            buf,
            num_bits,
        }
    }

    /// Get the bit at given position.
    ///
    /// # Panics
    ///
    /// Panics if the provided position is out of bounds.
    pub fn get(&self, bit: u64) -> bool {
        self.check(bit);
        let byte = self.buf.as_ref()[(bit / 8) as usize];
        let bitmask = 1 << (bit % 8);
        (byte & bitmask) == bitmask
    }
}

impl<T: AsMut<[u8]>> BitMap<T> {
    /// Set the bit at given position to given value.
    ///
    /// # Panics
    ///
    /// This functions panics if the bit-index is out of bounds.
    pub fn set(&mut self, bit: u64, value: bool) {
        self.check(bit);
        let byte = &mut self.buf.as_mut()[(bit / 8) as usize];
        let bitmask = 1 << (bit % 8);
        *byte &= !bitmask;
        *byte |= (value as u8) << (bit % 8);
    }

    /// Flip the bit at given position, returning its new value.
    ///
    /// # Panics
    ///
    /// Panics if the index is out of bounds.
    pub fn flip_bit(&mut self, bit: u64) -> bool {
        self.check(bit);
        let byte = &mut self.buf.as_mut()[(bit / 8) as usize];
        let bitmask = 1 << (bit % 8);
        *byte ^= bitmask;
        *byte == bitmask
    }

    /// Reset all bits to `0`
    pub fn reset(&mut self) {
        for byte in self.buf.as_mut() {
            *byte = 0;
        }
    }
}

impl<T> BitMap<T> {
    /// Returns the number of bits
    pub fn num_bits(&self) -> u64 {
        self.num_bits
    }

    /// Returns a reference to the inner type
    pub fn get_ref(&self) -> &T {
        &self.buf
    }

    /// Returns a mutable reference to the inner type
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.buf
    }

    /// Consumes self, returning the inner type
    pub fn into_inner(self) -> T {
        self.buf
    }
}

impl<T: AsRef<[u8]>> BitMap<T> {
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
    pub fn iter_range<R: RangeArgument<u64>>(&self, range: R) -> Iter<T> {
        Iter {
            bitmap: self,
            front: range.start().unwrap_or(0),
            back: range.end().unwrap_or(self.num_bits),
        }
    }
}

impl<T> BitMap<T> {
    #[inline]
    fn check(&self, bit: u64) {
        if bit > self.num_bits {
            panic!("bit index out of bounds: the number of bits is {} but the the index is {}",
                   self.num_bits, bit);
        }
    }
}

impl<T: AsRef<[u8]>> From<T> for BitMap<T> {
    fn from(buf: T) -> Self {
        BitMap::new(buf)
    }
}

impl<T: AsRef<[u8]>> Index<u64> for BitMap<T> {
    type Output = bool;

    fn index(&self, bit: u64) -> &Self::Output {
        if self.get(bit) {
            &true
        } else {
            &false
        }
    }
}

impl<'a, T: AsRef<[u8]>> IntoIterator for &'a BitMap<T> {
    type Item = bool;
    type IntoIter = Iter<'a, T>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

pub struct Iter<'a, T: AsRef<[u8]> + 'a> {
    bitmap: &'a BitMap<T>,
    front: u64,
    back: u64,
}

impl<'a, T: AsRef<[u8]>> Iterator for Iter<'a, T> {
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

impl<'a, T: AsRef<[u8]> + 'a> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        if self.front == self.back {
            return None;
        }
        self.back -= 1;
        Some(self.bitmap.get(self.back))
    }
}

impl<T: AsRef<[u8]> + AsMut<[u8]>> fmt::Debug for BitMap<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BitMap")
            .field("buf", &self.buf.as_ref())
            .field("num_bits", &self.num_bits)
            .finish()
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
