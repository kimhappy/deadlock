use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::{
    iter, mem,
    sync::atomic::{AtomicUsize, Ordering},
};
use triomphe::Arc;

use crate::{unsync::SlotMap as Inner, util};

/// Thread-safe slot map with RAII owning IDs.
///
/// Sharded like [`sync::SlotMap`](crate::sync::SlotMap); [`insert`](SlotMap::insert) returns a
/// [`SlotMapId`] that owns the entry. Dropping the ID without calling [`remove`](SlotMapId::remove)
/// removes the entry (RAII). Shard count must be a power of two and at least 4. Time complexities
/// match the underlying unsync map per shard.
///
/// # Examples
///
/// ```rust
/// use deadlock::raii::SlotMap;
///
/// let map = SlotMap::new();
/// let a = map.insert(10);
/// let b = map.insert(20);
/// assert_eq!(*a.get(), 10);
/// drop(a);
/// assert_eq!(map.len(), 1);
/// ```
pub struct SlotMap<T> {
    shards: Box<[Arc<Shard<T>>]>,
    rr: AtomicUsize,
}

/// Owning handle to a single entry in a [`SlotMap`]. Dropping it removes the entry.
pub struct SlotMapId<T> {
    from: Arc<Shard<T>>,
    id: usize,
}

struct Shard<T> {
    inner: RwLock<Inner<T>>,
    len: AtomicUsize,
}

impl<T> SlotMap<T> {
    /// Creates a new map with a shard count from hardware parallelism. Time: O(num_shards).
    pub fn new() -> Self {
        unsafe { Self::with_num_shards_unchecked(util::default_num_shards()) }
    }

    /// Creates a new map with exactly `num_shards` shards. Returns `None` if `num_shards` is not a power of two or is less than 4. Time: O(num_shards).
    pub fn with_num_shards(num_shards: usize) -> Option<Self> {
        (num_shards.is_power_of_two() && num_shards >= 4)
            .then(|| unsafe { Self::with_num_shards_unchecked(num_shards) })
    }

    /// Creates a new map with exactly `num_shards` shards without validation. Time: O(num_shards).
    ///
    /// # Safety
    ///
    /// `num_shards` must be a power of two and at least 4.
    pub unsafe fn with_num_shards_unchecked(num_shards: usize) -> Self {
        Self {
            shards: iter::repeat_with(|| {
                Arc::new(Shard {
                    inner: RwLock::new(Inner::new()),
                    len: AtomicUsize::new(0),
                })
            })
            .take(num_shards)
            .collect(),
            rr: AtomicUsize::new(0),
        }
    }

    /// Returns the total number of live entries (approximate, relaxed ordering). Time: O(num_shards).
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| shard.len.load(Ordering::Relaxed))
            .sum()
    }

    /// Returns `true` if every shard reports zero entries. Time: O(num_shards).
    pub fn is_empty(&self) -> bool {
        self.shards
            .iter()
            .all(|shard| shard.len.load(Ordering::Relaxed) == 0)
    }

    /// Inserts `value` and returns an owning ID. Picks the least-loaded of four round-robin shards. Time: O(1) amortized per shard.
    pub fn insert(&self, value: T) -> SlotMapId<T> {
        let shard_index = self.select_shard();
        let shard = unsafe { self.shards.get_unchecked(shard_index) };
        let from = shard.clone();

        let mut guard = shard.inner.write();
        let id = guard.insert(value);
        shard.len.fetch_add(1, Ordering::Relaxed);

        SlotMapId { from, id }
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
    /// Removes the entry and returns its value. Consumes the ID without running drop. Time: O(1).
    pub fn remove(self) -> T {
        let mut guard = self.from.inner.write();
        let value = unsafe { guard.remove_unchecked(self.id) };
        self.from.len.fetch_sub(1, Ordering::Relaxed);
        drop(guard);
        mem::forget(self);
        value
    }

    /// Returns a shared reference to the value. Time: O(1).
    pub fn get(&self) -> MappedRwLockReadGuard<'_, T> {
        let guard = self.from.inner.read();
        RwLockReadGuard::map(guard, |inner| unsafe { inner.get_unchecked(self.id) })
    }

    /// Returns an exclusive reference to the value. Time: O(1).
    pub fn get_mut(&self) -> MappedRwLockWriteGuard<'_, T> {
        let guard = self.from.inner.write();
        RwLockWriteGuard::map(guard, |inner| unsafe { inner.get_unchecked_mut(self.id) })
    }

    /// Swaps the values of the two entries. No fixed lock order; use [`swap_ordered`](SlotMapId::swap_ordered) to avoid deadlock when ordering matters. Time: O(1).
    pub fn swap(&self, other: &Self) {
        if Arc::ptr_eq(&self.from, &other.from) {
            let mut guard = self.from.inner.write();
            unsafe { guard.swap_unchecked(self.id, other.id) }
            return;
        }

        let mut guard0 = self.from.inner.write();
        let mut guard1 = other.from.inner.write();
        let elem0 = unsafe { guard0.get_unchecked_mut(self.id) };
        let elem1 = unsafe { guard1.get_unchecked_mut(other.id) };
        mem::swap(elem0, elem1)
    }

    /// Swaps the values of the two entries, acquiring shard locks in a deterministic order (by shard pointer) to avoid deadlock. Time: O(1).
    pub fn swap_ordered(&self, other: &Self) {
        if Arc::ptr_eq(&self.from, &other.from) {
            let mut guard = self.from.inner.write();
            unsafe { guard.swap_unchecked(self.id, other.id) }
            return;
        }

        if self.from.as_ptr() < other.from.as_ptr() {
            let mut guard0 = self.from.inner.write();
            let mut guard1 = other.from.inner.write();
            let elem0 = unsafe { guard0.get_unchecked_mut(self.id) };
            let elem1 = unsafe { guard1.get_unchecked_mut(other.id) };
            mem::swap(elem0, elem1)
        } else {
            let mut guard0 = other.from.inner.write();
            let mut guard1 = self.from.inner.write();
            let elem0 = unsafe { guard0.get_unchecked_mut(other.id) };
            let elem1 = unsafe { guard1.get_unchecked_mut(self.id) };
            mem::swap(elem0, elem1)
        }
    }
}

impl<T> Drop for SlotMapId<T> {
    fn drop(&mut self) {
        let mut guard = self.from.inner.write();
        unsafe { guard.remove_unchecked(self.id) };
        self.from.len.fetch_sub(1, Ordering::Relaxed);
    }
}
