# deadlock
Stable-ID slot map and slot heap for Rust - both single-threaded (`unsync`) and thread-safe (`sync`) variants.

| Type | Description |
|---|---|
| `unsync::SlotMap<T>` | Single-threaded slot map. O(1) insert / remove / lookup by stable ID. |
| `unsync::SlotHeap<K, V>` | Single-threaded min-heap. O(log n) insert / pop / remove / heapify by stable ID. |
| `sync::SlotMap<T>` | Thread-safe slot map. Concurrent inserts, removes, and reads on independent shards. |
| `sync::SlotHeap<K, V>` | Thread-safe min-heap. Mutations are serialized via an internal lock. |
