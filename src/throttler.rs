use std::sync::RwLock;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;

pub trait Counter {
    type Type;

    fn increment(&self) -> Self::Type;
    fn get_and_refresh(&self) -> Self::Type;
    fn check_counter_overcommit(&self) -> bool;
}

#[derive(Default)]
pub struct ThrottlerRwLock {
    counter: RwLock<u64>,
    threshold: Option<u64>,
}

impl ThrottlerRwLock {
    pub fn new(threshold: u64) -> Self {
        ThrottlerRwLock {
            threshold: Some(threshold),
            ..Default::default()
        }
    }
}

impl Counter for ThrottlerRwLock {
    type Type = u64;

    fn increment(&self) -> Self::Type {
        let mut c = self.counter.write().unwrap();
        *c += 1;
        *c
    }

    fn get_and_refresh(&self) -> Self::Type {
        let mut c = self.counter.write().unwrap();
        let old = *c;
        *c = 0;
        old
    }

    fn check_counter_overcommit(&self) -> bool {
        self.threshold.map_or(false, |threshold| *self.counter.read().unwrap() >= threshold)
    }
}

#[derive(Default)]
pub struct Throttler {
    counter: AtomicU64,
    threshold: Option<u64>,
}

impl Throttler {
    pub fn new(threshold: u64) -> Self {
        Throttler {
            threshold: Some(threshold),
            ..Default::default()
        }
    }
}

impl Counter for Throttler {
    type Type = u64;

    fn get_and_refresh(&self) -> Self::Type {
        self.counter.swap(0, Ordering::SeqCst)
    }

    fn increment(&self) -> Self::Type {
        let old = self.counter.fetch_add(1, Ordering::SeqCst);
        trace!("Incrementing rps counter in current interval to {}", old);
        old
    }

    fn check_counter_overcommit(&self) -> bool {
        self.threshold.map_or(false, |threshold| {
            self.counter.load(Ordering::SeqCst) >= threshold
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::future::join_all;
    use rstest::*;

    use std::sync::Arc;
    use tokio::task;

    #[rstest]
    #[case(35, true, Throttler::new(30))]
    #[case(25, false, Throttler::new(30))]
    #[case(30, true, Throttler::new(30))]
    #[case(30, true, ThrottlerRwLock::new(30))]
    #[case(30, true, ThrottlerRwLock::new(30))]
    #[case(30, true, ThrottlerRwLock::new(30))]
    #[tokio::test]
    async fn check_throttled_flow(
        #[case] cummulative_increments_in_counter: u64,
        #[case] expected_overcommit: bool,
        #[case] throttler: impl Counter::<Type = u64> + Send + Sync + 'static
    ) {
        // given
        let atmoic = Arc::new(throttler);
        let workers = 2;

        // when
        let handles = (0..workers).map(|_| {
            task::spawn({
                let atmoic = Arc::clone(&atmoic);
                async move {
                    for _ in 1..=cummulative_increments_in_counter / workers {
                        atmoic.increment();
                    }
                }
            })
        });
        join_all(handles).await;

        // then
        assert!(!(expected_overcommit ^ atmoic.check_counter_overcommit())) // XNOR
    }

    #[test]
    fn test_refresh_counter() {
        // given
        let threshold = 3;
        let atmoic = Throttler::new(threshold);

        for _ in 0..threshold {
            atmoic.increment();
        }

        // when
        assert!(atmoic.check_counter_overcommit());
        let old_val = atmoic.get_and_refresh();

        // then
        assert_eq!(threshold, old_val);
        assert!(!atmoic.check_counter_overcommit());
    }

    #[test]
    fn test_non_throttled() {
        // given
        let atmoic = Throttler::default();

        for _ in 0..100 {
            // when
            atmoic.increment();

            // then
            assert!(!atmoic.check_counter_overcommit())
        }
    }
}
