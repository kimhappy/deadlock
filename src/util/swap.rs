use std::{mem, ptr};

#[easy_ext::ext(SliceExt)]
pub impl<T> [T] {
    unsafe fn swap_unchecked_(&mut self, index0: usize, index1: usize) {
        let base = self.as_mut_ptr();
        let elem0 = unsafe { &mut *base.add(index0) };
        let elem1 = unsafe { &mut *base.add(index1) };
        mem::swap(elem0, elem1)
    }
}

#[easy_ext::ext(VecExt)]
pub impl<T> Vec<T> {
    unsafe fn swap_remove_unchecked_(&mut self, index: usize) -> T {
        let len = self.len();

        unsafe {
            let base = self.as_mut_ptr();
            let value0 = ptr::read(base.add(index));
            ptr::copy_nonoverlapping(base.add(len - 1), base.add(index), 1);
            self.set_len(len - 1);
            value0
        }
    }
}
