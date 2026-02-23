use deadlock::SlotMap;

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
