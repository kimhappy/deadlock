//! Thread-safe slot map with stable RAII handle.

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    iter,
    mem::{self, ManuallyDrop},
    sync::atomic::{AtomicUsize, Ordering},
};
use triomphe::Arc;

use crate::{inner, util};

/// Thread-safe slot map with stable RAII handle.
///
/// Stores values in slots and returns [`SlotMapId`].
pub struct SlotMap<T> {
    shards: Box<[Arc<Shard<T>>]>,
    rr: AtomicUsize,
}

struct Shard<T> {
    inner: RwLock<inner::SlotMap<T>>,
    len: AtomicUsize,
}

impl<T> SlotMap<T> {
    /// Creates a new slot map with a default number of shards (derived from parallelism).
    pub fn new() -> Self {
        let num_shards = util::default_num_shards();
        unsafe { Self::new_unchecked(num_shards) }
    }

    /// Returns the number of entries in the map.
    ///
    /// Time complexity: O(# of shards)
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| shard.len.load(Ordering::Relaxed))
            .sum()
    }

    /// Returns whether the map is empty.
    ///
    /// Time complexity: O(# of shards)
    pub fn is_empty(&self) -> bool {
        self.shards
            .iter()
            .all(|shard| shard.len.load(Ordering::Relaxed) == 0)
    }

    /// Inserts a value and returns its handle.
    ///
    /// Time complexity: O(1)
    pub fn insert(&self, value: T) -> SlotMapId<T> {
        let shard_index = self.select_shard();
        let shard = unsafe { self.shards.get_unchecked(shard_index) };
        let from = ManuallyDrop::new(shard.clone());

        let mut guard = shard.inner.write();
        let id = guard.insert(value);
        shard.len.fetch_add(1, Ordering::Relaxed);

        SlotMapId { from, id }
    }

    /// Creates an iterator over immutable references to values in the map.
    ///
    /// Each call to `next()` acquires and releases a read lock for each individual element.
    pub fn iter(&self) -> SlotMapIter<'_, T> {
        SlotMapIter {
            shards: &self.shards,
            shard_index: 0,
            inner_index: 0,
        }
    }

    /// Creates an iterator over mutable references to values in the map.
    ///
    /// Each call to `next()` acquires and releases a write lock for each individual element.
    pub fn iter_mut(&self) -> SlotMapIterMut<'_, T> {
        SlotMapIterMut {
            shards: &self.shards,
            shard_index: 0,
            inner_index: 0,
        }
    }

    /// Returns an iterator over shard references, each holding a read lock for an entire shard.
    ///
    /// Unlike [`iter`](Self::iter), which acquires and releases a lock per element,
    /// each [`SlotMapShardRef`] holds its read lock for the lifetime of the shard reference.
    /// This is more efficient when all values in a shard need to be processed at once.
    pub fn shards(&self) -> impl Iterator<Item = SlotMapShardRef<'_, T>> {
        self.shards.iter().map(|shard| SlotMapShardRef {
            guard: shard.inner.read(),
        })
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

/// Stable RAII handle to a value in a [`SlotMap`].
///
/// Dropping it removes the value from the map.
pub struct SlotMapId<T> {
    from: ManuallyDrop<Arc<Shard<T>>>,
    id: usize,
}

impl<T> SlotMapId<T> {
    /// Takes the value out of the map with consuming self.
    ///
    /// Time complexity: O(1)
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
    /// Time complexity: O(1)
    pub fn get(&self) -> SlotMapRef<'_, T> {
        let guard = self.from.inner.read();
        SlotMapRef { guard, id: self.id }
    }

    /// Returns a mutable reference to the value, holding a write lock until the ref is dropped.
    ///
    /// Time complexity: O(1)
    pub fn get_mut(&self) -> SlotMapRefMut<'_, T> {
        let guard = self.from.inner.write();
        SlotMapRefMut { guard, id: self.id }
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

