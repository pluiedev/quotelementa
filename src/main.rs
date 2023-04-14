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
mod util;

use argh::FromArgs;
use crawler::CrawlerReport;
use deadqueue::limited::Queue;
use eyre::Result;
use futures_util::TryFutureExt;
use tracing_subscriber::util::SubscriberInitExt;
use url::Url;
use util::Port;

use std::{path::PathBuf, sync::Arc};
use tokio::{
    sync::{mpsc, oneshot, watch},
    task::JoinSet,
};
use tracing::{error, info, warn};

use crate::{
    assigner::Assigner,
    crawler::Crawler,
    state::Output,
    tui::{App, Tui},
    util::ShutdownRx,
};

/// Crawls the interwebs and analyzes the utilization of elemental constituents
#[derive(FromArgs)]
struct Opts {
    /// the number of workers running concurrently
    #[argh(option, short = 'n', default = "3")]
    workers: Port,

    /// the base port
    #[argh(option, short = 'p', default = "4444")]
    base_port: Port,

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
type JobQueue = Arc<Queue<Url>>;

#[tokio::main]
async fn main() -> Result<()> {
    let appender = tracing_appender::rolling::daily(".", "quotelementa.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(appender);

    tracing_subscriber::fmt()
        .with_ansi(false)
        .with_writer(non_blocking)
        .finish()
        .init();

    let opts: Opts = argh::from_env();

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let (close_tx, close_rx) = oneshot::channel();

    let (mut crawlers, job_queue, output, report_rx) = spawn_crawlers(&opts, &shutdown_rx);

    let tui = Tui::new(App::new(output.clone(), report_rx, shutdown_tx))?;
    let tui = tokio::spawn(tui.run(close_rx));

    let assigner = Assigner::new(&opts.sites, job_queue.clone()).await?;
    tokio::spawn(assigner.run(shutdown_rx));

    while let Some(res) = crawlers.join_next().await {
        if let Err(e) = res? {
            error!(?e, "Encountered error while crawling")
        }
    }

    info!("Everything done! Waiting for UI to stop...");
    info!(?output);

    close_tx.send(()).unwrap();
    tui.await??;

    Ok(())
}

fn spawn_crawlers(
    opts: &Opts,
    rx: &ShutdownRx,
) -> (Workers, JobQueue, Output, mpsc::Receiver<CrawlerReport>) {
    let mut caps = serde_json::map::Map::new();
    if !opts.no_headless {
        caps.insert("moz:firefoxOptions".to_owned(), serde_json::json!({
            "args": ["--headless"]
        }));
        caps.insert("goog:chromeOptions".to_owned(), serde_json::json!({
            "args": ["--headless=new", "--disable-gpu"]
        }));
    }

    let double_workers = usize::from(opts.workers * 2);
    let (report_tx, report_rx) = mpsc::channel(double_workers);

    let job_queue = Arc::new(Queue::new(double_workers));
    let mut workers = JoinSet::new();
    let output = Output::default();

    for port in 0..opts.workers {
        let rx = rx.clone();

        workers.spawn(
            Crawler::new(
                opts.driver.clone(),
                opts.base_port + port,
                output.clone(),
                job_queue.clone(),
                caps.clone(),
                report_tx.clone(),
            )
            .and_then(|c| c.run(rx)),
        );
    }

    (workers, job_queue, output, report_rx)
}
