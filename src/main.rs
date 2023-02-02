//use anyhow::bail;
//use std::ops::Bound::Included;
use clap::{Parser, Subcommand};
use rapier::start_spies;

#[macro_use] extern crate log;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
        #[arg(short, long, value_name = "URL")]
        pub url: reqwest::Url,

        #[arg(short, long, value_name="CONCURRENT_WORKERS", default_value_t = 1)]
        pub concurrent_workers: usize,

        #[arg(short, long, value_name="THROTTLE")]
        pub throttle: Option<u64>,

        #[arg(short, long, value_name="METRICS_RECORDING_INTERVAL", default_value_t = 10)]
        pub metrics_recording_interval: u64
}



#[tokio::main]
async fn main() {
    pretty_env_logger::init();

    let cli = Cli::parse();
    info!("Spy was started with following configuration: {cli:?}");

    start_spies(cli.concurrent_workers, cli.url, cli.metrics_recording_interval).await;
    // let throttler = match cli.throttle {
    //     Some(throttle) => Arc::new(Semaphore::new(throttle as usize)),
    //     _ => Arc::new(Semaphore::new(0)),
    // };
}

