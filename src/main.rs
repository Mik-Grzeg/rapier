use anyhow::bail;
use clap::{Parser, Subcommand};
use futures::stream::FuturesUnordered;
use tokio::select;
use tokio::sync::broadcast::Sender;
use tokio::sync::broadcast::error::TryRecvError;
use tokio::sync::broadcast::{Receiver, self};
use std::sync::atomic::{AtomicU32, Ordering};
use url::Url;
use tokio::task::JoinHandle;
use tokio::{task, sync::Semaphore};
use tokio::time::{sleep, Duration, interval, Instant};
use std::sync::{Mutex, Arc};
use reqwest::{ClientBuilder, Client, Request, RequestBuilder, Method};
use futures::StreamExt;


extern crate pretty_env_logger;
#[macro_use] extern crate log;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
        #[arg(short, long, value_name = "URL")]
        pub url: Url,

        #[arg(short, long, value_name="CONCURRENT_WORKERS", default_value_t = 1)]
        pub concurrent_workers: usize,

        #[arg(short, long, value_name="THROTTLE")]
        pub throttle: Option<u64>,

        #[arg(short, long, value_name="RPS_REPORT_INTERVAL_IN_SECONDS", default_value_t = 10)]
        pub rps_report_interval_in_secs: usize
}

fn report_rps(workers: &Vec<Arc<SpyWorker>>, rps_flush_interval_duration: Duration) {
    let (sum_success, sum_errors) = workers
        .iter()
        .fold((0, 0), |acc, worker| (acc.0 + worker.rps_success.load(Ordering::SeqCst), acc.1 + worker.rps_error.load(Ordering::SeqCst)));

    info!("\n\n");
    info!("============================================");
    info!("==   RPS REPORT (average of {} seconds)   ==", rps_flush_interval_duration.as_secs());
    info!("============================================");
    info!("Success rps: {sum_success}");
    info!("Error rps:   {sum_errors}\n");

    workers
        .iter()
        .for_each(|worker| {
            let (rps_success, rps_error) = worker.flush_rps_success_error_counters();
            info!("[Worker : {}] rps | SUCCESS: {} | ERROR: {} |", worker.id, rps_success, rps_error);
        });
}

async fn run_report_rps(workers: Vec<Arc<SpyWorker>>, rps_flush_interval_duration: Duration, mut shutdown: broadcast::Receiver<()>) {
    let mut interval = interval(Duration::from_secs(10));
    interval.tick().await;
    loop {
        tokio::select! {
            _ = interval.tick() => {
                report_rps(&workers, rps_flush_interval_duration);
            }
            _ = shutdown.recv() => {
                info!("Reporter has finished his job");
                return
            }
        }
    }
}


#[derive(Debug, Clone, Copy)]
struct Flush{}


#[derive(Debug)]
struct SpyWorker {
    id: usize,
    client: reqwest::Client,
    rps_success: AtomicU32,
    rps_error: AtomicU32,
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
            rps_success: AtomicU32::new(0),
            rps_error: AtomicU32::new(0),
        }
    }

    fn flush_rps_success_error_counters(&self) -> (u32, u32) {
        let old_success_rps = self.rps_success.swap(0, Ordering::SeqCst);
        let old_error_rps = self.rps_error.swap(0, Ordering::SeqCst);

        (old_success_rps, old_error_rps)
    }


    async fn sending(&self, url: Url, mut shutdown: broadcast::Receiver<()>) -> Result<(), reqwest::Error> {
        loop {
            select! {
                res = {

                    let url = url.clone();
                    self.client.request(Method::GET, url).send()
                 } => {
                    let status = res?.status();
                    trace!("Next iteration in worker: {}", self.id);

                    if !status.is_success() {
                        trace!("[SpyWorker: {}] - got {}", self.id, status.as_u16());
                        self.rps_error.fetch_add(1, Ordering::SeqCst);
                    } else {
                        self.rps_success.fetch_add(1, Ordering::SeqCst);
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
    fn sending(&self, url: Url) -> Result<(), reqwest::Error>;
}

async fn join_handle_choice<T: std::fmt::Debug>(futures: &mut FuturesUnordered<JoinHandle<T>>) {
    while let Some(res) = futures.next().await {
        info!("Worker has finished his job: {:?}", res)
    };
}

#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let cli = Cli::parse();
    info!("Spy was started with following configuration: {cli:?}");

    let throttler = match cli.throttle {
        Some(throttle) => Arc::new(Semaphore::new(throttle as usize)),
        _ => Arc::new(Semaphore::new(0)),
    };

    let mut workers: Vec<Arc<SpyWorker>> = Vec::with_capacity(cli.concurrent_workers);


    let (shutdown_send, shutdown_recv) = broadcast::channel::<()>(1);

    debug!("Starting spy");
    let mut futures: FuturesUnordered<_>  = (0..cli.concurrent_workers)
        .map(|id| {
            let spy_worker = Arc::new(SpyWorker::new(id));
            workers.push(Arc::clone(&spy_worker));
            let url = cli.url.clone();
            let rx = shutdown_send.subscribe();

            tokio::spawn(async move {
                spy_worker.sending(url, rx).await
            })
        })
        .collect();

    let reporter = task::spawn( run_report_rps(
        workers,
        Duration::from_secs(cli.rps_report_interval_in_secs.try_into().unwrap()),
        shutdown_recv
    ));

    tokio::select! {
        _ = join_handle_choice(&mut futures) => {
            info!("Termination signal issued from worker");
        }
        _ = reporter => {
            info!("Termination signal issued from reporter");
        }
    };

    shutdown_send.send(());
}
