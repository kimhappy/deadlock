use parking_lot::Once;
use std::{cell::UnsafeCell, mem::MaybeUninit, thread};

pub fn default_num_shards() -> usize {
    struct Cache(UnsafeCell<MaybeUninit<usize>>);

    unsafe impl Sync for Cache {}

    static CACHE: Cache = Cache(UnsafeCell::new(MaybeUninit::uninit()));
    static ONCE: Once = Once::new();

    ONCE.call_once(|| {
        let num_threads = thread::available_parallelism().map_or(1, Into::into);
        let num_shards = num_threads.next_power_of_two() * 4;
        unsafe {
            (*CACHE.0.get()).write(num_shards);
        }
    });

    unsafe { (*CACHE.0.get()).assume_init_read() }
}
