//! Thread-safe slot min-heap with stable RAII handle.

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::mem::{self, ManuallyDrop};
use triomphe::Arc;

use crate::inner;

/// Thread-safe slot min-heap with stable RAII handle.
///
/// Elements are ordered by `T`'s [`PartialOrd`]. The minimum is at the top and can be read or
/// mutated via [`peek`](SlotHeap::peek) / [`peek_mut`](SlotHeap::peek_mut).
pub struct SlotHeap<T> {
    inner: Arc<RwLock<inner::SlotHeap<T>>>,
}

/// Stable handle to an element in a [`SlotHeap`].
///
/// Dropping it removes the element from the heap (deferred under contention).
pub struct SlotHeapId<T>
where
    T: PartialOrd,
{
    from: ManuallyDrop<Arc<RwLock<inner::SlotHeap<T>>>>,
    id: usize,
}

impl<T> SlotHeap<T>
where
    T: PartialOrd,
{
    /// Creates a new empty min-heap.
    ///
    /// Time complexity: O(1).
    pub fn new() -> Self {
        Self {
            inner: Arc::new(inner::SlotHeap::new().into()),
        }
    }

    /// Returns the number of elements in the heap.
    ///
    /// Time complexity: O(1).
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns whether the heap is empty.
    ///
    /// Time complexity: O(1).
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Inserts a value and returns its handle and whether it became the new minimum.
    ///
    /// Time complexity: O(log n).
    pub fn insert(&self, value: T) -> (SlotHeapId<T>, bool) {
        let from = ManuallyDrop::new(self.inner.clone());
        let mut guard = self.inner.write();
        let (id, is_top) = guard.insert(value);
        (SlotHeapId { from, id }, is_top)
    }

    /// Returns a shared reference to the minimum element, or `None` if the heap is empty.
    ///
    /// Time complexity: O(k) where k is the number of deferred removals, then O(1).
    pub fn peek(&self) -> Option<SlotHeapPeek<'_, T>> {
        let guard = self.inner.read();
        (!guard.is_empty()).then(|| SlotHeapPeek { guard })
    }

    /// Returns a mutable reference to the minimum element, or `None` if the heap is empty.
    ///
    /// If the minimum is mutated, the heap is re-heapified on drop of the returned guard.
    ///
    /// Time complexity: O(k) where k is the number of deferred removals, then O(1).
    pub fn peek_mut(&self) -> Option<SlotHeapPeekMut<'_, T>> {
        let guard = self.inner.write();
        (!guard.is_empty()).then(|| SlotHeapPeekMut {
            guard,
            dirty: false,
        })
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

impl<T> SlotHeapId<T>
where
    T: PartialOrd,
{
    /// Removes the element from the heap and returns it and whether it was the minimum.
    ///
    /// Time complexity: O(log n).
    pub fn into_inner(mut self) -> (T, bool) {
        let mut guard = self.from.write();
        let item = unsafe { guard.remove_unchecked(self.id) };
        drop(guard);
        unsafe { ManuallyDrop::drop(&mut self.from) };
        mem::forget(self);
        item
    }

    /// Returns an immutable reference to the element, holding a read lock until the ref is dropped.
    ///
    /// Time complexity: O(1).
    pub fn get(&self) -> SlotHeapRef<'_, T> {
        SlotHeapRef {
            guard: self.from.read(),
            id: self.id,
        }
    }

    /// Returns a mutable reference to the element; heap is re-heapified on drop if mutated.
    ///
    /// Time complexity: O(1) for access; O(log n) on drop if the value was mutated.
    pub fn get_mut(&self) -> SlotHeapRefMut<'_, T> {
        SlotHeapRefMut {
            guard: self.from.write(),
            id: self.id,
            dirty: false,
        }
    }
}

impl<T> Drop for SlotHeapId<T>
where
    T: PartialOrd,
{
    fn drop(&mut self) {
        let mut guard = self.from.write();
        unsafe { guard.remove_unchecked(self.id) };
        drop(guard);
        unsafe { ManuallyDrop::drop(&mut self.from) }
    }
}

/// Immutable reference to the minimum element of a [`SlotHeap`], holding a read lock.
pub struct SlotHeapPeek<'a, T>
where
    T: PartialOrd,
{
    guard: RwLockReadGuard<'a, inner::SlotHeap<T>>,
}

#[reflica::reflica]
impl<T> SlotHeapPeek<'_, T>
where
    T: PartialOrd,
{
    fn deref(&self) -> &T {
        unsafe { self.guard.peek_unchecked() }
    }
}

/// Mutable reference to the minimum element of a [`SlotHeap`]; re-heapifies on drop if dirty.
pub struct SlotHeapPeekMut<'a, T>
where
    T: PartialOrd,
{
    guard: RwLockWriteGuard<'a, inner::SlotHeap<T>>,
    dirty: bool,
}

