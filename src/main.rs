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
use tracing_subscriber::util::SubscriberInitExt;
use util::{Capabilities, JobQueue, Port};

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

    let (mut crawlers, report_rx) = Crawlers::new(&opts, shutdown_rx.clone());

    for _ in 0..opts.workers {
        crawlers.spawn();
    }

    let (assigner, sites_count) = Assigner::new(&opts.sites, crawlers.job_queue.clone()).await?;
    tokio::spawn(assigner.run(shutdown_rx));

    let tui = Tui::new(App::new(
        crawlers.output.clone(),
        report_rx,
        sites_count,
        shutdown_tx,
    ))?;
    let tui = tokio::spawn(tui.run(close_rx));

    while let Some(res) = crawlers.set.join_next().await {
        if let Err((respawn, e)) = res? {
            error!(?e, "Encountered error while crawling");
            if respawn {
                warn!(?e, "Attempting to respawn");
                crawlers.spawn();
            }
        }
    }

    info!("Everything done! Waiting for UI to stop...");

    close_tx.send(()).unwrap();
    tui.await??;

    Ok(())
}

fn make_capabilities(opts: &Opts) -> Capabilities {
    let mut caps = Capabilities::new();
    if !opts.no_headless {
        caps.insert(
            "moz:firefoxOptions".to_owned(),
            serde_json::json!({
                "args": ["--headless"]
            }),
        );
        caps.insert(
            "goog:chromeOptions".to_owned(),
            serde_json::json!({
                "args": ["--headless=new", "--disable-gpu"]
            }),
        );
    }
    caps
}

struct Crawlers {
    set: JoinSet<Result<(), (bool, eyre::Report)>>,

    driver: PathBuf,
    port: Port,
    output: Output,
    job_queue: JobQueue,
    caps: Capabilities,
    report_tx: mpsc::Sender<CrawlerReport>,
    shutdown_rx: ShutdownRx,
}
impl Crawlers {
    fn new(opts: &Opts, shutdown_rx: ShutdownRx) -> (Self, mpsc::Receiver<CrawlerReport>) {
        let double_workers = usize::from(opts.workers * 2);
        let (report_tx, report_rx) = mpsc::channel(double_workers);
        let job_queue = Arc::new(Queue::new(double_workers));

        (
            Self {
                set: JoinSet::new(),
                caps: make_capabilities(&opts),
                report_tx,
                job_queue,
                output: Output::default(),
                driver: opts.driver.clone(),
                port: opts.base_port,
                shutdown_rx,
            },
            report_rx,
        )
    }
    fn spawn(&mut self) {
        let crawler = Crawler::new(
            self.driver.clone(),
            self.port,
            self.output.clone(),
            self.job_queue.clone(),
            self.caps.clone(),
            self.report_tx.clone(),
        );
        let rx = self.shutdown_rx.clone();

        self.set.spawn(async move {
            match crawler.await {
                Ok(c) => c.run(rx).await.map_err(|e| (false, e)),
                Err(e) => Err((true, e)),
            }
        });
        self.port += 1;
    }
}