#[reflica::reflica]
impl<T> SlotMapRef<'_, T> {
    fn deref(&self) -> &T {
        unsafe { self.guard.get_unchecked(self.id) }
    }
}

/// Mutable reference to a value in a [`SlotMap`], holding a write lock.
pub struct SlotMapRefMut<'a, T> {
    guard: RwLockWriteGuard<'a, inner::SlotMap<T>>,
    id: usize,
}

#[reflica::reflica]
impl<T> SlotMapRefMut<'_, T> {
    fn deref(&self) -> &T {
        unsafe { self.guard.get_unchecked(self.id) }
    }

    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.guard.get_unchecked_mut(self.id) }
    }
}

/// A read-locked view of a single internal shard of a [`SlotMap`].
///
/// Created by [`SlotMap::shards`]. Holds a read lock on the shard for its entire lifetime,
/// preventing concurrent writes to that shard while the reference exists.
pub struct SlotMapShardRef<'a, T> {
    guard: RwLockReadGuard<'a, inner::SlotMap<T>>,
}

impl<T> SlotMapShardRef<'_, T> {
    /// Returns an iterator over immutable references to all values in this shard.
    ///
    /// The read lock is held for the entire lifetime of the returned iterator.
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        (0..self.guard.len()).map(move |index| unsafe { self.guard.get_unchecked_nth(index) })
    }
}

/// Iterator over immutable references to values in a [`SlotMap`].
///
/// Created by [`SlotMap::iter`]. Each call to [`next`](Iterator::next) acquires and releases
/// a read lock for a single element. This allows fine-grained locking but may have overhead
/// when iterating many elements.
pub struct SlotMapIter<'a, T> {
    shards: &'a [Arc<Shard<T>>],
    shard_index: usize,
    inner_index: usize,
}

impl<'a, T> Iterator for SlotMapIter<'a, T> {
    type Item = SlotMapRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(loop {
            let shard = self.shards.get(self.shard_index)?;
            let guard = shard.inner.read();

            if self.inner_index >= guard.len() {
                self.shard_index += 1;
                self.inner_index = 0;
                continue;
            }

            let id = unsafe { guard.get_unchecked_nth_id(self.inner_index) };
            self.inner_index += 1;
            break SlotMapRef { guard, id };
        })
    }
}

/// Iterator over mutable references to values in a [`SlotMap`].
///
/// Created by [`SlotMap::iter_mut`]. Each call to [`next`](Iterator::next) acquires and releases
/// a write lock for a single element. This allows fine-grained locking but may have overhead
/// when iterating many elements.
pub struct SlotMapIterMut<'a, T> {
    shards: &'a [Arc<Shard<T>>],
    shard_index: usize,
    inner_index: usize,
}

impl<'a, T> Iterator for SlotMapIterMut<'a, T> {
    type Item = SlotMapRefMut<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(loop {
            let shard = self.shards.get(self.shard_index)?;
            let guard = shard.inner.write();

            if self.inner_index >= guard.len() {
                self.shard_index += 1;
                self.inner_index = 0;
                continue;
            }

            let id = unsafe { guard.get_unchecked_nth_id(self.inner_index) };
            self.inner_index += 1;
            break SlotMapRefMut { guard, id };
        })
    }
}

unsafe impl<T> Send for SlotMap<T> where T: Send {}
unsafe impl<T> Sync for SlotMap<T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapId<T> where T: Send {}
unsafe impl<T> Sync for SlotMapId<T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapRef<'_, T> where T: Send + Sync {}
unsafe impl<T> Sync for SlotMapRef<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapRefMut<'_, T> where T: Send {}
unsafe impl<T> Sync for SlotMapRefMut<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapShardRef<'_, T> where T: Send + Sync {}
unsafe impl<T> Sync for SlotMapShardRef<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapIter<'_, T> where T: Send + Sync {}
unsafe impl<T> Sync for SlotMapIter<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapIterMut<'_, T> where T: Send {}
unsafe impl<T> Sync for SlotMapIterMut<'_, T> where T: Send + Sync {}
