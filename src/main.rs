pub mod assigner;
pub mod crawler;
pub mod state;
pub mod tui;

use argh::FromArgs;
use deadqueue::limited::Queue;
use eyre::Result;
use state::State;
use tracing_subscriber::{prelude::__tracing_subscriber_SubscriberExt, util::SubscriberInitExt};
use tui_logger::TuiTracingSubscriberLayer;

use std::{path::PathBuf, sync::Arc};
use tokio::{
    sync::{oneshot, watch},
    task::JoinSet,
};
use tracing::*;

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

type Workers = JoinSet<Result<State>>;
type JobQueue = Arc<Queue<String>>;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(TuiTracingSubscriberLayer)
        .init();

    let opts: Opts = argh::from_env();

    let (shutdown_tx, shutdown_rx) = watch::channel(());
    let (close_tx, close_rx) = oneshot::channel();

    let tui = Tui::new()?;
    tokio::spawn(tui.run(shutdown_tx, close_rx));

    let (mut workers, job_queue, output) = spawn_workers(&opts, &shutdown_rx);

    let assigner = Assigner::new(&opts.sites, job_queue.clone()).await?;
    tokio::spawn(assigner.run(shutdown_rx));

    while let Some(_) = workers.join_next().await {}

    info!("Everything done! Exiting...");
    close_tx.send(()).unwrap();
    info!(?output);

    Ok(())
}

fn spawn_workers(opts: &Opts, rx: &ShutdownRx) -> (Workers, JobQueue, Output) {
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
    let output = Output::default();

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

    (workers, job_queue, output)
}
