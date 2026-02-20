use crossbeam_utils::CachePadded;
use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::{
    mem::{self, ManuallyDrop},
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{unsync, util};

/// Thread-safe slot map with stable, reusable IDs.
///
/// Each inserted value receives a stable `usize` ID that remains valid until
/// the value is removed. The map is partitioned into a fixed number of
/// independent shards, each protected by its own [`RwLock`]; the lower bits
/// of every ID encode the shard index, so operations on different shards
/// proceed in parallel. On insertion, four candidate shards are sampled and
/// the one with the fewest live entries is chosen. The shard count must be a
/// power of two and at least 4; [`with_num_shards`][SlotMap::with_num_shards]
/// returns `None` for invalid values. All operations have the time
/// complexities documented on the methods below.
///
/// # Examples
///
/// ```rust
/// use deadlock::sync::SlotMap;
///
/// let map = SlotMap::new();
/// let a = map.insert(10);
/// let b = map.insert(20);
/// assert_eq!(*map.get(a).unwrap(), 10);
/// map.remove(a);
/// assert!(map.get(a).is_none());
/// ```
pub struct SlotMap<T> {
    shards: Box<[CachePadded<Shard<T>>]>,
    rr: AtomicUsize,
}

struct Shard<T> {
    inner: RwLock<unsync::SlotMap<T>>,
    len: AtomicUsize,
}

impl<T> SlotMap<T> {
    /// Creates a new `SlotMap` with a shard count derived from the available
    /// hardware parallelism (`next_power_of_two * 4`). Time: O(num_shards).
    pub fn new() -> Self {
        unsafe { Self::with_num_shards_unchecked(util::default_num_shards()) }
    }

    /// Creates a new `SlotMap` with exactly `num_shards` shards. Returns `None`
    /// if `num_shards` is not a power of two or is less than 4.
    /// Time: O(num_shards).
    pub fn with_num_shards(num_shards: usize) -> Option<Self> {
        (num_shards.is_power_of_two() && num_shards >= 4)
            .then(|| unsafe { Self::with_num_shards_unchecked(num_shards) })
    }

    /// Creates a new `SlotMap` with exactly `num_shards` shards without
    /// validating the argument. Time: O(num_shards).
    ///
    /// # Safety
    ///
    /// `num_shards` must be a power of two and at least 4.
    pub unsafe fn with_num_shards_unchecked(num_shards: usize) -> Self {
        Self {
            shards: (0..num_shards)
                .map(|_| {
                    CachePadded::new(Shard {
                        inner: RwLock::new(unsync::SlotMap::new()),
                        len: AtomicUsize::new(0),
                    })
                })
                .collect(),
            rr: AtomicUsize::new(0),
        }
    }

    /// Returns the number of shards. Time: O(1).
    pub fn num_shards(&self) -> usize {
        self.shards.len()
    }

    /// Returns the total number of live entries across all shards. Each shard's
    /// count is read with `Relaxed` ordering, so the result is approximate.
    /// Time: O(num_shards).
    pub fn len(&self) -> usize {
        self.shards
            .iter()
            .map(|shard| shard.len.load(Ordering::Relaxed))
            .sum()
    }

    /// Returns `true` if every shard reports zero live entries. Time: O(num_shards).
    pub fn is_empty(&self) -> bool {
        self.shards
            .iter()
            .all(|shard| shard.len.load(Ordering::Relaxed) == 0)
    }

    /// Removes all entries from every shard, acquiring each shard's write lock
    /// in sequence. Time: O(num_shards + total capacity).
    pub fn clear(&self) {
        for shard in self.shards.iter() {
            let mut inner = shard.inner.write();
            inner.clear();
            shard.len.store(0, Ordering::Relaxed);
        }
    }

    /// Returns `true` if `id` refers to a live entry. Time: O(1).
    pub fn contains(&self, id: usize) -> bool {
        let (shard_index, id) = self.split(id);
        self.shards
            .get(shard_index)
            .map(|shard| shard.inner.read().contains(id))
            .unwrap_or(false)
    }

    /// Returns a read-locked guard for the value at `id`, or `None` if `id` is
    /// not a live entry. Time: O(1).
    pub fn get(&self, id: usize) -> Option<MappedRwLockReadGuard<'_, T>> {
        let (shard_index, id) = self.split(id);
        let shard = self.shards.get(shard_index)?;
        RwLockReadGuard::try_map(shard.inner.read(), |inner| inner.get(id)).ok()
    }

    /// Returns a read-locked guard for the value at `id` without liveness
    /// checking. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live entry.
    pub unsafe fn get_unchecked(&self, id: usize) -> MappedRwLockReadGuard<'_, T> {
        let (shard_index, id) = self.split(id);
        let shard = unsafe { self.shards.get_unchecked(shard_index) };
        RwLockReadGuard::map(shard.inner.read(), |inner| unsafe {
            inner.get_unchecked(id)
        })
    }

    /// Returns a write-locked guard for the value at `id`, or `None` if `id`
    /// is not a live entry. Time: O(1).
    pub fn get_mut(&self, id: usize) -> Option<MappedRwLockWriteGuard<'_, T>> {
        let (shard_index, id) = self.split(id);
        let shard = self.shards.get(shard_index)?;
        RwLockWriteGuard::try_map(shard.inner.write(), |inner| inner.get_mut(id)).ok()
    }

    /// Returns a write-locked guard for the value at `id` without liveness
    /// checking. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live entry.
    pub unsafe fn get_unchecked_mut(&self, id: usize) -> MappedRwLockWriteGuard<'_, T> {
        let (shard_index, id) = self.split(id);
        let shard = unsafe { self.shards.get_unchecked(shard_index) };
        RwLockWriteGuard::map(shard.inner.write(), |inner| unsafe {
            inner.get_unchecked_mut(id)
        })
    }

    /// Inserts `value` and returns its stable ID. Picks the least-loaded shard
    /// among four round-robin candidates. Time: O(1) amortized per shard.
    pub fn insert(&self, value: T) -> usize {
        let shard_index = self.select_shard();
        let shard = unsafe { self.shards.get_unchecked(shard_index) };

        let mut guard = shard.inner.write();
        let id = guard.insert(value);
        shard.len.fetch_add(1, Ordering::Relaxed);

        self.merge(shard_index, id)
    }

    /// Reserves a slot in the same way as [`insert`][Self::insert] (shard choice,
    /// etc.) and returns the stable ID plus a guard. The slot is not live until
    /// the guard is committed; if the guard is dropped without [`commit`][LazyInsert::commit],
    /// the slot is freed. Time: O(1) amortized per shard.
    pub fn lazy_insert(&self) -> (usize, LazyInsert<'_, T>) {
        let shard_index = self.select_shard();
        let shard = unsafe { self.shards.get_unchecked(shard_index) };
        let mut guard = shard.inner.write();
        let id = guard.prepare_lazy_insert();
        (
            self.merge(shard_index, id),
            LazyInsert {
                guard: ManuallyDrop::new(guard),
                shard,
                id,
            },
        )
    }

    /// Removes the entry at `id` and returns its value, or `None` if `id` is
    /// not a live entry. Time: O(1).
    pub fn remove(&self, id: usize) -> Option<T> {
        let (shard_index, id) = self.split(id);
        let shard = self.shards.get(shard_index)?;

        let mut guard = shard.inner.write();
        let value = guard.remove(id)?;
        shard.len.fetch_sub(1, Ordering::Relaxed);
        value.into()
    }

    /// Removes the entry at `id` and returns its value without liveness
    /// checking. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live entry.
    pub unsafe fn remove_unchecked(&self, id: usize) -> T {
        let (shard_index, id) = self.split(id);
        let shard = unsafe { self.shards.get_unchecked(shard_index) };

        let mut guard = shard.inner.write();
        let value = unsafe { guard.remove_unchecked(id) };
        shard.len.fetch_sub(1, Ordering::Relaxed);
        value
    }

    /// Swaps the values at `id0` and `id1` in-place. Returns `None` if either
    /// ID is not a live entry. When the IDs belong to different shards, two
    /// write locks are acquired; `reverse` controls lock order—use a consistent
    /// value per shard pair to avoid deadlock. Time: O(1).
    pub fn swap(&self, id0: usize, id1: usize, reverse: bool) -> Option<()> {
        let (shard_index0, id0) = self.split(id0);
        let (shard_index1, id1) = self.split(id1);

        if shard_index0 == shard_index1 {
            let shard = self.shards.get(shard_index0)?;
            let mut guard = shard.inner.write();
            guard.swap(id0, id1)
        } else {
            let shard0 = self.shards.get(shard_index0)?;
            let shard1 = self.shards.get(shard_index1)?;

            let mut guard0;
            let mut guard1;

            if (shard_index0 < shard_index1) ^ reverse {
                guard0 = shard0.inner.write();
                guard1 = shard1.inner.write();
            } else {
                guard1 = shard1.inner.write();
                guard0 = shard0.inner.write();
            }

            let elem0 = guard0.get_mut(id0)?;
            let elem1 = guard1.get_mut(id1)?;
            mem::swap(elem0, elem1).into()
        }
    }

    /// Swaps the values at `id0` and `id1` in-place without liveness checking.
    /// Time: O(1).
    ///
    /// # Safety
    ///
    /// Both `id0` and `id1` must refer to live entries.
    pub unsafe fn swap_unchecked(&self, id0: usize, id1: usize, reverse: bool) {
        let (shard_index0, id0) = self.split(id0);
        let (shard_index1, id1) = self.split(id1);

        if shard_index0 == shard_index1 {
            let shard = unsafe { self.shards.get_unchecked(shard_index0) };
            let mut guard = shard.inner.write();
            unsafe { guard.swap_unchecked(id0, id1) }
        } else {
            let shard0 = unsafe { self.shards.get_unchecked(shard_index0) };
            let shard1 = unsafe { self.shards.get_unchecked(shard_index1) };

            let mut guard0;
            let mut guard1;

            if (shard_index0 < shard_index1) ^ reverse {
                guard0 = shard0.inner.write();
                guard1 = shard1.inner.write();
            } else {
                guard1 = shard1.inner.write();
                guard0 = shard0.inner.write();
            }

            let elem0 = unsafe { guard0.get_unchecked_mut(id0) };
            let elem1 = unsafe { guard1.get_unchecked_mut(id1) };
            mem::swap(elem0, elem1)
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

    fn merge(&self, shard_index: usize, id: usize) -> usize {
        (id << self.num_shift()) | shard_index
    }

    fn split(&self, id: usize) -> (usize, usize) {
        (id & self.rr_mask(), id >> self.num_shift())
    }

    fn num_shift(&self) -> u32 {
        self.num_shards().trailing_zeros()
    }

    fn rr_mask(&self) -> usize {
        self.num_shards() - 1
    }

    fn rr_interval(&self) -> usize {
        self.num_shards() >> 2
    }
}

/// Guard for a slot reserved by [`SlotMap::lazy_insert`]. Call [`commit`][Self::commit]
/// to store a value and make the slot live; otherwise the slot is freed on drop.
pub struct LazyInsert<'a, T> {
    guard: ManuallyDrop<RwLockWriteGuard<'a, unsync::SlotMap<T>>>,
    shard: &'a Shard<T>,
    id: usize,
}

impl<'a, T> LazyInsert<'a, T> {
    /// Stores `value` in the reserved slot and makes it live. Consumes the guard
    /// so that drop does not run and the slot is not freed.
    pub fn commit(mut self, value: T) {
        unsafe { self.guard.commit_lazy_insert(self.id, value) };
        self.shard.len.fetch_add(1, Ordering::Relaxed);
        unsafe { ManuallyDrop::drop(&mut self.guard) }
        mem::forget(self)
    }
}

impl<'a, T> Drop for LazyInsert<'a, T> {
    fn drop(&mut self) {
        unsafe {
            self.guard.drop_lazy_insert(self.id);
            ManuallyDrop::drop(&mut self.guard)
        }
    }
}

impl<T> Default for SlotMap<T> {
    fn default() -> Self {
        Self::new()
    }
}
