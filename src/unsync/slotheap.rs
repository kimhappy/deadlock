use std::{
    cmp::Ordering,
    mem,
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
        let entry = Entry {
            item: (key, value),
            id,
        };
        self.entries.push(entry);
        unsafe { self.heapify_up(self.entries.len() - 1) };
        id
    }

    /// Removes and returns the minimum-key element, or `None` if the heap is
    /// empty. Time: O(log n).
    pub fn pop(&mut self) -> Option<(K, V)> {
        (!self.entries.is_empty()).then(|| unsafe { self.pop_unchecked() })
    }

    /// Removes and returns the minimum-key element without checking that the
    /// heap is non-empty. Time: O(log n).
    ///
    /// # Safety
    ///
    /// The heap must not be empty. Calling this on an empty heap is undefined
    /// behavior.
    pub unsafe fn pop_unchecked(&mut self) -> (K, V) {
        if self.entries.len() == 1 {
            self.indices.clear();
            return unsafe { self.entries.pop().unwrap_unchecked().item };
        }

        unsafe {
            let entry = util::swap_remove_unchecked(&mut self.entries, 0);
            self.indices.remove_unchecked(entry.id);
            let id = self.entries.get_unchecked(0).id;
            *self.indices.get_unchecked_mut(id) = 0;
            self.heapify_down(0);
            entry.item
        }
    }

    /// Removes the element with the given `id` and returns `(key, value)`, or
    /// `None` if `id` is not a live element. Time: O(log n).
    pub fn remove(&mut self, id: usize) -> Option<(K, V)> {
        self.contains(id)
            .then(|| unsafe { self.remove_unchecked(id) })
    }

    /// Removes the element with the given `id` and returns `(key, value)`
    /// without checking that `id` is live. Time: O(log n).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live element. Using an invalid or reused `id` is
    /// undefined behavior.
    pub unsafe fn remove_unchecked(&mut self, id: usize) -> (K, V) {
        let index = unsafe { self.indices.remove_unchecked(id) };
        let len = self.entries.len();

        if index == len - 1 {
            return unsafe { self.entries.pop().unwrap_unchecked().item };
        }

        unsafe {
            let entry = util::swap_remove_unchecked(&mut self.entries, index);
            let id = self.entries.get_unchecked(index).id;
            *self.indices.get_unchecked_mut(id) = index;
            self.heapify(index);
            entry.item
        }
    }

    /// Returns a shared reference to the minimum element's `(key, value)` pair.
    /// Time: O(1).
    pub fn peek(&self) -> Option<&(K, V)> {
        self.entries.first().map(|item| &item.item)
    }

    /// Returns a shared reference to the minimum element's `(key, value)` pair
    /// without checking that the heap is non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked(&self) -> &(K, V) {
        unsafe { &self.entries.get_unchecked(0).item }
    }

    /// Returns a shared reference to the minimum element's key. Time: O(1).
    pub fn peek_key(&self) -> Option<&K> {
        self.entries.first().map(|item| &item.item.0)
    }

    /// Returns a shared reference to the minimum element's key without
    /// checking that the heap is non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_key(&self) -> &K {
        unsafe { &self.entries.get_unchecked(0).item.0 }
    }

    /// Returns a shared reference to the minimum element's value. Time: O(1).
    pub fn peek_value(&self) -> Option<&V> {
        self.entries.first().map(|item| &item.item.1)
    }

    /// Returns a shared reference to the minimum element's value without
    /// checking that the heap is non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_value(&self) -> &V {
        unsafe { &self.entries.get_unchecked(0).item.1 }
    }

    /// Returns an exclusive guard over the minimum element's `(key, value)` pair.
    /// If the guard is mutated, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn peek_mut(&mut self) -> Option<PeekMut<'_, K, V>> {
        (!self.entries.is_empty()).then(move || unsafe { self.peek_unchecked_mut() })
    }

    /// Returns an exclusive guard over the minimum element's `(key, value)` pair
    /// without checking that the heap is non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_mut(&mut self) -> PeekMut<'_, K, V> {
        PeekMut {
            dirty: false,
            from: self,
        }
    }

    /// Returns an exclusive guard over the minimum element's key. If the key
    /// is modified, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn peek_key_mut(&mut self) -> Option<PeekKeyMut<'_, K, V>> {
        (!self.entries.is_empty()).then(move || unsafe { self.peek_unchecked_key_mut() })
    }

    /// Returns an exclusive guard over the minimum element's key without
    /// checking that the heap is non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_key_mut(&mut self) -> PeekKeyMut<'_, K, V> {
        PeekKeyMut {
            dirty: false,
            from: self,
        }
    }

    /// Returns an exclusive reference to the minimum element's value.
    /// Modifying only the value does not affect the heap ordering. Time: O(1).
    pub fn peek_value_mut(&mut self) -> Option<&mut V> {
        (!self.entries.is_empty()).then(move || unsafe { self.peek_unchecked_value_mut() })
    }

    /// Returns an exclusive reference to the minimum element's value without
    /// checking that the heap is non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_value_mut(&mut self) -> &mut V {
        unsafe { &mut self.peek_mut_impl().1 }
    }

    /// Returns a shared reference to the `(key, value)` pair of the element
    /// with `id`, or `None` if `id` is not live. Time: O(1).
    pub fn get(&self, id: usize) -> Option<&(K, V)> {
        let index = self.get_index(id)?;
        Some(unsafe { &self.entries.get_unchecked(index).item })
    }

    /// Returns a shared reference to the `(key, value)` pair of the element
    /// with `id` without checking that `id` is live. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live element.
    pub unsafe fn get_unchecked(&self, id: usize) -> &(K, V) {
        unsafe {
            let index = self.get_unchecked_index(id);
            &self.entries.get_unchecked(index).item
        }
    }

    /// Returns a shared reference to the key of the element with `id`. Time: O(1).
    pub fn get_key(&self, id: usize) -> Option<&K> {
        let index = self.get_index(id)?;
        Some(unsafe { &self.entries.get_unchecked(index).item.0 })
    }

    /// Returns a shared reference to the key of the element with `id` without
    /// checking that `id` is live. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live element.
    pub unsafe fn get_unchecked_key(&self, id: usize) -> &K {
        unsafe {
            let index = self.get_unchecked_index(id);
            &self.entries.get_unchecked(index).item.0
        }
    }

    /// Returns a shared reference to the value of the element with `id`. Time: O(1).
    pub fn get_value(&self, id: usize) -> Option<&V> {
        let index = self.get_index(id)?;
        Some(unsafe { &self.entries.get_unchecked(index).item.1 })
    }

    /// Returns a shared reference to the value of the element with `id` without
    /// checking that `id` is live. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live element.
    pub unsafe fn get_unchecked_value(&self, id: usize) -> &V {
        unsafe {
            let index = self.get_unchecked_index(id);
            &self.entries.get_unchecked(index).item.1
        }
    }

    /// Returns an exclusive guard over the `(key, value)` pair of the element
    /// with `id`. If the guard is mutated, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn get_mut(&mut self, id: usize) -> Option<RefMut<'_, K, V>> {
        self.contains(id)
            .then(move || unsafe { self.get_unchecked_mut(id) })
    }

    /// Returns an exclusive guard over the `(key, value)` pair of the element
    /// with `id` without checking that `id` is live. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live element.
    pub unsafe fn get_unchecked_mut(&mut self, id: usize) -> RefMut<'_, K, V> {
        RefMut {
            dirty: false,
            from: self,
            id,
        }
    }

    /// Returns an exclusive guard over the key of the element with `id`. If the
    /// key is modified, the heap invariant is restored on drop.
    /// Time: O(1) to create; drop may run O(log n) heapify.
    pub fn get_key_mut(&mut self, id: usize) -> Option<RefKeyMut<'_, K, V>> {
        self.contains(id)
            .then(move || unsafe { self.get_unchecked_key_mut(id) })
    }

    /// Returns an exclusive guard over the key of the element with `id` without
    /// checking that `id` is live. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live element.
    pub unsafe fn get_unchecked_key_mut(&mut self, id: usize) -> RefKeyMut<'_, K, V> {
        RefKeyMut {
            dirty: false,
            from: self,
            id,
        }
    }

    /// Returns an exclusive reference to the value of the element with `id`.
    /// Modifying only the value does not affect the heap ordering. Time: O(1).
    pub fn get_value_mut(&mut self, id: usize) -> Option<&mut V> {
        let index = self.get_index(id)?;
        Some(unsafe { &mut self.entries.get_unchecked_mut(index).item.1 })
    }

    /// Returns an exclusive reference to the value of the element with `id`
    /// without checking that `id` is live. Time: O(1).
    ///
    /// # Safety
    ///
    /// `id` must refer to a live element.
    pub unsafe fn get_unchecked_value_mut(&mut self, id: usize) -> &mut V {
        unsafe {
            let index = self.get_unchecked_index(id);
            &mut self.entries.get_unchecked_mut(index).item.1
        }
    }

    pub(crate) unsafe fn heapify(&mut self, now: usize) {
        unsafe {
            let now = self.heapify_up(now);
            self.heapify_down(now)
        }
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

    pub(crate) unsafe fn peek_mut_impl(&mut self) -> &mut (K, V) {
        unsafe { &mut self.entries.get_unchecked_mut(0).item }
    }

    pub(crate) unsafe fn get_mut_impl(&mut self, id: usize) -> &mut (K, V) {
        unsafe {
            let index = self.get_unchecked_index(id);
            &mut self.entries.get_unchecked_mut(index).item
        }
    }

    pub(crate) fn get_index(&self, id: usize) -> Option<usize> {
        self.indices.get(id).copied()
    }

    pub(crate) unsafe fn get_unchecked_index(&self, id: usize) -> usize {
        unsafe { *self.indices.get_unchecked(id) }
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

impl<K, V> PeekMut<'_, K, V>
where
    K: PartialOrd,
{
    /// Removes the minimum element from the heap and returns its `(key, value)`.
    /// Consumes the guard without running drop. Time: O(log n).
    pub fn remove(self) -> (K, V) {
        let item = unsafe { self.from.pop_unchecked() };
        mem::forget(self);
        item
    }
}

impl<K, V> Deref for PeekMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = (K, V);

    fn deref(&self) -> &Self::Target {
        unsafe { self.from.peek_unchecked() }
    }
}

impl<K, V> DerefMut for PeekMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { self.from.peek_mut_impl() }
    }
}

