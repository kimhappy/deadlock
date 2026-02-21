use std::{
    cmp::Ordering,
    ops::{Deref, DerefMut},
};

use crate::{unsync::SlotMap, util};

/// Single-threaded min-heap with stable, reusable IDs.
///
/// Elements are ordered by key `K` via [`PartialOrd`]; each inserted `(key, value)`
/// receives a stable `usize` ID that remains valid until the element is
/// removed. Min access, key/value updates by ID, and heap repair after key
/// changes are provided via guard types ([`PeekMut`], [`PeekKeyMut`],
/// [`RefMut`], [`RefKeyMut`]) that re-heapify on drop when the key (or the
/// pair) was mutated. All operations have the time complexities documented
/// on the methods below.
///
/// # Examples
///
/// ```rust
/// use deadlock::unsync::SlotHeap;
///
/// let mut heap = SlotHeap::new();
/// let a = heap.insert(3, "three");
/// let b = heap.insert(1, "one");
/// let c = heap.insert(2, "two");
///
/// assert_eq!(heap.pop(), Some((1, "one")));
/// assert_eq!(heap.pop(), Some((2, "two")));
///
/// heap.remove(a);
/// assert_eq!(heap.len(), 0);
/// ```
pub struct SlotHeap<K, V> {
    entries: Vec<Entry<K, V>>,
    indices: SlotMap<usize>,
}

struct Entry<K, V> {
    item: (K, V),
    id: usize,
}

