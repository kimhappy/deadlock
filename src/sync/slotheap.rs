use parking_lot::{
    MappedRwLockReadGuard, MappedRwLockWriteGuard, RwLock, RwLockReadGuard, RwLockWriteGuard,
};
use std::ops::{Deref, DerefMut};

use crate::{unsync, util};

/// Thread-safe min-heap with stable, reusable IDs.
///
/// Elements are ordered by key `K` via [`Ord`]; each inserted `(key, value)`
/// receives a stable `usize` ID that remains valid until the element is
/// removed. All methods take `&self`; an internal [`RwLock`] serializes
/// concurrent mutations. Min access, key/value updates by ID, and heap repair
/// after key changes are provided via guard types ([`PeekMut`], [`PeekKeyMut`],
/// [`RefMut`], [`RefKeyMut`]) that re-heapify on drop when the key (or the
/// pair) was mutated. All operations have the time complexities documented
/// on the methods below.
///
/// # Examples
///
/// ```rust
/// use deadlock::sync::SlotHeap;
///
/// let heap = SlotHeap::new();
/// let a = heap.insert(3, "three");
/// let b = heap.insert(1, "one");
/// assert_eq!(heap.pop(), Some((1, "one")));
/// heap.remove(a);
/// assert_eq!(heap.len(), 0);
/// ```
pub struct SlotHeap<K, V> {
    inner: RwLock<unsync::SlotHeap<K, V>>,
}

impl<K, V> SlotHeap<K, V>
where
    K: Ord,
{
    /// Creates an empty `SlotHeap`. Time: O(1).
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(unsync::SlotHeap::new()),
        }
    }

    /// Removes all elements and resets internal state. Time: O(n).
    pub fn clear(&self) {
        self.inner.write().clear();
    }

    /// Returns the number of live elements. Time: O(1).
    pub fn len(&self) -> usize {
        self.inner.read().len()
    }

    /// Returns `true` if there are no live elements. Time: O(1).
    pub fn is_empty(&self) -> bool {
        self.inner.read().is_empty()
    }

    /// Returns `true` if `id` refers to a live element. Time: O(1).
    pub fn contains(&self, id: usize) -> bool {
        self.inner.read().contains(id)
    }

    /// Inserts `(key, value)` and returns the new element's stable ID.
    /// Time: O(log n).
    pub fn insert(&self, key: K, value: V) -> usize {
        self.inner.write().insert(key, value)
    }

    /// Removes and returns the minimum-key element, or `None` if the heap is
    /// empty. Time: O(log n).
    pub fn pop(&self) -> Option<(K, V)> {
        self.inner.write().pop()
    }

    /// Removes the element with the given `id` and returns `(key, value)`, or
    /// `None` if `id` is not a live element. Time: O(log n).
    pub fn remove(&self, id: usize) -> Option<(K, V)> {
        self.inner.write().remove(id)
    }

    /// Returns a shared reference to the minimum element's `(key, value)` pair.
    /// Time: O(1).
    pub fn peek(&self) -> Option<MappedRwLockReadGuard<'_, (K, V)>> {
        RwLockReadGuard::try_map(self.inner.read(), |inner| inner.peek()).ok()
    }

    /// Returns a shared reference to the minimum element's key. Time: O(1).
    pub fn peek_key(&self) -> Option<MappedRwLockReadGuard<'_, K>> {
        RwLockReadGuard::try_map(self.inner.read(), |inner| inner.peek_key()).ok()
    }

    /// Returns a shared reference to the minimum element's value. Time: O(1).
    pub fn peek_value(&self) -> Option<MappedRwLockReadGuard<'_, V>> {
        RwLockReadGuard::try_map(self.inner.read(), |inner| inner.peek_value()).ok()
    }

    /// Returns an exclusive guard over the minimum element's `(key, value)` pair.
    /// If the guard is mutated, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn peek_mut(&self) -> Option<PeekMut<'_, K, V>> {
        let guard = self.inner.write();
        util::ensure!(!guard.is_empty());
        Some(PeekMut {
            guard,
            dirty: false,
        })
    }

    /// Returns an exclusive guard over the minimum element's key. If the key
    /// is modified, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn peek_key_mut(&self) -> Option<PeekKeyMut<'_, K, V>> {
        let guard = self.inner.write();
        util::ensure!(!guard.is_empty());
        Some(PeekKeyMut {
            guard,
            dirty: false,
        })
    }

    /// Returns an exclusive reference to the minimum element's value.
    /// Modifying only the value does not affect the heap ordering. Time: O(1).
    pub fn peek_value_mut(&self) -> Option<MappedRwLockWriteGuard<'_, V>> {
        RwLockWriteGuard::try_map(self.inner.write(), |inner| inner.peek_value_mut()).ok()
    }

    /// Returns a shared reference to the `(key, value)` pair of the element
    /// with `id`, or `None` if `id` is not live. Time: O(1).
    pub fn get(&self, id: usize) -> Option<MappedRwLockReadGuard<'_, (K, V)>> {
        RwLockReadGuard::try_map(self.inner.read(), |inner| inner.get(id)).ok()
    }

    /// Returns a shared reference to the key of the element with `id`. Time: O(1).
    pub fn get_key(&self, id: usize) -> Option<MappedRwLockReadGuard<'_, K>> {
        RwLockReadGuard::try_map(self.inner.read(), |inner| inner.get_key(id)).ok()
    }

    /// Returns a shared reference to the value of the element with `id`. Time: O(1).
    pub fn get_value(&self, id: usize) -> Option<MappedRwLockReadGuard<'_, V>> {
        RwLockReadGuard::try_map(self.inner.read(), |inner| inner.get_value(id)).ok()
    }

    /// Returns an exclusive guard over the `(key, value)` pair of the element
    /// with `id`. If the guard is mutated, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn get_mut(&self, id: usize) -> Option<RefMut<'_, K, V>> {
        let guard = self.inner.write();
        let index = guard.get_index(id)?;
        Some(RefMut {
            guard,
            index,
            dirty: false,
        })
    }

    /// Returns an exclusive guard over the key of the element with `id`. If the
    /// key is modified, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn get_key_mut(&self, id: usize) -> Option<RefKeyMut<'_, K, V>> {
        let guard = self.inner.write();
        let index = guard.get_index(id)?;
        Some(RefKeyMut {
            guard,
            index,
            dirty: false,
        })
    }

    /// Returns an exclusive reference to the value of the element with `id`.
    /// Modifying only the value does not affect the heap ordering. Time: O(1).
    pub fn get_value_mut(&self, id: usize) -> Option<MappedRwLockWriteGuard<'_, V>> {
        RwLockWriteGuard::try_map(self.inner.write(), |inner| inner.get_value_mut(id)).ok()
    }
}