impl<K, V> AsRef<(K, V)> for PeekMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &(K, V) {
        self.deref()
    }
}

impl<K, V> AsMut<(K, V)> for PeekMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut (K, V) {
        self.deref_mut()
    }
}

impl<K, V> Drop for PeekMut<'_, K, V>
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

impl<K, V> PeekKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    /// Removes the minimum element from the heap and returns its key.
    /// Consumes the guard without running drop. Time: O(log n).
    pub fn remove(self) -> K {
        let (key, _) = unsafe { self.from.pop_unchecked() };
        mem::forget(self);
        key
    }
}

impl<K, V> Deref for PeekKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = K;

    fn deref(&self) -> &Self::Target {
        unsafe { self.from.peek_unchecked_key() }
    }
}

impl<K, V> DerefMut for PeekKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { &mut self.from.peek_mut_impl().0 }
    }
}

impl<K, V> AsRef<K> for PeekKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &K {
        self.deref()
    }
}

impl<K, V> AsMut<K> for PeekKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut K {
        self.deref_mut()
    }
}

impl<K, V> Drop for PeekKeyMut<'_, K, V>
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
    id: usize,
}

impl<K, V> RefMut<'_, K, V>
where
    K: PartialOrd,
{
    /// Removes the element identified by this guard from the heap and returns
    /// its `(key, value)`. Consumes the guard without running drop. Time: O(log n).
    pub fn remove(self) -> (K, V) {
        let item = unsafe { self.from.remove_unchecked(self.id) };
        mem::forget(self);
        item
    }
}

