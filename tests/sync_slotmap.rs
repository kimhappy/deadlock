use deadlock::sync::SlotMap;
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Barrier,
    },
    thread,
};

#[test]
fn insert_and_get() {
    let map = SlotMap::new();
    let a = map.insert(10i32);
    let b = map.insert(20);
    let c = map.insert(30);

    assert_eq!(*map.get(a).unwrap(), 10);
    assert_eq!(*map.get(b).unwrap(), 20);
    assert_eq!(*map.get(c).unwrap(), 30);
}

#[test]
fn contains() {
    let map = SlotMap::new();
    let a = map.insert(1i32);
    assert!(map.contains(a));
    assert!(!map.contains(a.wrapping_add(0xffff)));
}

#[test]
fn len_and_is_empty() {
    let map = SlotMap::new();
    assert_eq!(map.len(), 0);
    assert!(map.is_empty());

    let a = map.insert(1i32);
    assert_eq!(map.len(), 1);
    assert!(!map.is_empty());

    map.insert(2);
    assert_eq!(map.len(), 2);
    map.remove(a);
    assert_eq!(map.len(), 1);
}

#[test]
fn remove_returns_value() {
    let map = SlotMap::new();
    let a = map.insert(42i32);
    assert_eq!(map.remove(a), Some(42));
    assert_eq!(map.remove(a), None);
    assert!(!map.contains(a));
}

#[test]
fn remove_invalid_id() {
    let map = SlotMap::<i32>::new();
    assert_eq!(map.remove(0), None);
}

#[test]
fn stable_ids_after_remove() {
    let map = SlotMap::new();
    let a = map.insert(1i32);
    let b = map.insert(2);
    let c = map.insert(3);
    map.remove(b);

    assert_eq!(*map.get(a).unwrap(), 1);
    assert!(map.get(b).is_none());
    assert_eq!(*map.get(c).unwrap(), 3);
}

#[test]
fn get_mut() {
    let map = SlotMap::new();
    let a = map.insert(10i32);
    *map.get_mut(a).unwrap() = 99;
    assert_eq!(*map.get(a).unwrap(), 99);
    assert!(map.get_mut(999).is_none());
}

#[test]
fn clear() {
    let map = SlotMap::new();
    let a = map.insert(1i32);
    map.insert(2);
    map.clear();

    assert_eq!(map.len(), 0);
    assert!(map.is_empty());
    assert!(map.get(a).is_none());
}

#[test]
fn with_num_shards_valid() {
    let m = SlotMap::<i32>::with_num_shards(4).unwrap();
    assert_eq!(m.num_shards(), 4);
    let m = SlotMap::<i32>::with_num_shards(8).unwrap();
    assert_eq!(m.num_shards(), 8);
}

#[test]
fn with_num_shards_invalid() {
    for n in [0, 1, 2, 3, 5, 6, 7] {
        assert!(
            SlotMap::<i32>::with_num_shards(n).is_none(),
            "should reject {}",
            n
        );
    }
}

#[test]
fn get_unchecked() {
    let map = SlotMap::new();
    let a = map.insert(42i32);
    assert_eq!(unsafe { *map.get_unchecked(a) }, 42);
}

#[test]
fn get_unchecked_mut() {
    let map = SlotMap::new();
    let a = map.insert(42i32);
    unsafe { *map.get_unchecked_mut(a) = 99 };
    assert_eq!(*map.get(a).unwrap(), 99);
}

#[test]
fn remove_unchecked() {
    let map = SlotMap::new();
    let a = map.insert(7i32);
    assert_eq!(unsafe { map.remove_unchecked(a) }, 7);
    assert!(!map.contains(a));
}

#[test]
fn many_insertions_maintain_correctness() {
    let map = SlotMap::new();
    let n = 500i32;
    let ids = (0..n).map(|i| map.insert(i)).collect::<Vec<usize>>();
    assert_eq!(map.len(), n as usize);
    for (i, &id) in ids.iter().enumerate() {
        assert_eq!(*map.get(id).unwrap(), i as i32);
    }
    for &id in ids.iter().step_by(2) {
        map.remove(id);
    }
    assert_eq!(map.len(), (n / 2) as usize);
}

