//! Thread-safe slot min-heap with stable RAII handle.

use crossbeam_channel::{self, Receiver, Sender};
use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    convert::{TryFrom, TryInto},
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};
use triomphe::Arc;

use crate::inner;

/// Thread-safe slot min-heap with stable RAII handle.
///
/// Elements are ordered by `T`'s [`PartialOrd`]. The minimum is at the top and can be read or
/// mutated via [`peek`](SlotHeap::peek) / [`peek_mut`](SlotHeap::peek_mut).
pub struct SlotHeap<T> {
    inner: Arc<Inner<T>>,
}

/// Stable handle to an element in a [`SlotHeap`].
///
/// Dropping it removes the element from the heap (deferred under contention).
pub struct SlotHeapId<T>
where
    T: PartialOrd,
{
    from: ManuallyDrop<Arc<Inner<T>>>,
    id: usize,
}

struct Inner<T> {
    inner: RwLock<inner::SlotHeap<T>>,
    delete: (Sender<usize>, Receiver<usize>),
    len: AtomicUsize,
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
            inner: Arc::new(Inner {
                inner: RwLock::new(inner::SlotHeap::new()),
                delete: crossbeam_channel::unbounded(),
                len: 0.into(),
            }),
        }
    }

    /// Returns the number of elements in the heap.
    ///
    /// Time complexity: O(1).
    pub fn len(&self) -> usize {
        self.inner.len.load(Ordering::Relaxed)
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
        let mut guard = self.inner.inner.write();
        let (id, is_top) = guard.insert(value);
        self.inner.len.fetch_add(1, Ordering::Relaxed);
        (SlotHeapId { from, id }, is_top)
    }

    /// Returns a shared reference to the minimum element, or `None` if the heap is empty.
    ///
    /// Time complexity: O(k) where k is the number of deferred removals, then O(1).
    pub fn peek(&self) -> Option<SlotHeapPeek<'_, T>> {
        RwLockWriteGuard::downgrade(self.flush()).try_into().ok()
    }

    /// Returns a mutable reference to the minimum element, or `None` if the heap is empty.
    ///
    /// If the minimum is mutated, the heap is re-heapified on drop of the returned guard.
    ///
    /// Time complexity: O(k) where k is the number of deferred removals, then O(1).
    pub fn peek_mut(&self) -> Option<SlotHeapPeekMut<'_, T>> {
        self.flush().try_into().ok()
    }

    fn flush(&self) -> RwLockWriteGuard<'_, inner::SlotHeap<T>> {
        let mut guard = self.inner.inner.write();

        for id in self.inner.delete.1.try_iter() {
            unsafe { guard.remove_unchecked(id) };
        }

        guard
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
        let mut guard = self.from.inner.write();
        let item = unsafe { guard.remove_unchecked(self.id) };
        self.from.len.fetch_sub(1, Ordering::Relaxed);
        drop(guard);
        unsafe { ManuallyDrop::drop(&mut self.from) };
        mem::forget(self);
        item
    }

    /// Returns an immutable reference to the element, holding a read lock until the ref is dropped.
    ///
    /// Time complexity: O(1).
    pub fn get(&self) -> SlotHeapRef<'_, T> {
        (self.from.inner.read(), self.id).into()
    }

    /// Returns a mutable reference to the element; heap is re-heapified on drop if mutated.
    ///
    /// Time complexity: O(1) for access; O(log n) on drop if the value was mutated.
    pub fn get_mut(&self) -> SlotHeapRefMut<'_, T> {
        (self.from.inner.write(), self.id).into()
    }
}

impl<T> Drop for SlotHeapId<T>
where
    T: PartialOrd,
{
    fn drop(&mut self) {
        if let Some(mut guard) = self.from.inner.try_write() {
            unsafe { guard.remove_unchecked(self.id) };
        } else {
            unsafe { self.from.delete.0.send(self.id).unwrap_unchecked() };
        }

        self.from.len.fetch_sub(1, Ordering::Relaxed);
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

impl<'a, T> TryFrom<RwLockReadGuard<'a, inner::SlotHeap<T>>> for SlotHeapPeek<'a, T>
where
    T: PartialOrd,
{
    type Error = ();

    fn try_from(guard: RwLockReadGuard<'a, inner::SlotHeap<T>>) -> Result<Self, Self::Error> {
        (!guard.is_empty()).then(|| Self { guard }).ok_or(())
    }
}

impl<T> Deref for SlotHeapPeek<'_, T>
where
    T: PartialOrd,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.guard.peek_unchecked() }
    }
}

impl<T> AsRef<T> for SlotHeapPeek<'_, T>
where
    T: PartialOrd,
{
    fn as_ref(&self) -> &T {
        self.deref()
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

impl<'a, T> TryFrom<RwLockWriteGuard<'a, inner::SlotHeap<T>>> for SlotHeapPeekMut<'a, T>
where
    T: PartialOrd,
{
    type Error = ();

    fn try_from(guard: RwLockWriteGuard<'a, inner::SlotHeap<T>>) -> Result<Self, Self::Error> {
        (!guard.is_empty())
            .then(|| Self {
                guard,
                dirty: false,
            })
            .ok_or(())
    }
}

impl<T> Deref for SlotHeapPeekMut<'_, T>
where
    T: PartialOrd,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.guard.peek_unchecked() }
    }
}

impl<T> AsRef<T> for SlotHeapPeekMut<'_, T>
where
    T: PartialOrd,
{
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<T> DerefMut for SlotHeapPeekMut<'_, T>
where
    T: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { self.guard.peek_unchecked_mut() }
    }
}

impl<T> AsMut<T> for SlotHeapPeekMut<'_, T>
where
    T: PartialOrd,
{
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
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
}

impl<'a, T> From<(RwLockReadGuard<'a, inner::SlotHeap<T>>, usize)> for SlotHeapRef<'a, T>
where
    T: PartialOrd,
{
    fn from((guard, id): (RwLockReadGuard<'a, inner::SlotHeap<T>>, usize)) -> Self {
        Self { guard, id }
    }
}

impl<T> Deref for SlotHeapRef<'_, T>
where
    T: PartialOrd,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.guard.get_unchecked(self.id) }
    }
}

impl<T> AsRef<T> for SlotHeapRef<'_, T>
where
    T: PartialOrd,
{
    fn as_ref(&self) -> &T {
        self.deref()
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
}

impl<'a, T> From<(RwLockWriteGuard<'a, inner::SlotHeap<T>>, usize)> for SlotHeapRefMut<'a, T>
where
    T: PartialOrd,
{
    fn from((guard, id): (RwLockWriteGuard<'a, inner::SlotHeap<T>>, usize)) -> Self {
        Self {
            guard,
            id,
            dirty: false,
        }
    }
}

impl<T> Deref for SlotHeapRefMut<'_, T>
where
    T: PartialOrd,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.guard.get_unchecked(self.id) }
    }
}

impl<T> AsRef<T> for SlotHeapRefMut<'_, T>
where
    T: PartialOrd,
{
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<T> DerefMut for SlotHeapRefMut<'_, T>
where
    T: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { self.guard.get_unchecked_mut(self.id) }
    }
}

impl<T> AsMut<T> for SlotHeapRefMut<'_, T>
where
    T: PartialOrd,
{
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}

impl<T> Drop for SlotHeapRefMut<'_, T>
where
    T: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            let index = unsafe { self.guard.get_unchecked_index(self.id) };
            unsafe { self.guard.heapify(index) }
        }
    }
}
