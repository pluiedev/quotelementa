#![deny(rust_2018_idioms)]
#![warn(clippy::pedantic)]
#![allow(
    missing_docs,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::wildcard_imports
)]

pub mod assigner;
pub mod crawler;
pub mod state;
pub mod tui;

use argh::FromArgs;
use deadqueue::limited::Queue;
use eyre::Result;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};
use tui_logger::TuiTracingSubscriberLayer;

use std::{path::PathBuf, sync::Arc};
use tokio::{
    sync::{oneshot, watch},
    task::JoinSet,
};
use tracing::{info, warn};

use crate::{assigner::Assigner, crawler::Crawler, state::Output, tui::Tui};

type ShutdownRx = watch::Receiver<()>;

/// Crawls the interwebs and analyzes the utilization of elemental constituents
#[derive(FromArgs)]
struct Opts {
    /// the number of workers running concurrently
    #[argh(option, short = 'n', default = "3")]
    workers: u16,

    /// the base port
    #[argh(option, short = 'p', default = "4444")]
    base_port: u16,

    /// do not run the WebDriver in headless mode
    /// (GeckoDriver and ChromeDriver only)
    #[argh(switch)]
    no_headless: bool,

    /// the WebDriver binary to be run.
    #[argh(positional)]
    driver: PathBuf,

    /// a file containing a list of sites to crawl
    #[argh(positional)]
    sites: PathBuf,
}

type Workers = JoinSet<Result<()>>;
type JobQueue = Arc<Queue<String>>;

#[tokio::main]
async fn main() -> Result<()> {
    let appender = tracing_appender::rolling::daily(".", "quotelementa.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(appender);

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(non_blocking)
        .finish()
        .with(TuiTracingSubscriberLayer)
        .init();

    let opts: Opts = argh::from_env();

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let (close_tx, close_rx) = oneshot::channel();

    let output = Output::default();

    let tui = Tui::new(output.clone())?;
    tokio::spawn(tui.run(shutdown_tx, close_rx));

    let (mut workers, job_queue) = spawn_workers(&opts, &shutdown_rx, &output);

    let assigner = Assigner::new(&opts.sites, job_queue.clone()).await?;
    tokio::spawn(assigner.run(shutdown_rx));

    while workers.join_next().await.is_some() {}

    info!("Everything done! Exiting...");
    close_tx.send(()).unwrap();
    info!(?output);

    Ok(())
}

fn spawn_workers(opts: &Opts, rx: &ShutdownRx, output: &Output) -> (Workers, JobQueue) {
    let mut caps = serde_json::map::Map::new();
    if !opts.no_headless {
        let args = serde_json::json!({
            "args": ["--headless", "--disable-gpu"]
        });
        caps.insert("moz:firefoxOptions".to_owned(), args.clone());
        caps.insert("goog:chromeOptions".to_owned(), args);
    }

    let job_queue = Arc::new(Queue::<String>::new(10));
    let mut workers = JoinSet::new();

    for port in 0..opts.workers {
        let driver = opts.driver.clone();
        let port = opts.base_port + port;
        let output = output.clone();
        let job_queue = Arc::clone(&job_queue);
        let caps = caps.clone();
        let rx = rx.clone();

        workers.spawn(async move {
            let crawler = Crawler::new(&driver, port, output, job_queue, caps)
                .await
                .unwrap();
            crawler.run(rx).await
        });
    }

    (workers, job_queue)
}
