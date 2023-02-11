
use reqwest::RequestBuilder;




use std::sync::Arc;



use tokio::sync::AcquireError;
use tokio::sync::OwnedSemaphorePermit;
use tokio::sync::Semaphore;


pub trait Counter {
    type Out;
    type ErrType;

    async fn execute_under_semaphore(&self) -> Result<Self::Out, Self::ErrType>;
    fn refresh(&self) -> usize;
}

pub struct ThrottlerSempahores {
    permits: Arc<Semaphore>,
    threshold: u64,
}

// impl<F, Out, U> Counter for ThrottlerSempahores<F, Out, U>
// where
//     F: Fn(U) -> Out,
//     U: IntoUrl + Clone
// {
//     type Out = Out;
//     type ErrType = AcquireError;

//     async fn execute_under_semaphore(&self) -> Result<Out, AcquireError> {
//         let permit = self.permits.acquire().await?;
//         let to_return = Ok((self.to_execute)(self.arg.clone()));

//         permit.forget();
//         to_return

//     }

//     fn refresh(&self) -> usize {
//         let available_permits = self.permits.available_permits();
//         self.permits.add_permits(self.threshold as usize - available_permits);
//         available_permits
//     }
// }

impl ThrottlerSempahores {
    pub fn new(threshold: u64) -> Self {
        ThrottlerSempahores {
            permits: Arc::new(Semaphore::new(threshold as usize)),
            threshold,
        }
    }

    #[inline]
    pub fn threshold(&self) -> u64 {
        self.threshold
    }

    pub async fn execute_under_semaphore<F, Out>(
        &self,
        f: F,
        request_builder: RequestBuilder,
    ) -> Result<Out, AcquireError>
    where
        F: Fn(RequestBuilder) -> Out,
    {
        let permit = self.permits.acquire().await?;
        let to_return = Ok(f(request_builder));

        permit.forget();
        to_return
    }

    pub async fn execute_and_give_permit<F, Out>(
        &self,
        f: F,
        request_builder: RequestBuilder,
    ) -> Result<(Out, OwnedSemaphorePermit), AcquireError>
    where
        F: Fn(RequestBuilder) -> Out,
    {
        let permit = self.permits.clone().acquire_owned().await?;
        Ok((f(request_builder), permit))
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
        #[case] throttler: impl Counter<Type = u64> + Send + Sync + 'static,
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
