//! Thread-safe slot map with stable RAII handle.

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    iter,
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicUsize, Ordering},
};
use triomphe::Arc;

use crate::{inner, util};

/// Thread-safe slot map with stable RAII handle.
///
/// Stores values in slots and returns stable RAII handle. Handles remain valid until the corresponding
/// [`SlotMapId`] is dropped or consumed by [`SlotMapId::into_inner`].
pub struct SlotMap<T> {
    shards: Box<[Arc<Shard<T>>]>,
    rr: AtomicUsize,
}

/// Stable handle to a value in a [`SlotMap`].
///
/// Dropping it removes the value from the map. Use [`into_inner`](SlotMapId::into_inner) to
/// take the value out without removing the slot on drop.
pub struct SlotMapId<T> {
    from: ManuallyDrop<Arc<Shard<T>>>,
    id: usize,
}

struct Shard<T> {
    inner: RwLock<inner::SlotMap<T>>,
    len: AtomicUsize,
}

impl<T> SlotMap<T> {
    /// Creates a new slot map with a default number of shards (derived from parallelism).
    ///
    /// Time complexity: O(1) amortized (shard count is cached once).
    pub fn new() -> Self {
        unsafe { Self::new_unchecked(util::default_num_shards()) }
    }

    /// Returns the number of entries in the map.
    ///
    /// Time complexity: O(number of shards).
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| shard.len.load(Ordering::Relaxed))
            .sum()
    }

    /// Returns whether the map has no entries.
    ///
    /// Time complexity: O(number of shards).
    pub fn is_empty(&self) -> bool {
        self.shards
            .iter()
            .all(|shard| shard.len.load(Ordering::Relaxed) == 0)
    }

    /// Inserts a value and returns a handle to it.
    ///
    /// Time complexity: O(1) amortized.
    pub fn insert(&self, value: T) -> SlotMapId<T> {
        let shard_index = self.select_shard();
        let shard = unsafe { self.shards.get_unchecked(shard_index) };
        let from = ManuallyDrop::new(shard.clone());

        let mut guard = shard.inner.write();
        let id = guard.insert(value);
        shard.len.fetch_add(1, Ordering::Relaxed);

        SlotMapId { from, id }
    }

    unsafe fn new_unchecked(num_shards: usize) -> Self {
        Self {
            shards: iter::repeat_with(|| {
                Arc::new(Shard {
                    inner: RwLock::new(inner::SlotMap::new()),
                    len: 0.into(),
                })
            })
            .take(num_shards)
            .collect(),
            rr: 0.into(),
        }
    }

    fn select_shard(&self) -> usize {
        let rr = self.rr.fetch_add(1, Ordering::Relaxed);
        let candidates = (0..4).map(|i| {
            let index = (rr + i * self.rr_interval()) & self.rr_mask();
            let len = unsafe { self.shards.get_unchecked(index) }
                .len
                .load(Ordering::Relaxed);
            (index, len)
        });
        let min = candidates.min_by_key(|(_, len)| *len);
        unsafe { min.unwrap_unchecked() }.0
    }

    fn rr_mask(&self) -> usize {
        self.shards.len() - 1
    }

    fn rr_interval(&self) -> usize {
        self.shards.len() >> 2
    }
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SlotMapId<T> {
    /// Takes the value out of the map and invalidates this id (without running its destructor).
    ///
    /// Time complexity: O(1).
    pub fn into_inner(mut self) -> T {
        let mut guard = self.from.inner.write();
        let value = unsafe { guard.remove_unchecked(self.id) };
        self.from.len.fetch_sub(1, Ordering::Relaxed);
        drop(guard);
        unsafe { ManuallyDrop::drop(&mut self.from) };
        mem::forget(self);
        value
    }

    /// Returns an immutable reference to the value, holding a read lock until the ref is dropped.
    ///
    /// Time complexity: O(1).
    pub fn get(&self) -> SlotMapRef<'_, T> {
        let guard = self.from.inner.read();
        (guard, self.id).into()
    }

    /// Returns a mutable reference to the value, holding a write lock until the ref is dropped.
    ///
    /// Time complexity: O(1).
    pub fn get_mut(&self) -> SlotMapRefMut<'_, T> {
        let guard = self.from.inner.write();
        (guard, self.id).into()
    }
}

impl<T> Drop for SlotMapId<T> {
    fn drop(&mut self) {
        let mut guard = self.from.inner.write();
        unsafe { guard.remove_unchecked(self.id) };
        self.from.len.fetch_sub(1, Ordering::Relaxed);
        drop(guard);
        unsafe { ManuallyDrop::drop(&mut self.from) }
    }
}

/// Immutable reference to a value in a [`SlotMap`], holding a read lock.
pub struct SlotMapRef<'a, T> {
    guard: RwLockReadGuard<'a, inner::SlotMap<T>>,
    id: usize,
}

impl<'a, T> From<(RwLockReadGuard<'a, inner::SlotMap<T>>, usize)> for SlotMapRef<'a, T> {
    fn from((guard, id): (RwLockReadGuard<'a, inner::SlotMap<T>>, usize)) -> Self {
        Self { guard, id }
    }
}

impl<T> Deref for SlotMapRef<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.guard.get_unchecked(self.id) }
    }
}

impl<T> AsRef<T> for SlotMapRef<'_, T> {
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

/// Mutable reference to a value in a [`SlotMap`], holding a write lock.
pub struct SlotMapRefMut<'a, T> {
    guard: RwLockWriteGuard<'a, inner::SlotMap<T>>,
    id: usize,
}

impl<'a, T> From<(RwLockWriteGuard<'a, inner::SlotMap<T>>, usize)> for SlotMapRefMut<'a, T> {
    fn from((guard, id): (RwLockWriteGuard<'a, inner::SlotMap<T>>, usize)) -> Self {
        Self { guard, id }
    }
}

impl<T> Deref for SlotMapRefMut<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { self.guard.get_unchecked(self.id) }
    }
}

impl<T> AsRef<T> for SlotMapRefMut<'_, T> {
    fn as_ref(&self) -> &T {
        self.deref()
    }
}

impl<T> DerefMut for SlotMapRefMut<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.guard.get_unchecked_mut(self.id) }
    }
}

impl<T> AsMut<T> for SlotMapRefMut<'_, T> {
    fn as_mut(&mut self) -> &mut T {
        self.deref_mut()
    }
}
