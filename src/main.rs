pub mod assigner;
pub mod crawler;
pub mod state;
pub mod tui;

use argh::FromArgs;
use assigner::Assigner;
use crawler::Crawler;
use deadqueue::limited::Queue;
use eyre::Result;
use state::Output;
use tracing_subscriber::{util::SubscriberInitExt, prelude::__tracing_subscriber_SubscriberExt};
use tui_logger::TuiTracingSubscriberLayer;

use std::{path::PathBuf, sync::Arc, time::Duration};
use tokio::{sync::watch, task::JoinSet};
use tracing::*;

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

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .finish()
        .with(TuiTracingSubscriberLayer)
        .init();

    let opts: Opts = argh::from_env();

    // let (tx, rx) = watch::channel(());
    // ctrlc::set_handler(move || tx.send(()).unwrap())?;

    std::thread::spawn(tui::tui);

    info!("Haha");

    tokio::time::sleep(Duration::from_secs(10000)).await;


    // let mut caps = serde_json::map::Map::new();
    // if !opts.no_headless {
    //     let args = serde_json::json!({
    //         "args": ["--headless", "--disable-gpu"]
    //     });
    //     caps.insert("moz:firefoxOptions".to_owned(), args.clone());
    //     caps.insert("goog:chromeOptions".to_owned(), args.clone());
    // }

    // let queue = Arc::new(Queue::<String>::new(10));
    // let mut set = JoinSet::new();
    // let output = Output::default();

    // for port in 0..opts.workers {
    //     let driver = opts.driver.clone();
    //     let queue = Arc::clone(&queue);
    //     let rx = rx.clone();
    //     let caps = caps.clone();
    //     let output = output.clone();

    //     set.spawn(async move {
    //         let crawler = Crawler::new(&driver, opts.base_port + port, output, queue, caps)
    //             .await
    //             .unwrap();
    //         crawler.run(rx).await
    //     });
    // }

    // let assigner = Assigner::new(&opts.sites, queue.clone()).await?;
    // tokio::spawn(assigner.run(rx));


    // loop {
    //     tokio::select! {
    //         Some(_) = set.join_next() => {}
    //         else => {
    //             info!("Everything done! Exiting...");
    //             break;
    //         }
    //     }
    // }

    // info!(?output);
    Ok(())
}