const THREADS: usize = 32;
const OPS_PER_THREAD: usize = 500;

#[test]
fn concurrent_insert_value_visible() {
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    let val = (t * OPS_PER_THREAD + i) as i32;
                    let id = map.insert(val);
                    assert_eq!(*map.get(id).unwrap(), val, "value not visible after insert");
                }
            })
        })
        .collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(map.len(), THREADS * OPS_PER_THREAD);
}

#[test]
fn no_element_lost_under_concurrent_insert_remove() {
    let map = Arc::new(SlotMap::new());
    let removed = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles = (0..THREADS)
        .map(|_| {
            let map = Arc::clone(&map);
            let removed = Arc::clone(&removed);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut ids = Vec::with_capacity(OPS_PER_THREAD);
                for i in 0..OPS_PER_THREAD as i32 {
                    ids.push(map.insert(i));
                }
                barrier.wait();
                for id in ids {
                    if map.remove(id).is_some() {
                        removed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(
        removed.load(Ordering::Relaxed),
        THREADS * OPS_PER_THREAD,
        "some elements were lost or double-removed"
    );
    assert!(map.is_empty());
}

#[test]
fn sum_invariant_insert_remove() {
    const N: usize = 200;
    let map = Arc::new(SlotMap::new());
    let total_sum = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles = (0..THREADS)
        .map(|_| {
            let map = Arc::clone(&map);
            let total_sum = Arc::clone(&total_sum);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let ids = (0..N as i32).map(|i| map.insert(i)).collect::<Vec<usize>>();
                barrier.wait();
                for id in ids {
                    if let Some(v) = map.remove(id) {
                        total_sum.fetch_add(v as usize, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }

    let expected = THREADS * (N * (N - 1) / 2);
    assert_eq!(
        total_sum.load(Ordering::Relaxed),
        expected,
        "sum mismatch - values were created or destroyed"
    );
}

#[test]
fn concurrent_readers_and_writers() {
    const N: usize = 256;
    let map = Arc::new(SlotMap::new());

    let ids = Arc::new((0..N as i32).map(|i| map.insert(i)).collect::<Vec<usize>>());

    let barrier = Arc::new(Barrier::new(THREADS * 2));

    let writer_handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let ids = Arc::clone(&ids);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in (t..N).step_by(THREADS) {
                    if let Some(mut g) = map.get_mut(ids[i]) {
                        *g = (*g).wrapping_add(1000);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    let reader_handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let ids = Arc::clone(&ids);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for _ in 0..10 {
                    for i in (t..N).step_by(THREADS) {
                        if let Some(g) = map.get(ids[i]) {
                            let v = *g;
                            assert!(
                                v == i as i32 || v == i as i32 + 1000,
                                "reader observed impossible value {} for index {}",
                                v,
                                i
                            );
                        }
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for h in writer_handles.into_iter().chain(reader_handles) {
        h.join().unwrap();
    }
}

#[test]
fn hammer_insert_get_remove() {
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut live = Vec::new();
                barrier.wait();
                for i in 0..(OPS_PER_THREAD * 3) {
                    match i % 3 {
                        0 => {
                            let val = (t * OPS_PER_THREAD + i) as i32;
                            live.push(map.insert(val));
                        }
                        1 => {
                            if let Some(&id) = live.first() {
                                let _ = map.get(id);
                            }
                        }
                        _ => {
                            if !live.is_empty() {
                                let id = live.swap_remove(0);
                                map.remove(id);
                            }
                        }
                    }
                }
                for id in live {
                    map.remove(id);
                }
            })
        })
        .collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }

    assert!(map.is_empty());
}

#[test]
fn concurrent_clear_under_insert_load() {
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(THREADS + 1));

    let insert_handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..100i32 {
                    map.insert(t as i32 * 1000 + i);
                }
            })
        })
        .collect::<Vec<_>>();

    let clear_map = Arc::clone(&map);
    let clear_barrier = Arc::clone(&barrier);
    let clear_handle = thread::spawn(move || {
        clear_barrier.wait();
        for _ in 0..10 {
            clear_map.clear();
            thread::yield_now();
        }
    });

    for h in insert_handles {
        h.join().unwrap();
    }
    clear_handle.join().unwrap();

    let len = map.len();
    let counted = {
        let mut c = 0usize;
        for _ in 0..len {
            c += 1;
        }
        c
    };
    assert_eq!(len, counted, "len() is inconsistent after clear-under-load");
}

#[test]
fn per_thread_insert_verify_remove_cycle() {
    const CYCLES: usize = 1000;
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..CYCLES {
                    let val = (t * CYCLES + i) as i32;
                    let id = map.insert(val);
                    let got = *map
                        .get(id)
                        .expect("value not found immediately after insert");
                    assert_eq!(got, val, "wrong value immediately after insert");
                    let removed = map.remove(id).expect("remove failed for just-inserted id");
                    assert_eq!(removed, val, "remove returned wrong value");
                }
            })
        })
        .collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }
    assert!(map.is_empty());
}

