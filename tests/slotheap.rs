use deadlock::SlotHeap;
use std::{
    collections::HashSet,
    sync::{mpsc, Arc, Barrier},
    thread,
};

#[test]
fn empty_heap_peek_is_none() {
    let heap = SlotHeap::<i32>::new();
    assert!(heap.is_empty());
    assert_eq!(heap.len(), 0);
    assert!(heap.peek().is_none());
    assert!(heap.peek_mut().is_none())
}

#[test]
fn insert_many_then_peek_is_min() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();

    for &v in [7, 3, 11, 1, 5, 9, 13, 0, 2, 4, 6, 8, 10, 12, 14].iter() {
        let (id, _) = heap.insert(v);
        ids.push((id, v))
    }

    assert_eq!(heap.len(), 15);
    assert_eq!(*heap.peek().unwrap(), 0);
}

#[test]
fn insert_ascending_then_min_is_first() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();

    for i in 0..32 {
        let (id, _) = heap.insert(i);
        ids.push(id)
    }

    assert_eq!(heap.len(), 32);
    assert_eq!(*heap.peek().unwrap(), 0);
}

#[test]
fn insert_descending_then_min_is_last_inserted() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();

    for i in (0..32).rev() {
        let (id, _) = heap.insert(i);
        ids.push(id)
    }

    assert_eq!(heap.len(), 32);
    assert_eq!(*heap.peek().unwrap(), 0);
}

#[test]
fn extract_all_via_into_inner_in_sorted_order() {
    let heap = SlotHeap::new();
    let mut by_value = Vec::new();

    for v in 0..32 {
        let (id, _) = heap.insert(v);
        by_value.push((id, v))
    }

    let mut extracted = Vec::new();

    while !by_value.is_empty() {
        let min_val = *heap.peek().unwrap();
        let pos = by_value.iter().position(|(_, v)| *v == min_val).unwrap();
        let (id, _) = by_value.remove(pos);
        let (got, _) = id.into_inner();
        assert_eq!(got, min_val);
        extracted.push(got)
    }

    assert!(heap.is_empty());
    assert_eq!(extracted.len(), 32);

    for (i, &v) in extracted.iter().enumerate() {
        assert_eq!(v, i)
    }
}

#[test]
fn remove_arbitrary_elements_then_peek_stays_correct() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();

    for i in 0..32 {
        let (id, _) = heap.insert(i);
        ids.push((id, i))
    }

    let to_remove = [1, 5, 10, 15, 20, 25, 30].iter().collect::<HashSet<_>>();

    let mut i = 0;

    while i < ids.len() {
        if to_remove.contains(&ids[i].1) {
            let (id, v) = ids.remove(i);
            let (got, _) = id.into_inner();
            assert_eq!(got, v)
        } else {
            i += 1
        }
    }

    assert_eq!(heap.len(), 25);
    assert_eq!(*heap.peek().unwrap(), 0);
}

#[test]
fn get_mut_modifies_value_and_reheapifies_on_drop() {
    let heap = SlotHeap::new();
    let (id_min, _) = heap.insert(10);
    let (id2, _) = heap.insert(20);
    let (id3, _) = heap.insert(30);

    assert_eq!(*heap.peek().unwrap(), 10);

    {
        let mut r = id_min.get_mut();
        *r = 100
    }

    assert_eq!(*heap.peek().unwrap(), 20);
    assert_eq!(*id2.get(), 20);
    assert_eq!(*id3.get(), 30);
}

#[test]
fn peek_mut_modify_then_drop_reheapifies() {
    let heap = SlotHeap::new();
    let _id1 = heap.insert(10).0;
    let _id2 = heap.insert(20).0;
    let _id3 = heap.insert(30).0;

    assert_eq!(*heap.peek().unwrap(), 10);

    {
        let mut p = heap.peek_mut().unwrap();
        *p = 100
    }

    assert_eq!(heap.len(), 3);
    assert_eq!(*heap.peek().unwrap(), 20);
}

#[test]
fn get_is_top_true_only_for_current_min_id() {
    let heap = SlotHeap::new();
    let (id1, _) = heap.insert(1);
    let (id2, _) = heap.insert(2);
    let (id3, _) = heap.insert(0);

    assert!(!id1.get().is_top());
    assert!(!id2.get().is_top());
    assert!(id3.get().is_top());

    drop(id3);

    assert!(id1.get().is_top())
}

#[test]
fn get_mut_is_top_true_only_for_current_min_id() {
    let heap = SlotHeap::new();
    let (id1, _) = heap.insert(1);
    let (id2, _) = heap.insert(2);
    let (id3, _) = heap.insert(0);

    assert!(!id1.get_mut().is_top());
    assert!(!id2.get_mut().is_top());
    assert!(id3.get_mut().is_top());

    drop(id3);

    assert!(id1.get_mut().is_top())
}

#[test]
fn is_top_after_into_inner_min_next_becomes_top() {
    let heap = SlotHeap::new();
    let (id0, _) = heap.insert(0);
    let (id1, _) = heap.insert(1);
    let (id2, _) = heap.insert(2);

    assert!(id0.get().is_top());
    assert!(!id1.get().is_top());
    assert!(!id2.get().is_top());

    let _ = id0.into_inner();

    assert!(id1.get().is_top());
    assert!(!id2.get().is_top());
}

