use deadlock::{
    SlotMap, SlotMapId, SlotMapIter, SlotMapIterMut, SlotMapRef, SlotMapRefMut, SlotMapShardRef,
};
use std::{
    iter,
    sync::{Arc, Mutex},
    thread,
};

fn _slotmap_send_sync_checks<'a>() {
    fn assert_send_sync<T: Send + Sync>() {}

    assert_send_sync::<SlotMap<i32>>();
    assert_send_sync::<SlotMapId<i32>>();
    assert_send_sync::<SlotMapRef<'a, i32>>();
    assert_send_sync::<SlotMapRefMut<'a, i32>>();
    assert_send_sync::<SlotMapShardRef<'a, i32>>();
    assert_send_sync::<SlotMapIter<'a, i32>>();
    assert_send_sync::<SlotMapIterMut<'a, i32>>()
}

#[test]
fn shards_iter_yields_all_entries() {
    let map = SlotMap::new();
    let n = 64_i32;
    let _ids = (0..n).map(|i| map.insert(i * 5)).collect::<Vec<_>>();

    let mut collected = map
        .shards()
        .flat_map(|shard| shard.iter().copied().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    collected.sort_unstable();

    assert_eq!(collected.len(), n as usize);
    assert_eq!(collected, (0..n).map(|i| i * 5).collect::<Vec<_>>())
}

#[test]
fn shards_total_count_matches_len() {
    let map = SlotMap::new();
    let n = 48;
    let _ids = (0..n).map(|i| map.insert(i)).collect::<Vec<_>>();

    let total = map
        .shards()
        .map(|shard| shard.iter().count())
        .sum::<usize>();
    assert_eq!(total, n);
    assert_eq!(total, map.len())
}

#[test]
fn insert_get_many_preserves_values_and_len() {
    let map = SlotMap::new();
    let n = 64;
    let ids = (0..n).map(|i| map.insert(i)).collect::<Vec<_>>();

    assert_eq!(map.len(), n);

    for (i, id) in ids.iter().enumerate() {
        assert_eq!(*id.get(), i)
    }
}

#[test]
fn len_decreases_when_id_is_dropped() {
    let map = SlotMap::new();
    let n = 48;
    let mut ids = (0..n).map(|i| map.insert(i)).collect::<Vec<_>>();
    assert_eq!(map.len(), n);

    for _ in 0..(n / 2) {
        ids.pop();
    }

    assert_eq!(map.len(), n / 2);
    assert_eq!(ids.len(), n / 2);

    drop(ids);

    assert!(map.is_empty())
}

#[test]
fn into_inner_returns_value_and_removes_entry() {
    let map = SlotMap::new();
    let id = map.insert(100);
    assert_eq!(map.len(), 1);

    let v = id.into_inner();
    assert_eq!(v, 100);
    assert_eq!(map.len(), 0);
    assert!(map.is_empty())
}

#[test]
fn get_mut_modifies_value() {
    let map = SlotMap::new();
    let id = map.insert(10);
    assert_eq!(*id.get(), 10);

    *id.get_mut() = 20;
    assert_eq!(*id.get(), 20);
}

#[test]
fn slot_reuse_after_removal_keeps_correct_values() {
    let map = SlotMap::new();
    let n = 32;
    let ids = (0..n).map(|i| map.insert(i)).collect::<Vec<_>>();

    for id in ids {
        let _ = id.into_inner();
    }

    assert!(map.is_empty());

    let again = (0..n).map(|i| map.insert(i + 100)).collect::<Vec<_>>();
    assert_eq!(map.len(), n);

    for (i, id) in again.iter().enumerate() {
        assert_eq!(*id.get(), i + 100)
    }
}

#[test]
fn ref_remains_valid_after_other_id_dropped() {
    let map = SlotMap::new();
    let id0 = map.insert(42);
    let id1 = map.insert(43);
    let r0 = id0.get();
    assert_eq!(*r0, 42);

    drop(id1);
    assert_eq!(*r0, 42);
    assert_eq!(map.len(), 1)
}

#[test]
fn iter_yields_all_entries() {
    let map = SlotMap::new();
    let n = 32;
    let _ids = (0..n).map(|i| map.insert(i * 10)).collect::<Vec<_>>();

    let collected = map.iter().map(|r| *r).collect::<Vec<_>>();
    assert_eq!(collected.len(), n);

    let mut sorted = collected;
    sorted.sort_unstable();
    assert_eq!(sorted, (0..n).map(|i| i * 10).collect::<Vec<_>>())
}

#[test]
fn iter_mut_modifies_values() {
    let map = SlotMap::new();
    let _ids = (0..8).map(|i| map.insert(i)).collect::<Vec<_>>();

    for mut r in map.iter_mut() {
        *r += 100;
    }

    let collected = map.iter().map(|r| *r).collect::<Vec<_>>();
    let mut sorted = collected;
    sorted.sort_unstable();
    assert_eq!(sorted, (100..108).collect::<Vec<_>>())
}

#[test]
fn send_sync_multi_threaded_insert() {
    let map = Arc::new(SlotMap::new());
    let ids = Arc::new(Mutex::new(Vec::new()));
    let handles = (0..4)
        .map(|i| {
            let map = map.clone();
            let ids = ids.clone();

            thread::spawn(move || {
                let local_ids = (0..100)
                    .map(|j| map.insert(i * 100 + j))
                    .collect::<Vec<_>>();
                ids.lock().unwrap().extend(local_ids)
            })
        })
        .take(4)
        .collect::<Vec<_>>();

    for handle in handles {
        handle.join().unwrap()
    }

    assert_eq!(map.len(), 400)
}

#[test]
fn send_sync_multi_threaded_iter() {
    let map = Arc::new(SlotMap::new());
    let _ids = (0..100).map(|i| map.insert(i)).collect::<Vec<_>>();

    let handles = iter::repeat_with(|| {
        let map = map.clone();

        thread::spawn(move || {
            let count = map.iter().count();
            assert_eq!(count, 100)
        })
    })
    .take(4)
    .collect::<Vec<_>>();

    for handle in handles {
        handle.join().unwrap()
    }
}
