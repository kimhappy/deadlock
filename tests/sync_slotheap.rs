use deadlock::sync::SlotHeap;

const N: usize = 40;

fn lcg_next(state: &mut u32) -> u32 {
    *state = state.wrapping_mul(1103515245).wrapping_add(12345);
    *state & 0x7fff_ffff
}

fn shuffle_slice(rng: &mut u32, keys: &mut [i32]) {
    for i in 0..keys.len().saturating_sub(1) {
        let rem = (keys.len() - i) as u32;
        let j = i + (lcg_next(rng) % rem) as usize;
        keys.swap(i, j);
    }
}

fn pop_all_sorted<K: Ord, V>(heap: &SlotHeap<K, V>) -> Vec<(K, V)> {
    let mut result = Vec::new();
    while let Some(item) = heap.pop() {
        result.push(item);
    }
    result
}

fn is_non_decreasing<K: Ord, V>(items: &[(K, V)]) -> bool {
    items.windows(2).all(|w| w[0].0 <= w[1].0)
}

#[test]
fn insert_and_pop_order() {
    let heap = SlotHeap::new();
    let mut keys = (0..N as i32)
        .map(|i| (i * 7 + 3) % (N as i32))
        .collect::<Vec<i32>>();
    let mut rng = 1u32;
    shuffle_slice(&mut rng, &mut keys);
    for k in &keys {
        heap.insert(*k, ());
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), N);
}

const RAND_N: usize = 5000;

#[test]
fn random_data_insert_then_pop_all() {
    let heap = SlotHeap::new();
    let mut keys = (0..RAND_N as i32).collect::<Vec<i32>>();
    let mut rng = 42u32;
    shuffle_slice(&mut rng, &mut keys);
    for &k in &keys {
        heap.insert(k, ());
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), RAND_N);
    let mut sorted_keys = result.into_iter().map(|r| r.0).collect::<Vec<i32>>();
    sorted_keys.sort();
    assert_eq!(sorted_keys, (0..RAND_N as i32).collect::<Vec<_>>());
}

#[test]
fn remove_one_then_pop_all_ordered() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();
    for k in 0..30i32 {
        ids.push(heap.insert(k, ()));
    }
    heap.remove(ids[15]);
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), 29);
    let keys = result.into_iter().map(|r| r.0).collect::<Vec<i32>>();
    assert_eq!(keys, (0..30i32).filter(|&x| x != 15).collect::<Vec<_>>());
}

#[test]
fn remove_several_then_pop_all_ordered() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();
    for k in 0..50i32 {
        ids.push(heap.insert(k, ()));
    }
    for &i in &[0, 10, 25, 30, 49] {
        if i < ids.len() {
            heap.remove(ids[i]);
        }
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), 45);
}

#[test]
fn interleave_insert_remove_pop_ordered() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();
    for k in 0..20i32 {
        ids.push(heap.insert(k, ()));
    }
    heap.remove(ids[5]);
    assert_eq!(heap.pop(), Some((0, ())));
    assert_eq!(heap.pop(), Some((1, ())));
    heap.remove(ids[12]);
    let mut result = Vec::new();
    while let Some((k, _)) = heap.pop() {
        result.push(k);
    }
    assert!(result.windows(2).all(|w| w[0] <= w[1]));
}

