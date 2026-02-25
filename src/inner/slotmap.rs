use std::{
    alloc::{self, Layout},
    mem::{self, MaybeUninit},
    ptr::NonNull,
};

pub struct SlotMap<T> {
    entries: NonNull<Entry<T>>,
    capacity: usize,
    len: usize,
    next: usize,
}

pub struct Entry<T> {
    pub value: MaybeUninit<T>,
    pub index: usize,
    pub id: usize,
}

impl<T> SlotMap<T> {
    pub fn new() -> Self {
        Self {
            entries: NonNull::dangling(),
            capacity: 0,
            len: 0,
            next: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn insert(&mut self, value: T) -> usize {
        if self.next == self.capacity {
            unsafe { self.grow() }
        }

        let id = self.next;
        let value_entry = unsafe { self.entries.add(id).as_mut() };
        self.next = mem::replace(&mut value_entry.index, self.len);
        value_entry.value.write(value);
        let id_entry = unsafe { self.entries.add(self.len).as_mut() };
        id_entry.id = id;
        self.len += 1;
        id
    }

    pub unsafe fn remove_unchecked(&mut self, id: usize) -> T {
        let value_entry = unsafe { self.entries.add(id).as_mut() };
        let value = unsafe { value_entry.value.assume_init_read() };
        let index = mem::replace(&mut value_entry.index, self.next);
        self.next = id;
        self.len -= 1;

        if index != self.len {
            let id_entry = unsafe { self.entries.add(index).as_mut() };
            let last_id_entry = unsafe { self.entries.add(self.len).as_mut() };
            let moved_id = last_id_entry.id;
            id_entry.id = moved_id;
            let moved_value_entry = unsafe { self.entries.add(moved_id).as_mut() };
            moved_value_entry.index = index
        }

        value
    }

    pub unsafe fn get_unchecked(&self, id: usize) -> &T {
        unsafe { self.entries.add(id).as_ref().value.assume_init_ref() }
    }

    pub unsafe fn get_unchecked_mut(&mut self, id: usize) -> &mut T {
        unsafe { self.entries.add(id).as_mut().value.assume_init_mut() }
    }

    pub unsafe fn get_unchecked_ptr(&self, id: usize) -> NonNull<T> {
        let entry = unsafe { self.entries.add(id).as_mut() };
        unsafe { NonNull::new_unchecked(entry.value.as_mut_ptr()) }
    }

    pub unsafe fn get_unchecked_nth_ptr(&self, index: usize) -> NonNull<T> {
        let id = unsafe { self.entries.add(index).as_ref().id };
        unsafe { self.get_unchecked_ptr(id) }
    }

    unsafe fn grow(&mut self) {
        let old_capacity = self.capacity;
        self.capacity = if self.capacity == 0 {
            1
        } else {
            self.capacity * 2
        };
        self.entries = unsafe {
            let new_layout = Layout::array::<Entry<T>>(self.capacity).unwrap_unchecked();
            let ptr = if old_capacity == 0 {
                alloc::alloc(new_layout)
            } else {
                let old_layout = Layout::array::<Entry<T>>(old_capacity).unwrap_unchecked();
                alloc::realloc(
                    self.entries.as_ptr() as *mut u8,
                    old_layout,
                    new_layout.size(),
                )
            };
            NonNull::new_unchecked(ptr as *mut Entry<T>)
        };

        for i in old_capacity..self.capacity {
            let entry = unsafe { self.entries.add(i).as_mut() };
            entry.index = i + 1
        }
    }
}

impl<T> Drop for SlotMap<T> {
    fn drop(&mut self) {
        if self.capacity == 0 {
            return;
        }

        for index in 0..self.len {
            let id = unsafe { self.entries.add(index).as_ref().id };
            let entry = unsafe { self.entries.add(id).as_mut() };
            unsafe { entry.value.assume_init_drop() }
        }

        unsafe {
            let layout = Layout::array::<Entry<T>>(self.capacity).unwrap_unchecked();
            alloc::dealloc(self.entries.as_ptr() as *mut u8, layout)
        }
    }
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        Self::new()
    }
}
