use super::metrics::{MetricsStore, Reporter, ThroughputMetric};
use crate::fixed_window::{round_up_datetime, until_event};

use crate::throttler::ThrottlerSempahores;
use chrono::Utc;
use futures::stream::FuturesUnordered;
use futures::{FutureExt, StreamExt};
use reqwest::Url;
use reqwest::{Client, Method, RequestBuilder, Response};
use std::fmt::Debug;
use std::sync::Mutex;


use tokio::sync::OwnedSemaphorePermit;


use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::{
    sync::{broadcast, mpsc},
    time::{interval_at, Instant},
};
// use url::Url;

// impl<'a> PartialOrd<i32> for RpsCounter<'a> {
//     fn partial_cmp(&self, other: &i32) -> Option<std::cmp::Ordering> {
//         (*self.counter).partial_cmp(other)
//     }
// }

// impl<'a> PartialEq<i32> for RpsCounter<'a> {
//     fn eq(&self, other: &i32) -> bool {
//         *self.counter == *other
//     }
// }

#[derive(Debug)]
struct SpyWorker {
    id: usize,
    client: reqwest::Client,
    // rps_success: AtomicU32,
    // rps_error: AtomicU32,
    metrics: ThroughputMetric,
}

impl SpyWorker {
    pub fn new(id: usize) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            // .http2_prior_knowledge()
            .build()
            .unwrap_or_else(|_| panic!("Unable to create reqwest client in worker {id}"));

        Self {
            id,
            client,
            // rps_success: AtomicU32::new(0),
            // rps_error: AtomicU32::new(0),
            metrics: ThroughputMetric::default(),
        }
    }

    // async fn send_request(&self, throttler: impl Throttler) -> Result<Response, RequestBuilder>{

    // }

    fn flush_rps_success_error_counters(&self) -> (u32, u32) {
        todo!()
        // let old_success_rps = self.rps_success.swap(0, Ordering::SeqCst);
        // let old_error_rps = self.rps_error.swap(0, Ordering::SeqCst);

        // (old_success_rps, old_error_rps)
    }

    async fn sending(
        &self,
        url: reqwest::Url,
        response_sender: mpsc::UnboundedSender<(Response, OwnedSemaphorePermit)>,
        mut shutdown: broadcast::Receiver<()>,
        throttler: Arc<ThrottlerSempahores>,
    ) -> Result<(), reqwest::Error> {
        let mut request_builder: RequestBuilder;

        loop {
            tokio::select! {
                result = {
                    request_builder = self.client.request(Method::GET, url.clone());
                    throttler.execute_and_give_permit(
                        RequestBuilder::send,
                        request_builder
                    )
                } => {
                    match result {
                        Ok((out, permit)) => {
                            match out.await {
                                Ok(r) => {
                                    response_sender.send((r, permit)).unwrap();
                                }
                                Err(e) => {
                                    error!("Unable to send request: {}", e);
                                }
                            }
                        },
                        Err(err) => {
                            error!("There was an error while getting sempahore permit: {:?}", err);
                        }
                    }
                }
                _ = shutdown.recv() => {
                    break
                }
            }
        }
        Ok(())
    }
}

// impl Requester for SpyWorker { }

trait Requester {
    fn sending(&self, url: reqwest::Url) -> Result<(), reqwest::Error>;
}

async fn join_handle_choice<T: std::fmt::Debug>(futures: &mut FuturesUnordered<JoinHandle<T>>) {
    while let Some(res) = futures.next().await {
        info!("Worker has finished his job: {:?}", res)
    }
}

async fn receive_responses(
    metric: Arc<Mutex<ThroughputMetric>>,
    mut response_receiver: mpsc::UnboundedReceiver<Response>,
) {
    loop {
        let response = response_receiver.recv().await;
        metric.lock().unwrap().record_response(response.as_ref())
    }
}

async fn reponse_handler(
    mut response_receiver: mpsc::UnboundedReceiver<(Response, OwnedSemaphorePermit)>,
    mut shutdown_receiver: broadcast::Receiver<()>,
    metrics_period_in_secs: u64,
    throttler: Arc<ThrottlerSempahores>,
) {
    let start_next_period = Instant::now() + until_event(metrics_period_in_secs);
    let mut interval = interval_at(
        start_next_period,
        Duration::from_secs(metrics_period_in_secs),
    );

    let mut metrics_store = MetricsStore::default();
    let mut current_metric = ThroughputMetric::default();

    let mut permits: Vec<OwnedSemaphorePermit> = Vec::with_capacity(throttler.threshold() as usize);

    loop {
        tokio::select! {
            _ = interval.tick() => {
                let period_end = round_up_datetime(Utc::now(), metrics_period_in_secs as i64);
                metrics_store.add_record(period_end.timestamp(), current_metric);

                info!("Throttler counted: {:?}", permits.len());
                permits.clear();

                info!("Ended recording interval {}\n{}", period_end, current_metric);
                current_metric.flush_current_metric();
            }
            Some((response, permit)) = response_receiver.recv() => {
                current_metric.record_response(Some(&response));
                permits.push(permit);
            }
            _ = shutdown_receiver.recv() => {
                break
            }
        }
    }
}

pub async fn start_spies(
    concurrent_workers: usize,
    url: Url,
    metrics_period_in_secs: u64,
    throttler: ThrottlerSempahores,
) {
    let mut workers: Vec<Arc<SpyWorker>> = Vec::with_capacity(concurrent_workers);
    let url: reqwest::Url = url;

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (response_sender, response_receiver) =
        mpsc::unbounded_channel::<(Response, OwnedSemaphorePermit)>();
    let throttler = Arc::new(throttler);

    debug!("Starting spy");
    let mut worker_futures: FuturesUnordered<_> = (0..concurrent_workers)
        .map(|id| {
            let spy_worker = Arc::new(SpyWorker::new(id));
            workers.push(Arc::clone(&spy_worker));
            let url = url.clone();
            let shutdown_receiver = shutdown_tx.subscribe();

            tokio::task::spawn({
                let response_sender = response_sender.clone();
                let throttler_clone = throttler.clone();

                async move {
                    spy_worker
                        .sending(url, response_sender, shutdown_receiver, throttler_clone)
                        .await
                }
            })
        })
        .collect();

    let metric_master = tokio::spawn({
        async move {
            reponse_handler(
                response_receiver,
                shutdown_rx,
                metrics_period_in_secs,
                throttler,
            )
            .await
        }
    });

    tokio::select! {
        _ = metric_master => {
            warn!("Termination signal issued from metrics collector");
        }
        _ = join_handle_choice(&mut worker_futures) => {
            warn!("Termination signal issued from worker");
        }
    };
    shutdown_tx.send(()).unwrap();
}