#[test]
fn random_ops_insert_pop_remove() {
    let heap = SlotHeap::new();
    let mut rng = 123u32;
    let num_keys = 3000usize;
    let num_ops = 15_000usize;
    let mut keys = (0..num_keys as i32).collect::<Vec<i32>>();
    shuffle_slice(&mut rng, &mut keys);
    let mut key_index = 0usize;
    let mut live_ids: Vec<usize> = Vec::new();
    let mut popped_keys: Vec<i32> = Vec::new();
    let mut removed_keys: Vec<i32> = Vec::new();
    for _ in 0..num_ops {
        let op = lcg_next(&mut rng) % 3;
        if op == 0 && key_index < keys.len() {
            let k = keys[key_index];
            key_index += 1;
            let id = heap.insert(k, ());
            live_ids.push(id);
        } else if op == 1 {
            if let Some((k, _)) = heap.pop() {
                popped_keys.push(k);
            }
        } else if op == 2 && !live_ids.is_empty() {
            let idx = (lcg_next(&mut rng) as usize) % live_ids.len();
            let id = live_ids.swap_remove(idx);
            if let Some((k, _)) = heap.remove(id) {
                removed_keys.push(k);
            } else {
                live_ids.push(id);
            }
        }
    }
    let mut final_popped = Vec::new();
    while let Some((k, _)) = heap.pop() {
        final_popped.push(k);
    }
    assert!(heap.is_empty());
    assert!(
        final_popped.windows(2).all(|w| w[0] <= w[1]),
        "heap pop order violated in final drain: final_popped not non-decreasing"
    );
    let mut all_out: Vec<i32> = popped_keys;
    all_out.extend(final_popped);
    all_out.extend(removed_keys);
    all_out.sort();
    let mut inserted: Vec<i32> = keys[..key_index].to_vec();
    inserted.sort();
    assert_eq!(all_out.len(), inserted.len());
    assert_eq!(all_out, inserted);
}

#[test]
fn remove_by_id() {
    let heap = SlotHeap::new();
    let ids = (0..N as i32)
        .map(|i| heap.insert(i, ()))
        .collect::<Vec<usize>>();
    for &i in &[0, 5, 10, N / 2, N - 1] {
        if i < ids.len() {
            assert_eq!(heap.remove(ids[i]), Some((i as i32, ())));
        }
    }
    assert_eq!(heap.len(), N - 5);
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), N - 5);
}

#[test]
fn remove_last_element() {
    let heap = SlotHeap::new();
    for i in 0..N as i32 {
        heap.insert(i, ());
    }
    let last_id = heap.insert(1000, ());
    assert_eq!(heap.remove(last_id), Some((1000, ())));
    assert_eq!(heap.len(), N);
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
}

#[test]
fn remove_root() {
    let heap = SlotHeap::new();
    let keys = (0..N as i32).rev().collect::<Vec<i32>>();
    let root_id = heap.insert(keys[0], ());
    for k in &keys[1..] {
        heap.insert(*k, ());
    }
    assert_eq!(heap.remove(root_id), Some((keys[0], ())));
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
}

#[test]
fn get_by_id() {
    let heap = SlotHeap::new();
    let ids = (0..N as i32)
        .map(|i| heap.insert(i * 2, ()))
        .collect::<Vec<usize>>();
    for (i, &id) in ids.iter().enumerate() {
        assert_eq!(heap.get(id).map(|g| *g), Some((i as i32 * 2, ())));
        assert_eq!(heap.get_key(id).map(|g| *g), Some(i as i32 * 2));
    }
    assert!(heap.get(99999).is_none());
    assert!(heap.get_key(99999).is_none());
}

#[test]
fn get_value_mut() {
    let heap = SlotHeap::new();
    let ids = (0..N as i32)
        .map(|i| heap.insert(i, i as u64))
        .collect::<Vec<usize>>();
    for (i, &id) in ids.iter().enumerate() {
        *heap.get_value_mut(id).unwrap() = (i as u64) * 10;
    }
    for (i, &id) in ids.iter().enumerate() {
        assert_eq!(heap.get_value(id).map(|g| *g), Some(i as u64 * 10));
    }
}

#[test]
fn peek_value_mut() {
    let heap = SlotHeap::new();
    for i in (0..N as i32).rev() {
        heap.insert(i, i as u64);
    }
    *heap.peek_value_mut().unwrap() = 999;
    assert_eq!(heap.peek_key().map(|g| *g), Some(0));
    assert_eq!(heap.peek_value().map(|g| *g), Some(999));
}

