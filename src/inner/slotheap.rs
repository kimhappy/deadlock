use std::cmp::Ordering;

use crate::{
    inner::SlotMap,
    util::{SliceExt, VecExt},
};

pub struct SlotHeap<T> {
    ids: Vec<usize>,
    entries: SlotMap<(T, usize)>,
}

impl<T> SlotHeap<T>
where
    T: PartialOrd,
{
    pub fn new() -> Self {
        Self {
            ids: Vec::new(),
            entries: SlotMap::new(),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.len() == 0
    }

    pub fn insert(&mut self, value: T) -> (usize, bool) {
        let id = self.entries.insert((value, self.ids.len()));
        self.ids.push(id);
        let index = unsafe { self.heapify_up(self.ids.len() - 1) };
        (id, index == 0)
    }

    pub unsafe fn remove_unchecked(&mut self, id: usize) -> (T, bool) {
        unsafe {
            let (value, index) = self.entries.remove_unchecked(id);

            if index == self.ids.len() - 1 {
                self.ids.set_len(self.ids.len() - 1)
            } else {
                self.ids.swap_remove_unchecked_(index);
                let tail = self.ids.get_unchecked(index);
                self.entries.get_unchecked_mut(*tail).1 = index;
                self.heapify(index)
            }

            (value, index == 0)
        }
    }

    pub unsafe fn peek_unchecked(&self) -> &T {
        unsafe {
            let id = self.ids.get_unchecked(0);
            &self.entries.get_unchecked(*id).0
        }
    }

    pub unsafe fn peek_unchecked_mut(&mut self) -> &mut T {
        unsafe {
            let id = self.ids.get_unchecked(0);
            &mut self.entries.get_unchecked_mut(*id).0
        }
    }

    pub unsafe fn get_unchecked(&self, id: usize) -> &T {
        unsafe { &self.entries.get_unchecked(id).0 }
    }

    pub unsafe fn get_unchecked_mut(&mut self, id: usize) -> &mut T {
        unsafe { &mut self.entries.get_unchecked_mut(id).0 }
    }

    pub unsafe fn get_unchecked_index(&self, id: usize) -> usize {
        unsafe { self.entries.get_unchecked(id).1 }
    }

    pub unsafe fn heapify(&mut self, mut index: usize) {
        unsafe {
            index = self.heapify_up(index);
            self.heapify_down(index);
        }
    }

    unsafe fn heapify_up(&mut self, mut index: usize) -> usize {
        unsafe {
            while let Some(up_index) = self.next_up(index) {
                self.swap_entries(index, up_index);
                index = up_index
            }
        }

        index
    }

    pub unsafe fn heapify_down(&mut self, mut index: usize) {
        unsafe {
            while let Some(down_index) = self.next_down(index) {
                self.swap_entries(index, down_index);
                index = down_index
            }
        }
    }

    unsafe fn next_up(&self, index: usize) -> Option<usize> {
        index
            .checked_sub(1)
            .map(|x| x / 2)
            .filter(|up_index| unsafe {
                let id = self.ids.get_unchecked(index);
                let up_id = self.ids.get_unchecked(*up_index);
                self.less(*id, *up_id)
            })
    }

    unsafe fn next_down(&self, index: usize) -> Option<usize> {
        let id = unsafe { self.ids.get_unchecked(index) };
        let (left_index, right_index) = (index * 2 + 1, index * 2 + 2);

        if let Some(right_id) = self.ids.get(right_index) {
            let left_id = unsafe { self.ids.get_unchecked(left_index) };

            if unsafe { self.less(*left_id, *right_id) } {
                unsafe { self.less(*left_id, *id) }.then_some(left_index)
            } else {
                unsafe { self.less(*right_id, *id) }.then_some(right_index)
            }
        } else {
            let left_id = self.ids.get(left_index)?;
            unsafe { self.less(*left_id, *id) }.then_some(left_index)
        }
    }

    unsafe fn swap_entries(&mut self, index0: usize, index1: usize) {
        unsafe {
            let id0 = self.ids.get_unchecked(index0);
            let id1 = self.ids.get_unchecked(index1);
            self.entries.get_unchecked_mut(*id0).1 = index1;
            self.entries.get_unchecked_mut(*id1).1 = index0;
            self.ids.swap_unchecked_(index0, index1)
        }
    }

    unsafe fn less(&self, id0: usize, id1: usize) -> bool {
        let value0 = unsafe { &self.entries.get_unchecked(id0).0 };
        let value1 = unsafe { &self.entries.get_unchecked(id1).0 };

        match value0.partial_cmp(value1) {
            Some(Ordering::Less) => true,
            Some(Ordering::Greater) => false,
            _ => id0 < id1,
        }
    }
}

impl<T> Default for SlotHeap<T>
where
    T: PartialOrd,
{
    fn default() -> Self {
        Self::new()
    }
}
