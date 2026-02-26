//! Thread-safe slot map with stable RAII handle.

use parking_lot::{RwLock, RwLockReadGuard, RwLockWriteGuard};
use std::{
    iter,
    mem::{self, ManuallyDrop},
    ptr::NonNull,
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
        let num_shards = util::default_num_shards();
        unsafe { Self::new_unchecked(num_shards) }
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

    /// Creates an iterator over immutable references to values in the map.
    ///
    /// Each call to `next()` acquires and releases a read lock for each individual element.
    /// For better performance when iterating many elements, consider using [`arc_iter`](SlotMap::arc_iter)
    /// which holds a lock per shard.
    ///
    /// Time complexity: O(n) where n is the number of elements.
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
    /// For better performance when iterating many elements, consider using [`arc_iter_mut`](SlotMap::arc_iter_mut)
    /// which holds a lock per shard.
    ///
    /// Time complexity: O(n) where n is the number of elements.
    pub fn iter_mut(&self) -> SlotMapIterMut<'_, T> {
        SlotMapIterMut {
            shards: &self.shards,
            shard_index: 0,
            inner_index: 0,
        }
    }

    /// Creates a shard-aware iterator over immutable references to values in the map.
    ///
    /// This iterator acquires a read lock per shard and holds it while iterating through
    /// all elements in that shard, then moves to the next shard. This is more efficient
    /// than [`iter`](SlotMap::iter) which acquires/releases a lock for each element.
    ///
    /// Time complexity: O(n) where n is the number of elements.
    pub fn arc_iter(&self) -> SlotMapArcIter<'_, T> {
        SlotMapArcIter {
            shards: &self.shards,
            shard_index: 0,
            guard: None,
            inner_index: 0,
        }
    }

    /// Creates a shard-aware iterator over mutable references to values in the map.
    ///
    /// This iterator acquires a write lock per shard and holds it while iterating through
    /// all elements in that shard, then moves to the next shard. This is more efficient
    /// than [`iter_mut`](SlotMap::iter_mut) which acquires/releases a lock for each element.
    ///
    /// Time complexity: O(n) where n is the number of elements.
    pub fn arc_iter_mut(&self) -> SlotMapArcIterMut<'_, T> {
        SlotMapArcIterMut {
            shards: &self.shards,
            shard_index: 0,
            guard: None,
            inner_index: 0,
        }
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
        let ptr = unsafe { guard.get_unchecked_ptr(self.id) };
        SlotMapRef { _guard: guard, ptr }
    }

    /// Returns a mutable reference to the value, holding a write lock until the ref is dropped.
    ///
    /// Time complexity: O(1).
    pub fn get_mut(&self) -> SlotMapRefMut<'_, T> {
        let guard = self.from.inner.write();
        let ptr = unsafe { guard.get_unchecked_ptr(self.id) };
        SlotMapRefMut { _guard: guard, ptr }
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
    _guard: RwLockReadGuard<'a, inner::SlotMap<T>>,
    ptr: NonNull<T>,
}

#[reflica::reflica]
impl<T> SlotMapRef<'_, T> {
    fn deref(&self) -> &T {
        unsafe { self.ptr.as_ref() }
    }
}

/// Mutable reference to a value in a [`SlotMap`], holding a write lock.
pub struct SlotMapRefMut<'a, T> {
    _guard: RwLockWriteGuard<'a, inner::SlotMap<T>>,
    ptr: NonNull<T>,
}

#[reflica::reflica]
impl<T> SlotMapRefMut<'_, T> {
    fn deref(&self) -> &T {
        unsafe { self.ptr.as_ref() }
    }

    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.ptr.as_mut() }
    }
}

/// Immutable reference with Arc-wrapped read lock for use in [`SlotMapArcIter`].
///
/// Unlike [`SlotMapRef`], this type wraps the lock guard in an [`Arc`], allowing
/// the lock to be shared across multiple references while iterating through a shard.
pub struct SlotMapArcRef<'a, T> {
    _guard: Arc<RwLockReadGuard<'a, inner::SlotMap<T>>>,
    ptr: NonNull<T>,
}

#[reflica::reflica]
impl<T> SlotMapArcRef<'_, T> {
    fn deref(&self) -> &T {
        unsafe { self.ptr.as_ref() }
    }
}

/// Mutable reference with Arc-wrapped write lock for use in [`SlotMapArcIterMut`].
///
/// Unlike [`SlotMapRefMut`], this type wraps the lock guard in an [`Arc`], allowing
/// the lock to be shared across multiple references while iterating through a shard.
pub struct SlotMapArcRefMut<'a, T> {
    _guard: Arc<RwLockWriteGuard<'a, inner::SlotMap<T>>>,
    ptr: NonNull<T>,
}

#[reflica::reflica]
impl<T> SlotMapArcRefMut<'_, T> {
    fn deref(&self) -> &T {
        unsafe { self.ptr.as_ref() }
    }

    fn deref_mut(&mut self) -> &mut T {
        unsafe { self.ptr.as_mut() }
    }
}