impl<K, V> SlotHeap<K, V>
where
    K: PartialOrd,
{
    /// Creates an empty `SlotHeap`. Time: O(1).
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            indices: SlotMap::new(),
        }
    }

    /// Removes all elements and resets internal state. Time: O(n).
    pub fn clear(&mut self) {
        self.entries.clear();
        self.indices.clear()
    }

    /// Returns the number of live elements. Time: O(1).
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if there are no live elements. Time: O(1).
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns `true` if `id` refers to a live element. Time: O(1).
    pub fn contains(&self, id: usize) -> bool {
        self.indices.contains(id)
    }

    /// Inserts `(key, value)` and returns the new element's stable ID.
    /// Time: O(log n).
    pub fn insert(&mut self, key: K, value: V) -> usize {
        let id = self.indices.insert(self.entries.len());
        self.entries.push(Entry {
            item: (key, value),
            id,
        });
        unsafe { self.heapify_up(self.entries.len() - 1) };
        id
    }

    /// Removes and returns the minimum-key element, or `None` if the heap is
    /// empty. Time: O(log n).
    pub fn pop(&mut self) -> Option<(K, V)> {
        util::ensure!(!self.entries.is_empty());

        if self.entries.len() == 1 {
            self.indices.clear();
            return Some(unsafe { self.entries.pop().unwrap_unchecked().item });
        }

        let item = unsafe {
            let entry = util::swap_remove_unchecked(&mut self.entries, 0);
            self.indices.remove_unchecked(entry.id);
            *self
                .indices
                .get_unchecked_mut(self.entries.get_unchecked(0).id) = 0;
            entry.item
        };

        unsafe { self.heapify_down(0) }
        Some(item)
    }

    /// Removes the element with the given `id` and returns `(key, value)`, or
    /// `None` if `id` is not a live element. Time: O(log n).
    pub fn remove(&mut self, id: usize) -> Option<(K, V)> {
        let index = self.indices.remove(id)?;
        let len = self.entries.len();

        if index == len - 1 {
            return Some(unsafe { self.entries.pop().unwrap_unchecked().item });
        }

        let item = unsafe {
            let entry = util::swap_remove_unchecked(&mut self.entries, index);
            *self
                .indices
                .get_unchecked_mut(self.entries.get_unchecked(index).id) = index;
            entry.item
        };

        unsafe { self.heapify(index) }
        Some(item)
    }

    /// Returns a shared reference to the minimum element's `(key, value)` pair.
    /// Time: O(1).
    pub fn peek(&self) -> Option<&(K, V)> {
        self.entries.first().map(|item| &item.item)
    }

    /// Returns a shared reference to the minimum element's key. Time: O(1).
    pub fn peek_key(&self) -> Option<&K> {
        self.entries.first().map(|item| &item.item.0)
    }

    /// Returns a shared reference to the minimum element's value. Time: O(1).
    pub fn peek_value(&self) -> Option<&V> {
        self.entries.first().map(|item| &item.item.1)
    }

    /// Returns an exclusive guard over the minimum element's `(key, value)` pair.
    /// If the guard is mutated, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn peek_mut(&mut self) -> Option<PeekMut<'_, K, V>> {
        util::ensure!(!self.entries.is_empty());
        Some(PeekMut {
            dirty: false,
            from: self,
        })
    }

    /// Returns an exclusive guard over the minimum element's key. If the key
    /// is modified, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn peek_key_mut(&mut self) -> Option<PeekKeyMut<'_, K, V>> {
        util::ensure!(!self.entries.is_empty());
        Some(PeekKeyMut {
            dirty: false,
            from: self,
        })
    }

    /// Returns an exclusive reference to the minimum element's value.
    /// Modifying only the value does not affect the heap ordering. Time: O(1).
    pub fn peek_value_mut(&mut self) -> Option<&mut V> {
        self.entries.first_mut().map(|entry| &mut entry.item.1)
    }

    /// Returns a shared reference to the `(key, value)` pair of the element
    /// with `id`, or `None` if `id` is not live. Time: O(1).
    pub fn get(&self, id: usize) -> Option<&(K, V)> {
        let index = *self.indices.get(id)?;
        Some(unsafe { &self.entries.get_unchecked(index).item })
    }

    /// Returns a shared reference to the key of the element with `id`. Time: O(1).
    pub fn get_key(&self, id: usize) -> Option<&K> {
        let index = *self.indices.get(id)?;
        Some(unsafe { &self.entries.get_unchecked(index).item.0 })
    }

    /// Returns a shared reference to the value of the element with `id`. Time: O(1).
    pub fn get_value(&self, id: usize) -> Option<&V> {
        let index = *self.indices.get(id)?;
        Some(unsafe { &self.entries.get_unchecked(index).item.1 })
    }

    /// Returns an exclusive guard over the `(key, value)` pair of the element
    /// with `id`. If the guard is mutated, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn get_mut(&mut self, id: usize) -> Option<RefMut<'_, K, V>> {
        let index = *self.indices.get(id)?;
        Some(RefMut {
            dirty: false,
            from: self,
            index,
        })
    }

    /// Returns an exclusive guard over the key of the element with `id`. If the
    /// key is modified, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn get_key_mut(&mut self, id: usize) -> Option<RefKeyMut<'_, K, V>> {
        let index = *self.indices.get(id)?;
        Some(RefKeyMut {
            dirty: false,
            from: self,
            index,
        })
    }

    /// Returns an exclusive reference to the value of the element with `id`.
    /// Modifying only the value does not affect the heap ordering. Time: O(1).
    pub fn get_value_mut(&mut self, id: usize) -> Option<&mut V> {
        let index = *self.indices.get(id)?;
        Some(unsafe { &mut self.entries.get_unchecked_mut(index).item.1 })
    }

    pub(crate) unsafe fn heapify(&mut self, now: usize) {
        let now = unsafe { self.heapify_up(now) };
        unsafe { self.heapify_down(now) }
    }

    unsafe fn heapify_up(&mut self, mut now: usize) -> usize {
        unsafe {
            while let Some(up) = self.next_up(now) {
                self.swap_entries(now, up);
                now = up
            }
        }

        now
    }

    pub(crate) unsafe fn heapify_down(&mut self, mut now: usize) {
        unsafe {
            while let Some(down) = self.next_down(now) {
                self.swap_entries(now, down);
                now = down
            }
        }
    }

    unsafe fn next_up(&self, now: usize) -> Option<usize> {
        now.checked_sub(1).map(|x| x / 2).filter(|up| unsafe {
            self.entries.get_unchecked(now) < self.entries.get_unchecked(*up)
        })
    }

    unsafe fn next_down(&self, now: usize) -> Option<usize> {
        let now_item = unsafe { self.entries.get_unchecked(now) };
        let (left, right) = (now * 2 + 1, now * 2 + 2);

        if right < self.entries.len() {
            let left_item = unsafe { self.entries.get_unchecked(left) };
            let right_item = unsafe { self.entries.get_unchecked(right) };

            if left_item < right_item {
                (left_item < now_item).then_some(left)
            } else {
                (right_item < now_item).then_some(right)
            }
        } else {
            let left_item = self.entries.get(left)?;
            (left_item < now_item).then_some(left)
        }
    }

    unsafe fn swap_entries(&mut self, index0: usize, index1: usize) {
        unsafe {
            util::swap_unchecked(&mut self.entries, index0, index1);
            let id0 = self.entries.get_unchecked(index0).id;
            let id1 = self.entries.get_unchecked(index1).id;
            *self.indices.get_unchecked_mut(id0) = index0;
            *self.indices.get_unchecked_mut(id1) = index1
        }
    }

    pub(crate) fn get_index(&self, id: usize) -> Option<usize> {
        self.indices.get(id).copied()
    }

    pub(crate) unsafe fn by_index(&self, index: usize) -> &(K, V) {
        unsafe { &self.entries.get_unchecked(index).item }
    }

    pub(crate) unsafe fn by_index_mut(&mut self, index: usize) -> &mut (K, V) {
        unsafe { &mut self.entries.get_unchecked_mut(index).item }
    }
}

