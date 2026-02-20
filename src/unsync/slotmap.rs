use std::{
    iter::{Enumerate, FusedIterator},
    mem, ptr, slice, vec,
};

use crate::util::{self, Okok};

/// Single-threaded slot map with stable, reusable IDs.
///
/// Each inserted value receives a stable `usize` ID that remains valid until
/// the value is removed. All operations have the time complexities documented
/// on the methods below.
///
/// # Examples
///
/// ```rust
/// use deadlock::unsync::SlotMap;
///
/// let mut map = SlotMap::new();
/// let a = map.insert(10);
/// let b = map.insert(20);
/// assert_eq!(map.get(a), Some(&10));
/// map.remove(a);
/// assert_eq!(map.get(a), None);
/// assert_eq!(map.get(b), Some(&20));
/// ```
pub struct SlotMap<T> {
    entries: Vec<Result<T, usize>>,
    len: usize,
    next: usize,
}

impl<T> SlotMap<T> {
    /// Creates an empty `SlotMap`. Time: O(1).
    pub const fn new() -> Self {
        Self {
            entries: Vec::new(),
            next: 0,
            len: 0,
        }
    }

    /// Removes all entries and resets the free-list. Time: O(n) where n is the capacity.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.len = 0;
        self.next = 0;
    }

    /// Returns the number of live entries. Time: O(1).
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Returns `true` if there are no live entries. Time: O(1).
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Returns `true` if `id` refers to a live entry. Time: O(1).
    pub fn contains(&self, id: usize) -> bool {
        self.entries.get(id).is_some_and(Result::is_ok)
    }

    /// Returns a shared reference to the value at `id`, or `None` if `id` is
    /// not a live entry. Time: O(1).
    pub fn get(&self, id: usize) -> Option<&T> {
        self.entries.get(id)?.okok()
    }

    /// Returns a shared reference to the value at `id` without bounds or
    /// liveness checking. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live entry.
    pub unsafe fn get_unchecked(&self, id: usize) -> &T {
        unsafe { self.entries.get_unchecked(id).okok_unchecked() }
    }

    /// Returns an exclusive reference to the value at `id`, or `None` if `id`
    /// is not a live entry. Time: O(1).
    pub fn get_mut(&mut self, id: usize) -> Option<&mut T> {
        self.entries.get_mut(id)?.okok()
    }

    /// Returns an exclusive reference to the value at `id` without bounds or
    /// liveness checking. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live entry.
    pub unsafe fn get_unchecked_mut(&mut self, id: usize) -> &mut T {
        unsafe { self.entries.get_unchecked_mut(id).okok_unchecked() }
    }

    /// Inserts `value` and returns its new stable ID. Reuses the next free
    /// slot when available; otherwise appends to the backing buffer.
    /// Time: O(1) amortized.
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

    /// Removes the entry at `id` and returns its value, or `None` if `id` is
    /// not a live entry. Time: O(1).
    pub fn remove(&mut self, id: usize) -> Option<T> {
        util::ensure!(self.contains(id));
        unsafe { self.remove_unchecked(id) }.into()
    }

    /// Removes the entry at `id` and returns its value without liveness
    /// checking. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live entry.
    pub unsafe fn remove_unchecked(&mut self, id: usize) -> T {
        self.len -= 1;
        let value = unsafe {
            let new_entry = Err(self.next);
            let entry = self.entries.get_unchecked_mut(id);
            mem::replace(entry, new_entry).unwrap_unchecked()
        };
        self.next = id;
        value
    }

    /// Swaps the values at `id0` and `id1` in-place. Returns `None` if either
    /// ID is not a live entry. Time: O(1).
    pub fn swap(&mut self, id0: usize, id1: usize) -> Option<()> {
        util::ensure!(self.contains(id0) && self.contains(id1));

        if id0 != id1 {
            unsafe { self.swap_unchecked(id0, id1) }
        }
        .into()
    }

    /// Swaps the values at `id0` and `id1` in-place without liveness checking.
    /// Time: O(1).
    ///
    /// # Safety
    ///
    /// Both `id0` and `id1` must refer to live entries and must be distinct.
    pub unsafe fn swap_unchecked(&mut self, id0: usize, id1: usize) {
        unsafe {
            let ptr0 = self.entries.get_unchecked_mut(id0).okok_unchecked() as *mut _;
            let ptr1 = self.entries.get_unchecked_mut(id1).okok_unchecked() as *mut _;
            ptr::swap_nonoverlapping(ptr0, ptr1, 1)
        }
    }

    /// Returns an iterator over the IDs of all live entries. Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn ids(&self) -> Ids<'_, T> {
        Ids {
            inner: self.entries.iter().enumerate(),
            len: self.len,
        }
    }

    /// Returns an iterator over shared references to the values of all live
    /// entries. Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn values(&self) -> Values<'_, T> {
        Values {
            inner: self.entries.iter(),
            len: self.len,
        }
    }

    /// Returns an iterator over exclusive references to the values of all live
    /// entries. Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn values_mut(&mut self) -> ValuesMut<'_, T> {
        ValuesMut {
            inner: self.entries.iter_mut(),
            len: self.len,
        }
    }

    /// Consumes the map and returns an iterator over the IDs of all live
    /// entries. Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn into_ids(self) -> IntoIds<T> {
        IntoIds {
            inner: self.entries.into_iter().enumerate(),
            len: self.len,
        }
    }

    /// Consumes the map and returns an iterator over the values of all live
    /// entries. Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn into_values(self) -> IntoValues<T> {
        IntoValues {
            inner: self.entries.into_iter(),
            len: self.len,
        }
    }

    /// Removes all entries while yielding `(id, value)` pairs. Remaining
    /// entries are dropped when the iterator is dropped. Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn drain(&mut self) -> Drain<'_, T> {
        let len = self.len;
        self.len = 0;
        self.next = 0;
        Drain {
            inner: self.entries.drain(..).enumerate(),
            len,
        }
    }

    /// Returns an iterator over `(id, &value)` pairs for all live entries.
    /// Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn iter(&self) -> Iter<'_, T> {
        Iter {
            inner: self.entries.iter().enumerate(),
            len: self.len,
        }
    }

    /// Returns an iterator over `(id, &mut value)` pairs for all live entries.
    /// Time: O(1) to construct; each `next` is O(1) amortized over a full scan.
    pub fn iter_mut(&mut self) -> IterMut<'_, T> {
        IterMut {
            inner: self.entries.iter_mut().enumerate(),
            len: self.len,
        }
    }
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Iterator over `(id, value)` pairs of a slot map by shared reference.
pub struct Iter<'a, T> {
    inner: Enumerate<slice::Iter<'a, Result<T, usize>>>,
    len: usize,
}