#[crabtime::function]
fn def_sync_heap_guard(types: Vec<&str>) {
    for ty in types {
        let (target, deref_ref, deref_mut_ref) = if ty.contains("Key") {
            (
                "K",
                "&self.guard.by_index(idx).0",
                "&mut self.guard.by_index_mut(idx).0",
            )
        } else {
            (
                "(K, V)",
                "self.guard.by_index(idx)",
                "self.guard.by_index_mut(idx)",
            )
        };
        let (index_field, index_expr) = if ty.starts_with("Ref") {
            ("index: usize,", "self.index")
        } else {
            ("", "0")
        };
        let deref_body = deref_ref.replace("idx", index_expr);
        let deref_mut_body = deref_mut_ref.replace("idx", index_expr);

        crabtime::output! {
            pub struct {{ty}}<'a, K, V> where K: Ord {
                guard: RwLockWriteGuard<'a, unsync::SlotHeap<K, V>>,
                {{index_field}}
                dirty: bool,
            }

            impl<K, V> Deref for {{ty}}<'_, K, V> where K: Ord {
                type Target = {{target}};

                fn deref(&self) -> &Self::Target {
                    unsafe { {{deref_body}} }
                }
            }

            impl<K, V> DerefMut for {{ty}}<'_, K, V> where K: Ord {
                fn deref_mut(&mut self) -> &mut Self::Target {
                    self.dirty = true;
                    unsafe { {{deref_mut_body}} }
                }
            }

            impl<K, V> Drop for {{ty}}<'_, K, V> where K: Ord {
                fn drop(&mut self) {
                    if self.dirty {
                        unsafe { self.guard.heapify({{index_expr}}) }
                    }
                }
            }
        }
    }
}

def_sync_heap_guard!(["PeekMut", "PeekKeyMut", "RefMut", "RefKeyMut"]);

impl<K, V> Default for SlotHeap<K, V>
where
    K: Ord,
{
    fn default() -> Self {
        Self::new()
    }
}