#[test]
fn peek_mut_no_key_change() {
    let heap = SlotHeap::new();
    for i in (0..N as i32).rev() {
        heap.insert(i, ());
    }
    {
        let _guard = heap.peek_mut().unwrap();
    }
    assert_eq!(heap.peek_key().map(|g| *g), Some(0));
}

#[test]
fn peek_mut_with_key_increase() {
    let heap = SlotHeap::new();
    for i in 0..N as i32 {
        heap.insert(i, ());
    }
    {
        let mut guard = heap.peek_mut().unwrap();
        (*guard).0 = N as i32 + 100;
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.last().unwrap().0, N as i32 + 100);
}

#[test]
fn peek_key_mut_with_increase() {
    let heap = SlotHeap::new();
    for i in 0..N as i32 {
        heap.insert(i, ());
    }
    {
        let mut guard = heap.peek_key_mut().unwrap();
        *guard = N as i32 + 200;
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.last().unwrap().0, N as i32 + 200);
}

#[test]
fn get_key_mut_decrease() {
    let heap = SlotHeap::new();
    let mid = N / 2;
    let id = heap.insert(mid as i32, ());
    for i in 0..N as i32 {
        if i != mid as i32 {
            heap.insert(i, ());
        }
    }
    {
        let mut guard = heap.get_key_mut(id).unwrap();
        *guard = -1;
    }
    assert_eq!(heap.peek_key().map(|g| *g), Some(-1));
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
}

#[test]
fn get_key_mut_increase() {
    let heap = SlotHeap::new();
    let id = heap.insert(0i32, ());
    for i in 1..N as i32 {
        heap.insert(i, ());
    }
    {
        let mut guard = heap.get_key_mut(id).unwrap();
        *guard = N as i32 + 500;
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.last().unwrap().0, N as i32 + 500);
}

#[test]
fn get_mut_no_change() {
    let heap = SlotHeap::new();
    let ids = (0..N as i32)
        .map(|i| heap.insert(i, ()))
        .collect::<Vec<usize>>();
    let mid_id = ids[N / 2];
    {
        let _guard = heap.get_mut(mid_id).unwrap();
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), N);
}

#[test]
fn get_key_mut_no_change() {
    let heap = SlotHeap::new();
    let ids = (0..N as i32)
        .map(|i| heap.insert(i, ()))
        .collect::<Vec<usize>>();
    let mid_id = ids[N / 2];
    {
        let _guard = heap.get_key_mut(mid_id).unwrap();
    }
    assert_eq!(heap.len(), N);
}

#[test]
fn clear() {
    let heap = SlotHeap::new();
    let ids = (0..N as i32)
        .map(|i| heap.insert(i, ()))
        .collect::<Vec<usize>>();
    heap.clear();
    assert!(heap.is_empty());
    for &id in &ids {
        assert!(!heap.contains(id));
    }
    assert_eq!(heap.pop(), None);
}

#[test]
fn heap_property_after_remove_mid() {
    let heap = SlotHeap::new();
    let ids = (0..N as i32)
        .map(|i| heap.insert(i, ()))
        .collect::<Vec<usize>>();
    heap.remove(ids[3]);
    heap.remove(ids[7]);
    heap.remove(ids[N / 2]);
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), N - 3);
}

#[test]
fn heap_property_reversed_input() {
    let heap = SlotHeap::new();
    for i in (0..N as i32).rev() {
        heap.insert(i, ());
    }
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result.len(), N);
}

#[test]
fn duplicate_keys() {
    let heap = SlotHeap::new();
    for _ in 0..10 {
        heap.insert(5i32, 'a');
        heap.insert(5, 'b');
        heap.insert(5, 'c');
    }
    heap.insert(1i32, 'd');
    let result = pop_all_sorted(&heap);
    assert!(is_non_decreasing(&result));
    assert_eq!(result[0].0, 1);
    assert_eq!(result.iter().filter(|r| r.0 == 5).count(), 30);
}