impl<'a, T> Iterator for Iter<'a, T> {
    type Item = (usize, &'a T);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self
            .inner
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T> DoubleEndedIterator for Iter<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner)
            .rev()
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }
}

impl<'a, T> ExactSizeIterator for Iter<'a, T> {}
impl<'a, T> FusedIterator for Iter<'a, T> {}

/// Iterator over `(id, value)` pairs of a slot map by mutable reference.
pub struct IterMut<'a, T> {
    inner: Enumerate<slice::IterMut<'a, Result<T, usize>>>,
    len: usize,
}

impl<'a, T> Iterator for IterMut<'a, T> {
    type Item = (usize, &'a mut T);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self
            .inner
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T> DoubleEndedIterator for IterMut<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner)
            .rev()
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }
}

impl<'a, T> ExactSizeIterator for IterMut<'a, T> {}
impl<'a, T> FusedIterator for IterMut<'a, T> {}

/// Iterator that yields `(id, value)` pairs by value when consuming the slot map.
pub struct IntoIter<T> {
    inner: Enumerate<vec::IntoIter<Result<T, usize>>>,
    len: usize,
}

impl<T> Iterator for IntoIter<T> {
    type Item = (usize, T);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self
            .inner
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<T> DoubleEndedIterator for IntoIter<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner)
            .rev()
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }
}

impl<T> ExactSizeIterator for IntoIter<T> {}
impl<T> FusedIterator for IntoIter<T> {}