#[reflica::reflica]
impl<T> SlotHeapPeekMut<'_, T>
where
    T: PartialOrd,
{
    /// Explicitly finishes mutation and re-heapifies if needed, consuming the guard.
    ///
    /// This method allows early release of the write lock while determining whether the
    /// element remains at the top after re-heapification. Unlike relying on `Drop`,
    /// this provides immediate feedback about the element's final position.
    ///
    /// # Returns
    ///
    /// `true` if the element is still the minimum after re-heapification (or if it wasn't mutated),
    /// `false` if it moved to a different position.
    ///
    /// Time complexity: O(log n) if the element was mutated, O(1) otherwise.
    pub fn finish(mut self) -> bool {
        let is_top = if self.dirty {
            unsafe { self.guard.heapify_down(0) == 0 }
        } else {
            true
        };

        mem::forget(self);
        is_top
    }

    fn deref(&self) -> &T {
        unsafe { self.guard.peek_unchecked() }
    }

    fn deref_mut(&mut self) -> &mut T {
        self.dirty = true;
        unsafe { self.guard.peek_unchecked_mut() }
    }
}

impl<T> Drop for SlotHeapPeekMut<'_, T>
where
    T: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            unsafe { self.guard.heapify_down(0) };
        }
    }
}

/// Immutable reference to an element in a [`SlotHeap`], holding a read lock.
pub struct SlotHeapRef<'a, T>
where
    T: PartialOrd,
{
    guard: RwLockReadGuard<'a, inner::SlotHeap<T>>,
    id: usize,
}

#[reflica::reflica]
impl<T> SlotHeapRef<'_, T>
where
    T: PartialOrd,
{
    /// Returns whether this element is the current minimum (top) of the heap.
    ///
    /// Time complexity: O(1).
    pub fn is_top(&self) -> bool {
        unsafe { self.guard.get_unchecked_index(self.id) == 0 }
    }

    fn deref(&self) -> &T {
        unsafe { self.guard.get_unchecked(self.id) }
    }
}

/// Mutable reference to an element in a [`SlotHeap`]; re-heapifies on drop if mutated.
pub struct SlotHeapRefMut<'a, T>
where
    T: PartialOrd,
{
    guard: RwLockWriteGuard<'a, inner::SlotHeap<T>>,
    id: usize,
    dirty: bool,
}

#[reflica::reflica]
impl<T> SlotHeapRefMut<'_, T>
where
    T: PartialOrd,
{
    /// Returns whether this element is the current minimum (top) of the heap.
    ///
    /// Time complexity: O(1).
    pub fn is_top(&self) -> bool {
        unsafe { self.guard.get_unchecked_index(self.id) == 0 }
    }

    /// Explicitly finishes mutation and re-heapifies if needed, consuming the guard.
    ///
    /// This method allows early release of the write lock while determining whether the
    /// element moved to the top after re-heapification. Unlike relying on `Drop`,
    /// this provides immediate feedback about the element's final position.
    ///
    /// # Returns
    ///
    /// `true` if the element became (or remained) the minimum after re-heapification,
    /// or if it wasn't mutated. `false` if it ended up at a different position.
    ///
    /// Time complexity: O(log n) if the element was mutated, O(1) otherwise.
    pub fn finish(mut self) -> bool {
        let is_top = if self.dirty {
            let index = unsafe { self.guard.get_unchecked_index(self.id) };
            unsafe { self.guard.heapify(index) == 0 }
        } else {
            true
        };

        mem::forget(self);
        is_top
    }

    fn deref(&self) -> &T {
        unsafe { self.guard.get_unchecked(self.id) }
    }

    fn deref_mut(&mut self) -> &mut T {
        self.dirty = true;
        unsafe { self.guard.get_unchecked_mut(self.id) }
    }
}

impl<T> Drop for SlotHeapRefMut<'_, T>
where
    T: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            let index = unsafe { self.guard.get_unchecked_index(self.id) };
            unsafe {
                self.guard.heapify(index);
            }
        }
    }
}

unsafe impl<T> Send for SlotHeap<T> where T: Send + PartialOrd {}
unsafe impl<T> Sync for SlotHeap<T> where T: Send + Sync + PartialOrd {}

unsafe impl<T> Send for SlotHeapId<T> where T: Send + PartialOrd {}
unsafe impl<T> Sync for SlotHeapId<T> where T: Send + Sync + PartialOrd {}

unsafe impl<T> Send for SlotHeapPeek<'_, T> where T: Send + Sync + PartialOrd {}
unsafe impl<T> Sync for SlotHeapPeek<'_, T> where T: Send + Sync + PartialOrd {}

unsafe impl<T> Send for SlotHeapPeekMut<'_, T> where T: Send + PartialOrd {}
unsafe impl<T> Sync for SlotHeapPeekMut<'_, T> where T: Send + Sync + PartialOrd {}

unsafe impl<T> Send for SlotHeapRef<'_, T> where T: Send + Sync + PartialOrd {}
unsafe impl<T> Sync for SlotHeapRef<'_, T> where T: Send + Sync + PartialOrd {}

unsafe impl<T> Send for SlotHeapRefMut<'_, T> where T: Send + PartialOrd {}
unsafe impl<T> Sync for SlotHeapRefMut<'_, T> where T: Send + Sync + PartialOrd {}