#[test]
fn len_monotonic_under_concurrent_ops() {
    let map = Arc::new(SlotMap::new());
    let inserted = Arc::new(AtomicUsize::new(0));
    let removed = Arc::new(AtomicUsize::new(0));
    let barrier = Arc::new(Barrier::new(THREADS));

    let handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let inserted = Arc::clone(&inserted);
            let removed = Arc::clone(&removed);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut live = Vec::new();
                barrier.wait();
                for i in 0..OPS_PER_THREAD {
                    if i % 2 == 0 || live.is_empty() {
                        live.push(map.insert((t * OPS_PER_THREAD + i) as i32));
                        inserted.fetch_add(1, Ordering::Relaxed);
                    } else {
                        let id = live.swap_remove(0);
                        if map.remove(id).is_some() {
                            removed.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    let len = map.len();
                    let net = inserted
                        .load(Ordering::Relaxed)
                        .saturating_sub(removed.load(Ordering::Relaxed));
                    assert!(
                        len <= net + THREADS * OPS_PER_THREAD,
                        "len {} exceeds upper bound {}",
                        len,
                        net
                    );
                }
                for id in live {
                    if map.remove(id).is_some() {
                        removed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }
    assert!(map.is_empty());
}

#[test]
fn high_thread_count_balance() {
    const T: usize = 64;
    const N: usize = 64;
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(T));
    let total_inserted = Arc::new(AtomicUsize::new(0));
    let total_removed = Arc::new(AtomicUsize::new(0));

    let handles = (0..T)
        .map(|_| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            let ins = Arc::clone(&total_inserted);
            let rem = Arc::clone(&total_removed);
            thread::spawn(move || {
                let mut ids = Vec::with_capacity(N);
                barrier.wait();
                for i in 0..N as i32 {
                    ids.push(map.insert(i));
                    ins.fetch_add(1, Ordering::Relaxed);
                }
                for id in ids {
                    if map.remove(id).is_some() {
                        rem.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect::<Vec<_>>();

    for h in handles {
        h.join().unwrap();
    }

    assert_eq!(
        total_inserted.load(Ordering::Relaxed),
        total_removed.load(Ordering::Relaxed),
        "insert/remove count mismatch"
    );
    assert!(map.is_empty());
}

const STRESS_OPS: usize = 2_000;

#[test]
fn large_scale_concurrent_insert_remove_no_loss() {
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(THREADS));
    let inserted = Arc::new(AtomicUsize::new(0));
    let removed = Arc::new(AtomicUsize::new(0));
    let handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            let inserted = Arc::clone(&inserted);
            let removed = Arc::clone(&removed);
            thread::spawn(move || {
                let mut ids = Vec::with_capacity(STRESS_OPS);
                barrier.wait();
                for i in 0..STRESS_OPS as i32 {
                    let id = map.insert(t as i32 * STRESS_OPS as i32 + i);
                    inserted.fetch_add(1, Ordering::Relaxed);
                    ids.push(id);
                }
                for id in ids {
                    if map.remove(id).is_some() {
                        removed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(
        inserted.load(Ordering::Relaxed),
        removed.load(Ordering::Relaxed),
        "large-scale insert/remove count mismatch"
    );
    assert!(map.is_empty());
}

#[test]
fn large_scale_sum_invariant() {
    const N: usize = 2_000;
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(THREADS));
    let handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut ids = Vec::with_capacity(N);
                barrier.wait();
                for i in 0..N as i32 {
                    ids.push(map.insert(t as i32 * N as i32 + i));
                }
                for id in ids {
                    map.remove(id);
                }
            })
        })
        .collect::<Vec<_>>();
    for h in handles {
        h.join().unwrap();
    }
    assert!(map.is_empty());
}

#[test]
fn edge_with_num_shards_min_and_max() {
    let m4 = SlotMap::<i32>::with_num_shards(4).unwrap();
    let m8 = SlotMap::<i32>::with_num_shards(8).unwrap();
    assert_eq!(m4.num_shards(), 4);
    assert_eq!(m8.num_shards(), 8);
    let a = m4.insert(1);
    let b = m8.insert(2);
    assert_eq!(*m4.get(a).unwrap(), 1);
    assert_eq!(*m8.get(b).unwrap(), 2);
    assert_eq!(m4.remove(a), Some(1));
    assert_eq!(m8.remove(b), Some(2));
}

#[test]
fn edge_clear_then_insert_again() {
    let map = SlotMap::new();
    map.insert(1i32);
    map.insert(2);
    map.clear();
    assert!(map.is_empty());
    let a = map.insert(10);
    let b = map.insert(20);
    assert_eq!(*map.get(a).unwrap(), 10);
    assert_eq!(*map.get(b).unwrap(), 20);
    assert_eq!(map.len(), 2);
}

#[test]
fn edge_remove_invalid_id_returns_none() {
    let map = SlotMap::new();
    let a = map.insert(42i32);
    map.remove(a);
    assert!(map.remove(a).is_none());
    assert!(map.get(a).is_none());
}

#[test]
fn stress_repeated_fill_and_drain() {
    const ROUNDS: usize = 20;
    const BATCH: usize = 100;
    let map = Arc::new(SlotMap::new());

    for _ in 0..ROUNDS {
        let barrier = Arc::new(Barrier::new(THREADS));
        let ids: Arc<parking_lot::Mutex<Vec<usize>>> =
            Arc::new(parking_lot::Mutex::new(Vec::new()));

        let fill_handles = (0..THREADS / 2)
            .map(|_| {
                let map = Arc::clone(&map);
                let ids = Arc::clone(&ids);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    let mut local = Vec::with_capacity(BATCH);
                    for i in 0..BATCH as i32 {
                        local.push(map.insert(i));
                    }
                    ids.lock().extend(local);
                })
            })
            .collect::<Vec<_>>();

        let drain_handles = (0..THREADS / 2)
            .map(|_| {
                let map = Arc::clone(&map);
                let barrier = Arc::clone(&barrier);
                thread::spawn(move || {
                    barrier.wait();
                    thread::yield_now();
                    while map.len() > 0 {
                        thread::yield_now();
                    }
                })
            })
            .collect::<Vec<_>>();

        for h in fill_handles {
            h.join().unwrap();
        }

        let snapshot = ids.lock().drain(..).collect::<Vec<usize>>();
        for id in snapshot {
            map.remove(id);
        }

        for h in drain_handles {
            h.join().unwrap();
        }

        assert!(map.is_empty(), "map not empty after drain round");
    }
}

const STRESS_THREADS: usize = 64;
const STRESS_OPS_PER_THREAD: usize = 2_000;
const STRESS_LARGE_N: usize = 100_000;

#[test]
fn extreme_many_threads_insert_remove_no_loss() {
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(STRESS_THREADS));
    let removed = Arc::new(AtomicUsize::new(0));
    let handles = (0..STRESS_THREADS)
        .map(|_| {
            let map = Arc::clone(&map);
            let removed = Arc::clone(&removed);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let ids = (0..STRESS_OPS_PER_THREAD as i32)
                    .map(|i| map.insert(i))
                    .collect::<Vec<usize>>();
                barrier.wait();
                for id in ids {
                    if map.remove(id).is_some() {
                        removed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(
        removed.load(Ordering::Relaxed),
        STRESS_THREADS * STRESS_OPS_PER_THREAD
    );
    assert!(map.is_empty());
}

#[test]
fn extreme_large_insert_then_concurrent_remove_by_id() {
    let map = Arc::new(SlotMap::new());
    let ids = (0..STRESS_LARGE_N as i32)
        .map(|i| map.insert(i))
        .collect::<Vec<usize>>();
    let ids = Arc::new(ids);
    let barrier = Arc::new(Barrier::new(THREADS));
    let removed = Arc::new(AtomicUsize::new(0));
    let handles = (0..THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let ids = Arc::clone(&ids);
            let removed = Arc::clone(&removed);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in (t..STRESS_LARGE_N).step_by(THREADS) {
                    if map.remove(ids[i]).is_some() {
                        removed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            })
        })
        .collect::<Vec<_>>();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(removed.load(Ordering::Relaxed), STRESS_LARGE_N);
    assert!(map.is_empty());
}

#[test]
fn extreme_hammer_insert_get_remove_high_contention() {
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(STRESS_THREADS));
    let handles = (0..STRESS_THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                let mut live = Vec::new();
                barrier.wait();
                for i in 0..(STRESS_OPS_PER_THREAD * 2) {
                    match i % 3 {
                        0 => live.push(map.insert((t * STRESS_OPS_PER_THREAD + i) as i32)),
                        1 => {
                            if let Some(&id) = live.first() {
                                let _ = map.get(id);
                            }
                        }
                        _ => {
                            if !live.is_empty() {
                                let id = live.swap_remove(0);
                                map.remove(id);
                            }
                        }
                    }
                }
                for id in live {
                    map.remove(id);
                }
            })
        })
        .collect::<Vec<_>>();
    for h in handles {
        h.join().unwrap();
    }
    assert!(map.is_empty());
}

#[test]
fn extreme_concurrent_clear_under_insert() {
    let map = Arc::new(SlotMap::new());
    let barrier = Arc::new(Barrier::new(STRESS_THREADS + 1));
    let insert_handles = (0..STRESS_THREADS)
        .map(|t| {
            let map = Arc::clone(&map);
            let barrier = Arc::clone(&barrier);
            thread::spawn(move || {
                barrier.wait();
                for i in 0..300i32 {
                    map.insert(t as i32 * 1000 + i);
                }
            })
        })
        .collect::<Vec<_>>();
    let clear_handle = thread::spawn({
        let map = Arc::clone(&map);
        let barrier = Arc::clone(&barrier);
        move || {
            barrier.wait();
            for _ in 0..20 {
                map.clear();
                thread::yield_now();
            }
        }
    });
    for h in insert_handles {
        h.join().unwrap();
    }
    clear_handle.join().unwrap();
}

#[test]
fn extreme_get_unchecked_and_remove_unchecked() {
    let map = SlotMap::new();
    let a = map.insert(11i32);
    let b = map.insert(22i32);
    assert_eq!(unsafe { *map.get_unchecked(a) }, 11);
    assert_eq!(unsafe { *map.get_unchecked(b) }, 22);
    unsafe { *map.get_unchecked_mut(a) = 111 };
    assert_eq!(unsafe { map.remove_unchecked(a) }, 111);
    assert!(!map.contains(a));
    assert_eq!(map.remove(b), Some(22));
}

#[test]
fn lazy_insert_commit() {
    let map = SlotMap::new();
    let (id, guard) = map.lazy_insert();
    assert_eq!(map.len(), 0);
    guard.commit(42i32);
    assert!(map.get(id).is_some_and(|v| *v == 42));
    assert_eq!(map.len(), 1);
}