/// Iterator that drains `(id, value)` pairs from the slot map.
pub struct Drain<'a, T> {
    inner: Enumerate<vec::Drain<'a, Result<T, usize>>>,
    len: usize,
}

impl<'a, T> Iterator for Drain<'a, T> {
    type Item = (usize, T);

    fn next(&mut self) -> Option<Self::Item> {
        let item = self
            .inner
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T> DoubleEndedIterator for Drain<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner)
            .rev()
            .find_map(|(id, entry)| entry.okok().map(|value| (id, value)))?;
        self.len -= 1;
        Some(item)
    }
}

impl<'a, T> ExactSizeIterator for Drain<'a, T> {}
impl<'a, T> FusedIterator for Drain<'a, T> {}

/// Iterator over live slot IDs by shared reference.
pub struct Ids<'a, T> {
    inner: Enumerate<slice::Iter<'a, Result<T, usize>>>,
    len: usize,
}

impl<'a, T> Iterator for Ids<'a, T> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self
            .inner
            .find_map(|(id, entry)| entry.is_ok().then_some(id))?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T> DoubleEndedIterator for Ids<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner)
            .rev()
            .find_map(|(id, entry)| entry.is_ok().then_some(id))?;
        self.len -= 1;
        Some(item)
    }
}

impl<'a, T> ExactSizeIterator for Ids<'a, T> {}
impl<'a, T> FusedIterator for Ids<'a, T> {}

/// Iterator that yields live slot IDs by value when consuming the slot map.
pub struct IntoIds<T> {
    inner: Enumerate<vec::IntoIter<Result<T, usize>>>,
    len: usize,
}

impl<T> Iterator for IntoIds<T> {
    type Item = usize;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self
            .inner
            .find_map(|(id, entry)| entry.is_ok().then_some(id))?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<T> DoubleEndedIterator for IntoIds<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner)
            .rev()
            .find_map(|(id, entry)| entry.is_ok().then_some(id))?;
        self.len -= 1;
        Some(item)
    }
}

impl<T> ExactSizeIterator for IntoIds<T> {}
impl<T> FusedIterator for IntoIds<T> {}

/// Iterator over values by shared reference, without exposing IDs.
pub struct Values<'a, T> {
    inner: slice::Iter<'a, Result<T, usize>>,
    len: usize,
}

impl<'a, T> Iterator for Values<'a, T> {
    type Item = &'a T;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.find_map(Okok::okok)?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T> DoubleEndedIterator for Values<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner).rev().find_map(Okok::okok)?;
        self.len -= 1;
        Some(item)
    }
}

impl<'a, T> ExactSizeIterator for Values<'a, T> {}
impl<'a, T> FusedIterator for Values<'a, T> {}

/// Iterator over values by mutable reference, without exposing IDs.
pub struct ValuesMut<'a, T> {
    inner: slice::IterMut<'a, Result<T, usize>>,
    len: usize,
}

impl<'a, T> Iterator for ValuesMut<'a, T> {
    type Item = &'a mut T;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.find_map(Okok::okok)?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<'a, T> DoubleEndedIterator for ValuesMut<'a, T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner).rev().find_map(Okok::okok)?;
        self.len -= 1;
        Some(item)
    }
}

impl<'a, T> ExactSizeIterator for ValuesMut<'a, T> {}
impl<'a, T> FusedIterator for ValuesMut<'a, T> {}

/// Iterator that yields values by value when consuming the slot map, without IDs.
pub struct IntoValues<T> {
    inner: vec::IntoIter<Result<T, usize>>,
    len: usize,
}

impl<T> Iterator for IntoValues<T> {
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        let item = self.inner.find_map(Okok::okok)?;
        self.len -= 1;
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.len, Some(self.len))
    }
}

impl<T> DoubleEndedIterator for IntoValues<T> {
    fn next_back(&mut self) -> Option<Self::Item> {
        let item = (&mut self.inner).rev().find_map(Okok::okok)?;
        self.len -= 1;
        Some(item)
    }
}

impl<T> ExactSizeIterator for IntoValues<T> {}
impl<T> FusedIterator for IntoValues<T> {}
