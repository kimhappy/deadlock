use deadlock::SlotMap;
use std::{
    sync::{Arc, Mutex},
    thread,
};

fn _assert_send<T: Send>() {}
fn _assert_sync<T: Sync>() {}

#[allow(dead_code)]
fn _compile_time_trait_checks() {
    _assert_send::<SlotMap<i32>>();
    _assert_sync::<SlotMap<i32>>();
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
    assert_eq!(sorted, (0..n).map(|i| i * 10).collect::<Vec<_>>());
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
    assert_eq!(sorted, (100..108).collect::<Vec<_>>());
}

#[test]
fn shard_iter_yields_all_entries() {
    let map = SlotMap::new();
    let n = 32;
    let _ids = (0..n).map(|i| map.insert(i * 10)).collect::<Vec<_>>();

    let collected = map.arc_iter().map(|r| *r).collect::<Vec<_>>();
    assert_eq!(collected.len(), n);
    let mut sorted = collected;
    sorted.sort_unstable();
    assert_eq!(sorted, (0..n).map(|i| i * 10).collect::<Vec<_>>());
}

#[test]
fn shard_iter_mut_modifies_values() {
    let map = SlotMap::new();
    let _ids = (0..8).map(|i| map.insert(i)).collect::<Vec<_>>();

    for mut r in map.arc_iter_mut() {
        *r += 100;
    }

    let collected = map.iter().map(|r| *r).collect::<Vec<_>>();
    let mut sorted = collected;
    sorted.sort_unstable();
    assert_eq!(sorted, (100..108).collect::<Vec<_>>());
}

#[test]
fn send_sync_multi_threaded_insert() {
    let map = Arc::new(SlotMap::new());
    let ids = Arc::new(Mutex::new(Vec::new()));
    let mut handles = vec![];

    for i in 0..4 {
        let map_clone = map.clone();
        let ids_clone = ids.clone();
        let handle = thread::spawn(move || {
            let mut local_ids = Vec::new();
            for j in 0..100 {
                local_ids.push(map_clone.insert(i * 100 + j));
            }
            ids_clone.lock().unwrap().extend(local_ids);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }

    assert_eq!(map.len(), 400);
    drop(ids);
}

#[test]
fn send_sync_multi_threaded_iter() {
    let map = Arc::new(SlotMap::new());
    let _ids = (0..100).map(|i| map.insert(i)).collect::<Vec<_>>();

    let mut handles = vec![];
    for _ in 0..4 {
        let map_clone = map.clone();
        let handle = thread::spawn(move || {
            let count = map_clone.iter().count();
            assert_eq!(count, 100);
        });
        handles.push(handle);
    }

    for handle in handles {
        handle.join().unwrap();
    }
}
