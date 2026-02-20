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

#[crabtime::function]
fn def_slotmap_iterator(types: Vec<&str>) {
    for ty in types {
        let lt = ty.starts_with("Into").then_some("").unwrap_or("'a, ");
        let ty_full = format!("{ty}<{lt}T>");
        let inner = {
            let inner = if ty == "Drain" {
                "vec::Drain"
            } else if ty.starts_with("Into") {
                "vec::IntoIter"
            } else if ty.ends_with("Mut") {
                "slice::IterMut"
            } else {
                "slice::Iter"
            };
            let inner = format!("{inner}<{lt}Result<T, usize>>");

            if ty.contains("Values") {
                inner
            } else {
                format!("Enumerate<{inner}>")
            }
        };
        let elem_ty = {
            let elem_ty = if ty.ends_with("Ids") {
                "usize"
            } else if ty.ends_with("Mut") {
                "&'a mut T"
            } else if ty.starts_with("Into") || ty == "Drain" {
                "T"
            } else {
                "&'a T"
            };
            if ty.contains("Iter") || ty == "Drain" {
                format!("(usize, {elem_ty})")
            } else {
                elem_ty.into()
            }
        };
        let map_expr = if ty.ends_with("Ids") {
            "|(id, entry)| entry.is_ok().then_some(id)"
        } else if ty.contains("Values") {
            "Okok::okok"
        } else {
            "|(id, entry)| entry.okok().map(|value| (id, value))"
        };

        crabtime::output! {
            pub struct {{ty_full}} {
                inner: {{inner}},
                len: usize,
            }

            impl<{{lt}}T> Iterator for {{ty_full}} {
                type Item = {{elem_ty}};

                fn next(&mut self) -> Option<Self::Item> {
                    let item = self.inner.find_map({{map_expr}})?;
                    self.len -= 1;
                    Some(item)
                }

                fn size_hint(&self) -> (usize, Option<usize>) {
                    (self.len, Some(self.len))
                }
            }

            impl<{{lt}}T> DoubleEndedIterator for {{ty_full}} {
                fn next_back(&mut self) -> Option<Self::Item> {
                    let item = (&mut self.inner).rev().find_map({{map_expr}})?;
                    self.len -= 1;
                    Some(item)
                }
            }

            impl<{{lt}}T> ExactSizeIterator for {{ty_full}} {}
            impl<{{lt}}T> FusedIterator for {{ty_full}} {}
        }
    }
}

def_slotmap_iterator!([
    "Iter",
    "IterMut",
    "IntoIter",
    "Drain",
    "Ids",
    "IntoIds",
    "Values",
    "ValuesMut",
    "IntoValues"
]);