#[test]
fn insert_then_drop_all_ids_heap_empty() {
    let heap = SlotHeap::new();
    let mut ids = Vec::new();

    for i in 0..32 {
        let (id, _) = heap.insert(i);
        ids.push(id)
    }

    assert_eq!(heap.len(), 32);

    drop(ids);

    assert_eq!(heap.len(), 0);
    assert!(heap.is_empty());
    assert!(heap.peek().is_none())
}

#[test]
fn insert_returns_is_top_when_new_min() {
    let heap = SlotHeap::new();

    let (_id1, top1) = heap.insert(5);
    assert!(top1);

    let (_id2, top2) = heap.insert(3);
    assert!(top2);

    let (_id3, top3) = heap.insert(4);
    assert!(!top3);

    assert_eq!(*heap.peek().unwrap(), 3);
}

#[test]
fn lazy_delete_then_peek_flushes_and_removes_deferred() {
    let heap = SlotHeap::new();
    let (id0, _) = heap.insert(0);
    let (id1, _) = heap.insert(1);
    let (id2, _) = heap.insert(2);

    assert_eq!(heap.len(), 3);
    assert_eq!(*heap.peek().unwrap(), 0);

    let (tx_to_thread, rx_from_main) = mpsc::channel::<deadlock::SlotHeapId<_>>();
    let (tx_to_main, rx_from_thread) = mpsc::channel();
    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let handle = thread::spawn(move || {
        let id1 = rx_from_main.recv().unwrap();
        let _guard = id1.get();
        barrier_clone.wait();
        barrier_clone.wait();
        drop(_guard);
        tx_to_main.send(id1).unwrap();
    });

    tx_to_thread.send(id1).unwrap();
    barrier.wait();
    drop(id0);
    barrier.wait();
    let _id1 = rx_from_thread.recv().unwrap();
    handle.join().unwrap();

    assert_eq!(heap.len(), 2);
    assert_eq!(*heap.peek().unwrap(), 1);
    assert_eq!(*id2.get(), 2);
}

#[test]
fn lazy_delete_then_peek_mut_flushes_and_removes_deferred() {
    let heap = SlotHeap::new();
    let (id0, _) = heap.insert(10);
    let (id1, _) = heap.insert(20);
    let (id2, _) = heap.insert(30);

    let (tx_to_thread, rx_from_main) = mpsc::channel::<deadlock::SlotHeapId<_>>();
    let (tx_to_main, rx_from_thread) = mpsc::channel();
    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let handle = thread::spawn(move || {
        let id1 = rx_from_main.recv().unwrap();
        let _guard = id1.get();
        barrier_clone.wait();
        barrier_clone.wait();
        drop(_guard);
        tx_to_main.send(id1).unwrap();
    });

    tx_to_thread.send(id1).unwrap();
    barrier.wait();
    drop(id0);
    barrier.wait();
    let _id1 = rx_from_thread.recv().unwrap();
    handle.join().unwrap();

    assert_eq!(heap.len(), 2);
    let min = heap.peek_mut().unwrap();
    assert_eq!(*min, 20);
    drop(min);
    assert_eq!(*heap.peek().unwrap(), 20);
    assert_eq!(*id2.get(), 30);
}

#[test]
fn multiple_lazy_deletes_flushed_by_single_peek() {
    let heap = SlotHeap::new();
    let (id0, _) = heap.insert(0);
    let (id1, _) = heap.insert(1);
    let (id2, _) = heap.insert(2);
    let (id3, _) = heap.insert(3);

    let (tx_to_thread, rx_from_main) = mpsc::channel::<deadlock::SlotHeapId<_>>();
    let (tx_to_main, rx_from_thread) = mpsc::channel();
    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let handle = thread::spawn(move || {
        let id2 = rx_from_main.recv().unwrap();
        let _guard = id2.get();
        barrier_clone.wait();
        barrier_clone.wait();
        drop(_guard);
        tx_to_main.send(id2).unwrap();
    });

    tx_to_thread.send(id2).unwrap();
    barrier.wait();
    drop(id0);
    drop(id1);
    barrier.wait();
    let _id2 = rx_from_thread.recv().unwrap();
    handle.join().unwrap();

    assert_eq!(heap.len(), 2);
    assert_eq!(*heap.peek().unwrap(), 2);
    assert_eq!(*id3.get(), 3);
}

#[test]
fn len_decrements_on_lazy_delete_before_flush() {
    let heap = SlotHeap::new();
    let (id0, _) = heap.insert(0);
    let (id1, _) = heap.insert(1);

    let (tx_to_thread, rx_from_main) = mpsc::channel::<deadlock::SlotHeapId<_>>();
    let (tx_to_main, rx_from_thread) = mpsc::channel();
    let barrier = Arc::new(Barrier::new(2));
    let barrier_clone = barrier.clone();
    let handle = thread::spawn(move || {
        let id1 = rx_from_main.recv().unwrap();
        let _guard = id1.get();
        barrier_clone.wait();
        barrier_clone.wait();
        drop(_guard);
        tx_to_main.send(id1).unwrap();
    });

    tx_to_thread.send(id1).unwrap();
    barrier.wait();
    drop(id0);
    barrier.wait();
    let _id1 = rx_from_thread.recv().unwrap();
    handle.join().unwrap();

    assert_eq!(heap.len(), 1);
    assert_eq!(*heap.peek().unwrap(), 1);
}
