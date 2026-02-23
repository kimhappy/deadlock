use std::mem;

use crate::util::Okok;

pub struct SlotMap<T> {
    entries: Vec<Result<T, usize>>,
    len: usize,
    next: usize,
}

impl<T> SlotMap<T> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            len: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn contains(&self, id: usize) -> bool {
        self.entries.get(id).is_some_and(Result::is_ok)
    }

    pub fn insert(&mut self, value: T) -> usize {
        self.len += 1;
        let id = self.next;
        self.next = if self.entries.len() == id {
            self.entries.push(Ok(value));
            id + 1
        } else {
            unsafe {
                let entry = self.entries.get_unchecked_mut(id);
                mem::replace(entry, Ok(value)).unwrap_err_unchecked()
            }
        };

        id
    }

    pub unsafe fn remove_unchecked(&mut self, id: usize) -> T {
        let entry = unsafe { self.entries.get_unchecked_mut(id) };
        let new_entry = Err(self.next);
        self.len -= 1;
        let value = unsafe { mem::replace(entry, new_entry).unwrap_unchecked() };
        self.next = id;
        value
    }

    pub unsafe fn get_unchecked(&self, id: usize) -> &T {
        unsafe { self.entries.get_unchecked(id).okok_unchecked() }
    }

    pub unsafe fn get_unchecked_mut(&mut self, id: usize) -> &mut T {
        unsafe { self.entries.get_unchecked_mut(id).okok_unchecked() }
    }
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        Self::new()
    }
}
