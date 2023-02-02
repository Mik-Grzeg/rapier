use super::metrics::{ThroughputMetric, Reporter, MetricsStore};
use reqwest::{Client, Response, Method};
use std::time::Duration;
use tokio::task::JoinHandle;
use tokio::{sync::{broadcast, mpsc}, time::{Interval, interval_at, Instant}};
use chrono::Utc;
use url::Url;
use futures::{stream::FuturesUnordered, future::select};
use std::sync::Arc;
use futures::StreamExt;
use crate::fixed_window::{until_event, round_up_datetime};


#[derive(Debug)]
struct SpyWorker {
    id: usize,
    client: reqwest::Client,
    // rps_success: AtomicU32,
    // rps_error: AtomicU32,
    metrics: ThroughputMetric
}


impl SpyWorker {
    pub fn new(id: usize) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(1))
            // .http2_prior_knowledge()
            .build().expect(&format!("Unable to create reqwest client in worker {}", id));

        Self {
            id,
            client,
            // rps_success: AtomicU32::new(0),
            // rps_error: AtomicU32::new(0),
            metrics: ThroughputMetric::default()
        }
    }

    fn flush_rps_success_error_counters(&self) -> (u32, u32) {
        todo!()
        // let old_success_rps = self.rps_success.swap(0, Ordering::SeqCst);
        // let old_error_rps = self.rps_error.swap(0, Ordering::SeqCst);

        // (old_success_rps, old_error_rps)
    }

    async fn sending(&self, url: reqwest::Url, response_sender: mpsc::UnboundedSender<Response>, mut shutdown: broadcast::Receiver<()>) -> Result<(), reqwest::Error> {
        let mut metrics = ThroughputMetric::default();
        loop {
            tokio::select! {
                res = {
                    self.client.request(Method::GET, url.clone()).send()
                 } => {
                     match res {

                        Ok(r) => response_sender.send(r).unwrap(),
                        Err(e) => {
                            error!("Unable to send request: {}", e);
                        }
                     }
                  }
                _ = shutdown.recv() => {
                    info!("Shutting down worker {}", self.id);
                    break;
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
    };
}

async fn reponse_handler(mut response_receiver: mpsc::UnboundedReceiver<Response>, mut shutdown_receiver: broadcast::Receiver<()>, metrics_period_in_secs: u64) {
    let start_next_period = Instant::now() + until_event(metrics_period_in_secs);
    let mut interval = interval_at(start_next_period, Duration::from_secs(metrics_period_in_secs));

    let mut metrics_store = MetricsStore::default();
    let mut current_metric = ThroughputMetric::default();
    loop {
        tokio::select! {
            _ = interval.tick() => {
                let period_end = round_up_datetime(Utc::now(), metrics_period_in_secs as i64);
                metrics_store.add_record(period_end.timestamp(), current_metric);
                info!("Ended recording interval {}\nWith following stats:\n{}", period_end, current_metric);
                current_metric.flush_current_metric();

            }
            response = response_receiver.recv() => {
                current_metric.record_response(response.as_ref());
            }
            _ = shutdown_receiver.recv() => {
                break
            }
        }
    }
}

pub async fn start_spies(concurrent_workers: usize, url: Url, metrics_period_in_secs: u64) {
    let mut workers: Vec<Arc<SpyWorker>> = Vec::with_capacity(concurrent_workers);
    let url: reqwest::Url = url.into();

    let (shutdown_tx, shutdown_rx) = broadcast::channel(1);
    let (response_sender, response_receiver) = mpsc::unbounded_channel::<Response>();

    debug!("Starting spy");
    let mut worker_futures: FuturesUnordered<_>  = (0..concurrent_workers)
        .map(|id| {
            let spy_worker = Arc::new(SpyWorker::new(id));
            workers.push(Arc::clone(&spy_worker));
            let url = url.clone();
            let shutdown_receiver = shutdown_tx.subscribe();

            tokio::task::spawn({
                let response_sender = response_sender.clone();
                async move {
                    spy_worker.sending(url, response_sender, shutdown_receiver).await
                }
            })
        })
        .collect();

    let metric_master = tokio::spawn({
        async move {
            reponse_handler(response_receiver, shutdown_rx, metrics_period_in_secs).await
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
