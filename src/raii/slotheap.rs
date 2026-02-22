use parking_lot::{MappedRwLockReadGuard, MappedRwLockWriteGuard};
use std::{
    mem,
    ops::{Deref, DerefMut},
};
use triomphe::Arc;

use crate::sync::{
    slotheap::{
        PeekKeyMut as InnerPeekKeyMut, PeekMut as InnerPeekMut, RefKeyMut as InnerRefKeyMut,
        RefMut as InnerRefMut,
    },
    SlotHeap as Inner,
};

/// Thread-safe min-heap with RAII owning IDs.
///
/// Wraps [`sync::SlotHeap`](crate::sync::SlotHeap) in an [`Arc`]; [`insert`](SlotHeap::insert)
/// returns a [`SlotHeapId`] that owns the element. Dropping the ID without calling
/// [`remove`](SlotHeapId::remove) removes the element from the heap (RAII). All methods
/// take `&self`; concurrency and time complexities match the inner sync heap.
///
/// # Examples
///
/// ```rust
/// use deadlock::raii::SlotHeap;
///
/// let heap = SlotHeap::new();
/// let a = heap.insert(3, "three");
/// let b = heap.insert(1, "one");
/// drop(b);
/// assert_eq!(heap.len(), 1);
/// assert_eq!(*a.get_value(), "three");
/// ```
pub struct SlotHeap<K, V>(Arc<Inner<K, V>>);

/// Owning handle to a single element in a [`SlotHeap`]. Dropping it removes the element.
pub struct SlotHeapId<K, V>
where
    K: PartialOrd,
{
    from: Arc<Inner<K, V>>,
    id: usize,
}

impl<K, V> SlotHeap<K, V>
where
    K: PartialOrd,
{
    /// Creates an empty heap. Time: O(1).
    pub fn new() -> Self {
        Self(Arc::new(Inner::new()))
    }

    /// Returns the number of live elements. Time: O(1).
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns `true` if there are no live elements. Time: O(1).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Inserts `(key, value)` and returns an owning ID. Time: O(log n).
    pub fn insert(&self, key: K, value: V) -> SlotHeapId<K, V> {
        SlotHeapId {
            from: self.0.clone(),
            id: self.0.insert(key, value),
        }
    }

    /// Returns a shared reference to the minimum element's `(key, value)`. Time: O(1).
    pub fn peek(&self) -> Option<MappedRwLockReadGuard<'_, (K, V)>> {
        self.0.peek()
    }

    /// Returns a shared reference to the minimum element without checking non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked(&self) -> MappedRwLockReadGuard<'_, (K, V)> {
        unsafe { self.0.peek_unchecked() }
    }

    /// Returns a shared reference to the minimum element's key. Time: O(1).
    pub fn peek_key(&self) -> Option<MappedRwLockReadGuard<'_, K>> {
        self.0.peek_key()
    }

    /// Returns a shared reference to the minimum key without checking non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_key(&self) -> MappedRwLockReadGuard<'_, K> {
        unsafe { self.0.peek_unchecked_key() }
    }

    /// Returns a shared reference to the minimum element's value. Time: O(1).
    pub fn peek_value(&self) -> Option<MappedRwLockReadGuard<'_, V>> {
        self.0.peek_value()
    }

    /// Returns a shared reference to the minimum value without checking non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_value(&self) -> MappedRwLockReadGuard<'_, V> {
        unsafe { self.0.peek_unchecked_value() }
    }

    /// Exclusive guard over the minimum `(key, value)`; re-heapifies on drop if mutated. Time: O(1).
    pub fn peek_mut(&self) -> Option<PeekMut<'_, K, V>> {
        self.0.peek_mut().map(PeekMut)
    }

    /// Exclusive guard over the minimum element without checking non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_mut(&self) -> PeekMut<'_, K, V> {
        PeekMut(unsafe { self.0.peek_unchecked_mut() })
    }

    /// Exclusive guard over the minimum key; re-heapifies on drop if mutated. Time: O(1).
    pub fn peek_key_mut(&self) -> Option<PeekKeyMut<'_, K, V>> {
        self.0.peek_key_mut().map(PeekKeyMut)
    }

    /// Exclusive guard over the minimum key without checking non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_key_mut(&self) -> PeekKeyMut<'_, K, V> {
        PeekKeyMut(unsafe { self.0.peek_unchecked_key_mut() })
    }

    /// Exclusive reference to the minimum value; value-only mutation does not affect order. Time: O(1).
    pub fn peek_value_mut(&self) -> Option<MappedRwLockWriteGuard<'_, V>> {
        self.0.peek_value_mut()
    }

    /// Exclusive reference to the minimum value without checking non-empty. Time: O(1).
    ///
    /// # Safety
    ///
    /// The heap must not be empty.
    pub unsafe fn peek_unchecked_value_mut(&self) -> MappedRwLockWriteGuard<'_, V> {
        unsafe { self.0.peek_unchecked_value_mut() }
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

