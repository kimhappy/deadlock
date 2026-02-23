# deadlock
Thread-safe **slot map** and **slot min-heap** with stable RAII handle. Values are stored by handle; dropping the handle removes the value. The map is sharded to reduce contention; the heap supports peek and stable references into elements.

[![crates.io](https://img.shields.io/crates/v/deadlock?style=flat-square)](https://crates.io/crates/deadlock)
[![docs.rs](https://img.shields.io/docsrs/deadlock?style=flat-square)](https://docs.rs/deadlock/latest/deadlock)
[![License](https://img.shields.io/github/license/kimhappy/deadlock?style=flat-square)](https://github.com/kimhappy/deadlock/blob/main/LICENSE)

## Example
```rust
use deadlock::{SlotMap, SlotHeap};

let map = SlotMap::new();
let id = map.insert(42);
assert_eq!(*id.get(), 42);

let heap = SlotHeap::new();
let (id1, _) = heap.insert(3);
let (id2, _) = heap.insert(1);
let (id3, _) = heap.insert(2);
assert_eq!(*heap.peek().unwrap(), 1);
assert_eq!(*id3.get(), 2);
```
