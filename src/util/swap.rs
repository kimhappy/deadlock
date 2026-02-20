use std::ptr;

pub unsafe fn swap_unchecked<S, T>(slice: &mut S, index0: usize, index1: usize)
where
    S: AsMut<[T]>,
{
    let slice = slice.as_mut();

    unsafe {
        let value0 = slice.as_mut_ptr().add(index0);
        let value1 = slice.as_mut_ptr().add(index1);
        ptr::swap_nonoverlapping(value0, value1, 1)
    }
}

pub unsafe fn swap_remove_unchecked<T>(vec: &mut Vec<T>, index: usize) -> T {
    let len = vec.len();

    unsafe {
        let value0 = ptr::read(vec.as_ptr().add(index));
        ptr::copy_nonoverlapping(vec.as_ptr().add(len - 1), vec.as_mut_ptr().add(index), 1);
        vec.set_len(len - 1);
        value0
    }
}
