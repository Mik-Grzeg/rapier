use criterion::{black_box, criterion_group, criterion_main, Criterion};
use rapier::{Throttler, ThrottlerRwLock, Counter};
use std::sync::Arc;
use std::thread;

fn increment(throttler: Arc<impl Counter::<Type = u64>>) {
    for _ in 0..100 {
        if !throttler.check_counter_overcommit() {
            throttler.increment();
        }
    }
}

fn spawn_tasks(n: u16, throttler: Arc<impl Counter::<Type = u64> + Send + Sync + 'static> ) {
    let handles = (0..n)
        .map(|_| {
            let throttler_cp = Arc::clone(&throttler);
            thread::spawn(|| increment(throttler_cp))
        });

    for handle in handles {
        handle.join();
    }

    let counted = throttler.get_and_refresh();
}

fn criterion_benchmark(c: &mut Criterion) {
    let n = 4;
    c.bench_function("Atomic throttler", |b| b.iter(|| {
        let throttler = Arc::new(Throttler::default());
        spawn_tasks(n, throttler)
    }));

    c.bench_function("RwLock throttler", |b| b.iter(|| {
        let throttler = Arc::new(ThrottlerRwLock::default());
        spawn_tasks(n, throttler)
    }));
}


criterion_group!(benches, criterion_benchmark);
criterion_main!(benches);
