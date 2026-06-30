use std::marker::PhantomData;
use std::mem::ManuallyDrop;
use std::ptr::NonNull;

pub(super) struct ReusableIntoIter<T> {
    ptr: NonNull<T>,
    cap: usize,
    index: usize,
    len: usize,
    _marker: PhantomData<T>,
}

impl<T> ReusableIntoIter<T> {
    pub(super) fn new() -> Self {
        Self::from_vec(Vec::new())
    }

    pub(super) fn from_vec(vec: Vec<T>) -> Self {
        let mut vec = ManuallyDrop::new(vec);
        let ptr = NonNull::new(vec.as_mut_ptr()).unwrap_or_else(NonNull::dangling);
        let cap = vec.capacity();
        let len = vec.len();

        Self {
            ptr,
            cap,
            index: 0,
            len,
            _marker: PhantomData,
        }
    }

    pub(super) fn next(&mut self) -> Option<T> {
        if self.index == self.len {
            return None;
        }

        let item = unsafe { self.ptr.as_ptr().add(self.index).read() };
        self.index += 1;
        Some(item)
    }

    pub(super) fn into_vec(self) -> Vec<T> {
        assert_eq!(self.index, self.len);

        let this = ManuallyDrop::new(self);
        unsafe { Vec::from_raw_parts(this.ptr.as_ptr(), 0, this.cap) }
    }
}

impl<T> Default for ReusableIntoIter<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Drop for ReusableIntoIter<T> {
    fn drop(&mut self) {
        unsafe {
            for i in self.index..self.len {
                self.ptr.as_ptr().add(i).drop_in_place();
            }
            drop(Vec::from_raw_parts(self.ptr.as_ptr(), 0, self.cap));
        }
    }
}
