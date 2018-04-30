use core::ops::{Range, RangeFrom, RangeTo, RangeFull};

// FIXME: Replace with RangeArument in libcore once it's stable
pub trait RangeArgument<T: Copy> {
    fn start(&self) -> Option<T>;
    fn end(&self) -> Option<T>;
}

impl<T: Copy> RangeArgument<T> for Range<T> {
    fn start(&self) -> Option<T> {
        Some(self.start)
    }

    fn end(&self) -> Option<T> {
        Some(self.end)
    }
}

impl<T: Copy> RangeArgument<T> for RangeFrom<T> {
    fn start(&self) -> Option<T> {
        Some(self.start)
    }

    fn end(&self) -> Option<T> {
        None
    }
}

impl<T: Copy> RangeArgument<T> for RangeTo<T> {
    fn start(&self) -> Option<T> {
        None
    }

    fn end(&self) -> Option<T> {
        Some(self.end)
    }
}

impl<T: Copy> RangeArgument<T> for RangeFull {
    fn start(&self) -> Option<T> {
        None
    }

    fn end(&self) -> Option<T> {
        None
    }
}