impl<K, V> SlotHeapId<K, V>
where
    K: PartialOrd,
{
    /// Removes the element from the heap and returns `(key, value)`. Consumes the ID without running drop. Time: O(log n).
    pub fn remove(self) -> (K, V) {
        let item = unsafe { self.from.remove_unchecked(self.id) };
        mem::forget(self);
        item
    }

    /// Returns a shared reference to the element's `(key, value)`. Time: O(1).
    pub fn get(&self) -> MappedRwLockReadGuard<'_, (K, V)> {
        unsafe { self.from.get_unchecked(self.id) }
    }

    /// Returns a shared reference to the element's key. Time: O(1).
    pub fn get_key(&self) -> MappedRwLockReadGuard<'_, K> {
        unsafe { self.from.get_unchecked_key(self.id) }
    }

    /// Returns a shared reference to the element's value. Time: O(1).
    pub fn get_value(&self) -> MappedRwLockReadGuard<'_, V> {
        unsafe { self.from.get_unchecked_value(self.id) }
    }

    /// Exclusive guard over the element's `(key, value)`; re-heapifies on drop if mutated. Time: O(1).
    pub fn get_mut(&self) -> RefMut<'_, K, V> {
        RefMut(unsafe { self.from.get_unchecked_mut(self.id) })
    }

    /// Exclusive guard over the element's key; re-heapifies on drop if mutated. Time: O(1).
    pub fn get_key_mut(&self) -> RefKeyMut<'_, K, V> {
        RefKeyMut(unsafe { self.from.get_unchecked_key_mut(self.id) })
    }

    /// Exclusive reference to the element's value; value-only mutation does not affect order. Time: O(1).
    pub fn get_value_mut(&self) -> MappedRwLockWriteGuard<'_, V> {
        unsafe { self.from.get_unchecked_value_mut(self.id) }
    }
}

impl<K, V> Drop for SlotHeapId<K, V>
where
    K: PartialOrd,
{
    fn drop(&mut self) {
        unsafe { self.from.remove_unchecked(self.id) };
    }
}

/// Exclusive guard over the minimum element's `(key, value)`; re-heapifies on drop when mutated.
pub struct PeekMut<'a, K, V>(InnerPeekMut<'a, K, V>)
where
    K: PartialOrd;

impl<K, V> Deref for PeekMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = (K, V);

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<K, V> DerefMut for PeekMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// Exclusive guard over the minimum element's key; re-heapifies on drop when mutated.
pub struct PeekKeyMut<'a, K, V>(InnerPeekKeyMut<'a, K, V>)
where
    K: PartialOrd;

impl<K, V> Deref for PeekKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = K;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<K, V> DerefMut for PeekKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// Exclusive guard over an element's `(key, value)` by ID; re-heapifies on drop when mutated.
pub struct RefMut<'a, K, V>(InnerRefMut<'a, K, V>)
where
    K: PartialOrd;

impl<K, V> Deref for RefMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = (K, V);

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<K, V> DerefMut for RefMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}

/// Exclusive guard over an element's key by ID; re-heapifies on drop when mutated.
pub struct RefKeyMut<'a, K, V>(InnerRefKeyMut<'a, K, V>)
where
    K: PartialOrd;

impl<K, V> Deref for RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    type Target = K;

    fn deref(&self) -> &Self::Target {
        self.0.deref()
    }
}

impl<K, V> DerefMut for RefKeyMut<'_, K, V>
where
    K: PartialOrd,
{
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.deref_mut()
    }
}
