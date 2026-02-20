use deadlock::unsync::SlotMap;

#[test]
fn insert_and_get() {
    let mut map = SlotMap::new();
    let a = map.insert(10i32);
    let b = map.insert(20);
    let c = map.insert(30);

    assert_eq!(map.get(a), Some(&10));
    assert_eq!(map.get(b), Some(&20));
    assert_eq!(map.get(c), Some(&30));
}

#[test]
fn contains() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    assert!(map.contains(a));
    assert!(!map.contains(a + 100));
}

#[test]
fn len_and_is_empty() {
    let mut map = SlotMap::new();
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
    let mut map = SlotMap::new();
    let a = map.insert(42i32);
    assert_eq!(map.remove(a), Some(42));
    assert_eq!(map.remove(a), None);
}

#[test]
fn remove_invalid_id_returns_none() {
    let mut map = SlotMap::<i32>::new();
    assert_eq!(map.remove(0), None);
    assert_eq!(map.remove(999), None);
}

#[test]
fn stable_ids_after_remove() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    let b = map.insert(2);
    let c = map.insert(3);

    map.remove(b);

    assert_eq!(map.get(a), Some(&1));
    assert_eq!(map.get(b), None);
    assert_eq!(map.get(c), Some(&3));
}

#[test]
fn free_slot_reuse() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    map.remove(a);
    let b = map.insert(2);
    assert_eq!(b, a);
    assert_eq!(map.get(b), Some(&2));
}

#[test]
fn get_mut() {
    let mut map = SlotMap::new();
    let a = map.insert(10i32);
    *map.get_mut(a).unwrap() = 99;
    assert_eq!(map.get(a), Some(&99));
    assert_eq!(map.get_mut(999), None);
}

#[test]
fn swap() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    let b = map.insert(2);

    assert!(map.swap(a, b).is_some());
    assert_eq!(map.get(a), Some(&2));
    assert_eq!(map.get(b), Some(&1));
}

#[test]
fn swap_same_id() {
    let mut map = SlotMap::new();
    let a = map.insert(7i32);
    assert!(map.swap(a, a).is_some());
    assert_eq!(map.get(a), Some(&7));
}

#[test]
fn swap_invalid_id() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    assert!(map.swap(a, 999).is_none());
    assert!(map.swap(999, a).is_none());
}

#[test]
fn clear() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    map.insert(2);
    map.clear();

    assert_eq!(map.len(), 0);
    assert!(map.is_empty());
    assert_eq!(map.get(a), None);
}

#[test]
fn ids_iterator() {
    let mut map = SlotMap::new();
    let a = map.insert(10i32);
    let b = map.insert(20);
    let c = map.insert(30);
    map.remove(b);

    let mut ids = map.ids().collect::<Vec<usize>>();
    ids.sort();
    assert_eq!(ids, vec![a, c]);
}

#[test]
fn values_iterator() {
    let mut map = SlotMap::new();
    map.insert(10i32);
    let b = map.insert(20);
    map.insert(30);
    map.remove(b);

    let mut vals = map.values().copied().collect::<Vec<i32>>();
    vals.sort();
    assert_eq!(vals, vec![10, 30]);
}

#[test]
fn values_mut_iterator() {
    let mut map = SlotMap::new();
    map.insert(1i32);
    map.insert(2);
    map.insert(3);

    for v in map.values_mut() {
        *v *= 10;
    }

    let mut vals = map.values().copied().collect::<Vec<i32>>();
    vals.sort();
    assert_eq!(vals, vec![10, 20, 30]);
}

#[test]
fn iter_iterator() {
    let mut map = SlotMap::new();
    let a = map.insert(10i32);
    let b = map.insert(20);
    map.remove(b);
    let c = map.insert(30);

    let mut pairs = map
        .iter()
        .map(|(id, &v)| (id, v))
        .collect::<Vec<(usize, i32)>>();
    pairs.sort();
    assert_eq!(pairs, vec![(a, 10), (c, 30)]);
}

#[test]
fn iter_mut_iterator() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    let b = map.insert(2);

    for (_, v) in map.iter_mut() {
        *v += 100;
    }

    assert_eq!(map.get(a), Some(&101));
    assert_eq!(map.get(b), Some(&102));
}

#[test]
fn drain_iterator() {
    let mut map = SlotMap::new();
    map.insert(1i32);
    map.insert(2);
    map.insert(3);

    let mut drained = map.drain().map(|(_, v)| v).collect::<Vec<i32>>();
    drained.sort();
    assert_eq!(drained, vec![1, 2, 3]);
    assert!(map.is_empty());
}

#[test]
fn into_ids_iterator() {
    let mut map = SlotMap::new();
    let a = map.insert(10i32);
    let b = map.insert(20);
    let c = map.insert(30);

    let mut ids = map.into_ids().collect::<Vec<usize>>();
    ids.sort();
    assert_eq!(ids, vec![a, b, c]);
}

#[test]
fn into_values_iterator() {
    let mut map = SlotMap::new();
    map.insert(10i32);
    map.insert(20);
    map.insert(30);

    let mut vals = map.into_values().collect::<Vec<i32>>();
    vals.sort();
    assert_eq!(vals, vec![10, 20, 30]);
}

#[test]
fn exact_size_iterator() {
    let mut map = SlotMap::new();
    map.insert(1i32);
    map.insert(2);
    map.insert(3);

    let iter = map.ids();
    assert_eq!(iter.len(), 3);
}

#[test]
fn double_ended_iterator() {
    let mut map = SlotMap::new();
    map.insert(1i32);
    map.insert(2);
    map.insert(3);

    let mut vals = map.values().copied().rev().collect::<Vec<i32>>();
    vals.sort_by(|a, b| b.cmp(a));
    let mut expected = vec![1, 2, 3];
    expected.sort_by(|a, b| b.cmp(a));
    assert_eq!(vals, expected);
}

#[test]
fn many_insertions_and_removals() {
    let mut map = SlotMap::new();
    let ids = (0..100i32).map(|i| map.insert(i)).collect::<Vec<usize>>();

    for &id in ids.iter().step_by(2) {
        map.remove(id);
    }

    assert_eq!(map.len(), 50);

    for (i, &id) in ids.iter().enumerate().filter(|(i, _)| i % 2 != 0) {
        assert_eq!(map.get(id), Some(&(i as i32)));
    }
}

#[test]
fn get_unchecked() {
    let mut map = SlotMap::new();
    let a = map.insert(42i32);
    let val = unsafe { *map.get_unchecked(a) };
    assert_eq!(val, 42);
}

#[test]
fn get_unchecked_mut() {
    let mut map = SlotMap::new();
    let a = map.insert(42i32);
    unsafe { *map.get_unchecked_mut(a) = 99 };
    assert_eq!(map.get(a), Some(&99));
}

#[test]
fn remove_unchecked() {
    let mut map = SlotMap::new();
    let a = map.insert(7i32);
    let val = unsafe { map.remove_unchecked(a) };
    assert_eq!(val, 7);
    assert!(!map.contains(a));
}

#[test]
fn swap_unchecked() {
    let mut map = SlotMap::new();
    let a = map.insert(1i32);
    let b = map.insert(2);
    unsafe { map.swap_unchecked(a, b) };
    assert_eq!(map.get(a), Some(&2));
    assert_eq!(map.get(b), Some(&1));
}

#[test]
fn default_slotmap() {
    let mut map = SlotMap::<i32>::default();
    assert!(map.is_empty());
    let a = map.insert(10);
    assert_eq!(map.get(a), Some(&10));
}
