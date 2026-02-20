use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use deadlock::{sync, unsync};
use parking_lot::RwLock;
use std::{hint, sync::Arc, thread};

fn unsync_0(n: usize, num_threads: usize) {
    let per_thread = n / num_threads;
    let map = Arc::new(RwLock::new(unsync::SlotMap::new()));

    let id_lists = Arc::new(RwLock::new(Vec::new()));
    let handles = (0..num_threads)
        .map(|t| {
            let map = map.clone();
            let id_lists = id_lists.clone();
            let start = t * per_thread;
            let values = (start..start + per_thread).collect::<Vec<_>>();
            thread::spawn(move || {
                let ids = values
                    .iter()
                    .map(|v| map.write().insert(*v))
                    .collect::<Vec<_>>();
                id_lists.write().push(ids);
            })
        })
        .collect::<Vec<_>>();
    handles.into_iter().for_each(|h| h.join().unwrap());

    let id_lists = Arc::into_inner(id_lists).unwrap().into_inner();
    let handles = id_lists
        .into_iter()
        .map(|ids| {
            let map = map.clone();
            thread::spawn(move || {
                ids.into_iter().for_each(|id| {
                    hint::black_box(map.write().remove(id));
                })
            })
        })
        .collect::<Vec<_>>();
    handles.into_iter().for_each(|h| h.join().unwrap());
    hint::black_box(map.read().len());
}

fn sync_0(n: usize, num_threads: usize) {
    let per_thread = n / num_threads;
    let map = Arc::new(sync::SlotMap::new());

    let id_lists = Arc::new(RwLock::new(Vec::new()));
    let handles = (0..num_threads)
        .map(|t| {
            let map = map.clone();
            let id_lists = id_lists.clone();
            let start = t * per_thread;
            let values = (start..start + per_thread).collect::<Vec<_>>();
            thread::spawn(move || {
                let ids = values.iter().map(|v| map.insert(*v)).collect::<Vec<_>>();
                id_lists.write().push(ids);
            })
        })
        .collect::<Vec<_>>();
    handles.into_iter().for_each(|h| h.join().unwrap());

    let id_lists = Arc::into_inner(id_lists).unwrap().into_inner();
    let handles = id_lists
        .into_iter()
        .map(|ids| {
            let map = map.clone();
            thread::spawn(move || {
                ids.into_iter().for_each(|id| {
                    hint::black_box(map.remove(id));
                })
            })
        })
        .collect::<Vec<_>>();
    handles.into_iter().for_each(|h| h.join().unwrap());
    hint::black_box(map.len());
}

fn bench_low_contention(c: &mut Criterion) {
    let n = 1000000;
    let num_threads = 4;

    let mut group = c.benchmark_group("low_contention");
    group.throughput(Throughput::Elements(n as u64));

    group.bench_function("sync_SlotMap/low", |b| b.iter(|| sync_0(n, num_threads)));
    group.bench_function("unsync_SlotMap/low", |b| {
        b.iter(|| unsync_0(n, num_threads))
    });

    group.finish();
}

fn bench_high_contention(c: &mut Criterion) {
    let n = 1000000;
    let num_threads_high = 32;

    let mut group = c.benchmark_group("high_contention");
    group.throughput(Throughput::Elements(n as u64));

    group.bench_function("sync_SlotMap/high", |b| {
        b.iter(|| sync_0(n, num_threads_high))
    });
    group.bench_function("unsync_SlotMap/high", |b| {
        b.iter(|| unsync_0(n, num_threads_high))
    });

    group.finish();
}

criterion_group!(benches, bench_low_contention, bench_high_contention);
criterion_main!(benches);
