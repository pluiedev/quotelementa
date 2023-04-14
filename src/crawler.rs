use std::{fmt::Display, path::PathBuf, process::Stdio};

use eyre::{Context, Result};
use fantoccini::{wd::Capabilities, Client, ClientBuilder, Locator};
use futures_util::{StreamExt, TryStreamExt};
use tokio::{process::{Command, Child}, sync::mpsc};
use tracing::*;
use url::Url;

use crate::{
    state::{Output, State},
    util::Port,
    JobQueue, ShutdownRx,
};

#[derive(Clone, Debug)]
pub struct CrawlerReport {
    pub port: Port,
    pub state: CrawlerState,
}
#[derive(Clone, Debug)]
pub enum CrawlerState {
    Initializing,
    InProgress(String),
    Idle,
    Done,
    ShuttingDown,
}
impl Display for CrawlerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initializing => write!(f, "Initializing..."),
            Self::InProgress(url) => write!(f, "{url}"),
            Self::Idle => write!(f, "Idle"),
            Self::Done => write!(f, "Done"),
            Self::ShuttingDown => write!(f, "Shutting down..."),
        }
    }
}

pub struct Crawler {
    port: Port,
    _driver: Child,
    client: Client,
    pub state: State,

    job_queue: JobQueue,
    report_tx: mpsc::Sender<CrawlerReport>,
}
impl Crawler {
    #[tracing::instrument(skip_all, fields(port = port))]
    pub async fn new(
        driver: PathBuf,
        port: Port,
        output: Output,
        job_queue: JobQueue,
        capabilities: Capabilities,
        report_tx: mpsc::Sender<CrawlerReport>,
    ) -> Result<Self> {
        info!("Initializing crawler instance");
        report_tx
            .send(CrawlerReport {
                port,
                state: CrawlerState::Initializing,
            })
            .await
            .expect("UI should still be alive");

        let log_path = format!("webdriver-{port}.log");
        let log_file = std::fs::File::create(&log_path)?;
        debug!(?log_path, "WebDriver log file created");

        let driver = Command::new(driver)
            .arg(format!("--port={port}"))
            .stdout(Stdio::from(log_file.try_clone()?))
            .stderr(Stdio::from(log_file))
            .kill_on_drop(true)
            .spawn()?;
        debug!(id = driver.id(), "WebDriver spawned");

        let url = format!("http://localhost:{port}");
        let client = ClientBuilder::native()
            .capabilities(capabilities)
            .connect(&url)
            .await
            .wrap_err("failed to connect to WebDriver!")?;

        info!(?url, "Crawler instance initialized");

        client.set_ua("Quotelementa-Crawler").await?;

        let state = State::new(output, &client).await?;

        Ok(Self {
            port,
            _driver: driver,
            client,
            state,
            job_queue,
            report_tx,
        })
    }

    #[tracing::instrument(skip_all, fields(port = self.port))]
    pub async fn run(mut self, mut shutdown_rx: ShutdownRx) -> Result<()> {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    info!("Shutdown received - exiting");
                    break;
                }
                res = self.crawl_loop() => match res {
                    Ok(()) => break,
                    Err(e) => return Err(e),
                }
            }
        }

        self.report_tx
            .send(CrawlerReport {
                port: self.port,
                state: CrawlerState::ShuttingDown,
            })
            .await?;

        tokio::select! {
            _ = shutdown_rx.changed() => {
                info!("Forcibly shutting down!");
            }
            res = self.client.close() => res?,
        }

        self.report_tx
            .send(CrawlerReport {
                port: self.port,
                state: CrawlerState::Done,
            })
            .await?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn crawl_loop(&mut self) -> Result<()> {
        while self.job_queue.available() > 0 {
            info!(?self.port, "Waiting for work");

            self.report_tx
                .send(CrawlerReport {
                    port: self.port,
                    state: CrawlerState::Idle,
                })
                .await?;

            let site = self.job_queue.pop().await;
            self.crawl(site).await?;
        }

        info!("No work remains - I'm done!");
        Ok(())
    }

    #[tracing::instrument(skip_all, fields(url = url.as_str()))]
    async fn crawl(&mut self, url: Url) -> Result<()> {
        info!(?url, ?self.port, "Start crawling");

        self.report_tx
            .send(CrawlerReport {
                port: self.port,
                state: CrawlerState::InProgress(
                    url.as_str().trim_start_matches("https://").to_owned(),
                ),
            })
            .await?;

        self.client
            .goto(url.as_str())
            .await
            .wrap_err("Failed to navigate to site")?;

        let element = self
            .client
            .find(Locator::Css("body"))
            .await
            .wrap_err("No body element found - how?")?;
        let elements = element
            .find_all(Locator::Css("*"))
            .await
            .wrap_err("Looks like body element is empty?")?;

        self.state = futures_util::stream::iter(elements)
            .map(Ok::<_, eyre::Report>)
            .try_fold(std::mem::take(&mut self.state), State::accept_node)
            .await?;

        // info!("Crawling complete");
        Ok(())
    }
}