impl<K, V> Default for SlotHeap<K, V>
where
    K: PartialOrd,
{
    fn default() -> Self {
        Self::new()
    }
}

impl<K, V> PartialEq for Entry<K, V>
where
    K: PartialOrd,
{
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl<K, V> Eq for Entry<K, V> where K: PartialOrd {}

impl<K, V> PartialOrd for Entry<K, V>
where
    K: PartialOrd,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl<K, V> Ord for Entry<K, V>
where
    K: PartialOrd,
{
    fn cmp(&self, other: &Self) -> Ordering {
        let id_cmp = || self.id.cmp(&other.id);
        self.item
            .0
            .partial_cmp(&other.item.0)
            .map_or_else(id_cmp, |ordering| ordering.then_with(id_cmp))
    }
}

/// Exclusive guard over the minimum element's `(key, value)` pair. Mutating
/// the guard marks the heap as dirty; on drop the heap is re-heapified.
pub struct PeekMut<'a, K, V>
where
    K: PartialOrd,
{
    dirty: bool,
    from: &'a mut SlotHeap<K, V>,
}

impl<'a, K, V> Deref for PeekMut<'a, K, V>
where
    K: PartialOrd,
{
    type Target = (K, V);

    fn deref(&self) -> &Self::Target {
        unsafe { &self.from.entries.get_unchecked(0).item }
    }
}

impl<'a, K, V> DerefMut for PeekMut<'a, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { &mut self.from.entries.get_unchecked_mut(0).item }
    }
}

impl<'a, K, V> AsRef<(K, V)> for PeekMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &(K, V) {
        self.deref()
    }
}

impl<'a, K, V> AsMut<(K, V)> for PeekMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut (K, V) {
        self.deref_mut()
    }
}

impl<'a, K, V> Drop for PeekMut<'a, K, V>
where
    K: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            unsafe { self.from.heapify_down(0) }
        }
    }
}

/// Exclusive guard over the minimum element's key. Mutating the guard marks
/// the heap as dirty; on drop the heap is re-heapified.
pub struct PeekKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    dirty: bool,
    from: &'a mut SlotHeap<K, V>,
}

impl<'a, K, V> Deref for PeekKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    type Target = K;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.from.entries.get_unchecked(0).item.0 }
    }
}

impl<'a, K, V> DerefMut for PeekKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { &mut self.from.entries.get_unchecked_mut(0).item.0 }
    }
}

impl<'a, K, V> AsRef<K> for PeekKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &K {
        self.deref()
    }
}

impl<'a, K, V> AsMut<K> for PeekKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut K {
        self.deref_mut()
    }
}

impl<'a, K, V> Drop for PeekKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            unsafe { self.from.heapify_down(0) }
        }
    }
}

/// Exclusive guard over the `(key, value)` pair of an element identified by
/// index. Mutating the guard marks the heap as dirty; on drop the heap is
/// re-heapified.
pub struct RefMut<'a, K, V>
where
    K: PartialOrd,
{
    dirty: bool,
    from: &'a mut SlotHeap<K, V>,
    index: usize,
}

impl<'a, K, V> Deref for RefMut<'a, K, V>
where
    K: PartialOrd,
{
    type Target = (K, V);

    fn deref(&self) -> &Self::Target {
        unsafe { &self.from.entries.get_unchecked(self.index).item }
    }
}

impl<'a, K, V> DerefMut for RefMut<'a, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { &mut self.from.entries.get_unchecked_mut(self.index).item }
    }
}

impl<'a, K, V> AsRef<(K, V)> for RefMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &(K, V) {
        self.deref()
    }
}

impl<'a, K, V> AsMut<(K, V)> for RefMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut (K, V) {
        self.deref_mut()
    }
}

impl<'a, K, V> Drop for RefMut<'a, K, V>
where
    K: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            unsafe { self.from.heapify(self.index) }
        }
    }
}

/// Exclusive guard over the key of an element identified by index. Mutating
/// the guard marks the heap as dirty; on drop the heap is re-heapified.
pub struct RefKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    dirty: bool,
    from: &'a mut SlotHeap<K, V>,
    index: usize,
}

impl<'a, K, V> Deref for RefKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    type Target = K;

    fn deref(&self) -> &Self::Target {
        unsafe { &self.from.entries.get_unchecked(self.index).item.0 }
    }
}

impl<'a, K, V> DerefMut for RefKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { &mut self.from.entries.get_unchecked_mut(self.index).item.0 }
    }
}

impl<'a, K, V> AsRef<K> for RefKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &K {
        self.deref()
    }
}

impl<'a, K, V> AsMut<K> for RefKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut K {
        self.deref_mut()
    }
}

impl<'a, K, V> Drop for RefKeyMut<'a, K, V>
where
    K: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            unsafe { self.from.heapify(self.index) }
        }
    }
}