/// Iterator over immutable references to values in a [`SlotMap`].
///
/// Created by [`SlotMap::iter`]. Each call to [`next`](Iterator::next) acquires and releases
/// a read lock for a single element. This allows fine-grained locking but may have overhead
/// when iterating many elements.
///
/// For better performance when iterating through many elements, consider using
/// [`SlotMap::arc_iter`] which holds a lock per shard.
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

            let ptr = unsafe { guard.get_unchecked_nth_ptr(self.inner_index) };
            self.inner_index += 1;
            break SlotMapRef { _guard: guard, ptr };
        })
    }
}

/// Iterator over mutable references to values in a [`SlotMap`].
///
/// Created by [`SlotMap::iter_mut`]. Each call to [`next`](Iterator::next) acquires and releases
/// a write lock for a single element. This allows fine-grained locking but may have overhead
/// when iterating many elements.
///
/// For better performance when iterating through many elements, consider using
/// [`SlotMap::arc_iter_mut`] which holds a lock per shard.
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

            let ptr = unsafe { guard.get_unchecked_nth_ptr(self.inner_index) };
            self.inner_index += 1;
            break SlotMapRefMut { _guard: guard, ptr };
        })
    }
}

/// Shard-aware iterator over immutable references to values in a [`SlotMap`].
///
/// Created by [`SlotMap::arc_iter`]. This iterator acquires a read lock per shard and holds it
/// while iterating through all elements in that shard, then moves to the next shard.
///
/// This is more efficient than [`SlotMapIter`] which acquires and releases a lock for each element.
/// The lock guard is wrapped in an [`Arc`] and shared across all [`SlotMapArcRef`] items
/// from the same shard, allowing the lock to be released only when the last reference is dropped.
pub struct SlotMapArcIter<'a, T> {
    shards: &'a [Arc<Shard<T>>],
    shard_index: usize,
    guard: Option<Arc<RwLockReadGuard<'a, inner::SlotMap<T>>>>,
    inner_index: usize,
}

impl<'a, T> Iterator for SlotMapArcIter<'a, T> {
    type Item = SlotMapArcRef<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(loop {
            if self.guard.is_none() {
                let shard = self.shards.get(self.shard_index)?;
                self.shard_index += 1;
                let guard = shard.inner.read();

                if guard.len() == 0 {
                    continue;
                }

                self.guard = Some(Arc::new(guard));
            }

            let guard = unsafe { self.guard.as_ref().unwrap_unchecked() };
            let ptr = unsafe { guard.get_unchecked_nth_ptr(self.inner_index) };
            self.inner_index += 1;

            let guard = if self.inner_index >= guard.len() {
                self.inner_index = 0;
                unsafe { self.guard.take().unwrap_unchecked() }
            } else {
                guard.clone()
            };

            break SlotMapArcRef { _guard: guard, ptr };
        })
    }
}

/// Shard-aware iterator over mutable references to values in a [`SlotMap`].
///
/// Created by [`SlotMap::arc_iter_mut`]. This iterator acquires a write lock per shard and holds it
/// while iterating through all elements in that shard, then moves to the next shard.
///
/// This is more efficient than [`SlotMapIterMut`] which acquires and releases a lock for each element.
/// The lock guard is wrapped in an [`Arc`] and shared across all [`SlotMapArcRefMut`] items
/// from the same shard, allowing the lock to be released only when the last reference is dropped.
pub struct SlotMapArcIterMut<'a, T> {
    shards: &'a [Arc<Shard<T>>],
    shard_index: usize,
    guard: Option<Arc<RwLockWriteGuard<'a, inner::SlotMap<T>>>>,
    inner_index: usize,
}

impl<'a, T> Iterator for SlotMapArcIterMut<'a, T> {
    type Item = SlotMapArcRefMut<'a, T>;

    fn next(&mut self) -> Option<Self::Item> {
        Some(loop {
            if self.guard.is_none() {
                let shard = self.shards.get(self.shard_index)?;
                self.shard_index += 1;
                let guard = shard.inner.write();

                if guard.len() == 0 {
                    continue;
                }

                self.guard = Some(Arc::new(guard));
            }

            let guard = unsafe { self.guard.as_ref().unwrap_unchecked() };
            let ptr = unsafe { guard.get_unchecked_nth_ptr(self.inner_index) };
            self.inner_index += 1;

            let guard = if self.inner_index >= guard.len() {
                self.inner_index = 0;
                unsafe { self.guard.take().unwrap_unchecked() }
            } else {
                guard.clone()
            };

            break SlotMapArcRefMut { _guard: guard, ptr };
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

unsafe impl<T> Send for SlotMapArcRef<'_, T> where T: Send + Sync {}
unsafe impl<T> Sync for SlotMapArcRef<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapArcRefMut<'_, T> where T: Send {}
unsafe impl<T> Sync for SlotMapArcRefMut<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapIter<'_, T> where T: Send + Sync {}
unsafe impl<T> Sync for SlotMapIter<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapIterMut<'_, T> where T: Send {}
unsafe impl<T> Sync for SlotMapIterMut<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapArcIter<'_, T> where T: Send + Sync {}
unsafe impl<T> Sync for SlotMapArcIter<'_, T> where T: Send + Sync {}

unsafe impl<T> Send for SlotMapArcIterMut<'_, T> where T: Send {}
unsafe impl<T> Sync for SlotMapArcIterMut<'_, T> where T: Send + Sync {}