impl<K, V> Deref for RefMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = (K, V);

    fn deref(&self) -> &Self::Target {
        unsafe { self.from.get_unchecked(self.id) }
    }
}

impl<K, V> DerefMut for RefMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { self.from.get_mut_impl(self.id) }
    }
}

impl<K, V> AsRef<(K, V)> for RefMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &(K, V) {
        self.deref()
    }
}

impl<K, V> AsMut<(K, V)> for RefMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut (K, V) {
        self.deref_mut()
    }
}

impl<K, V> Drop for RefMut<'_, K, V>
where
    K: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            unsafe {
                let index = self.from.get_unchecked_index(self.id);
                self.from.heapify(index)
            }
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
    id: usize,
}

impl<K, V> RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    /// Removes the element identified by this guard from the heap and returns
    /// its key. Consumes the guard without running drop. Time: O(log n).
    pub fn remove(self) -> K {
        let (key, _) = unsafe { self.from.remove_unchecked(self.id) };
        mem::forget(self);
        key
    }
}

impl<K, V> Deref for RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = K;

    fn deref(&self) -> &Self::Target {
        unsafe { self.from.get_unchecked_key(self.id) }
    }
}

impl<K, V> DerefMut for RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.dirty = true;
        unsafe { &mut self.from.get_mut_impl(self.id).0 }
    }
}

impl<K, V> AsRef<K> for RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_ref(&self) -> &K {
        self.deref()
    }
}

impl<K, V> AsMut<K> for RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn as_mut(&mut self) -> &mut K {
        self.deref_mut()
    }
}

impl<K, V> Drop for RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn drop(&mut self) {
        if self.dirty {
            unsafe {
                let index = self.from.get_unchecked_index(self.id);
                self.from.heapify(index)
            }
        }
    }
}
