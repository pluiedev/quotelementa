use std::{fmt::Display, path::PathBuf, process::Stdio};

use eyre::{Context, Result};
use fantoccini::{wd::Capabilities, Client, ClientBuilder, Locator};
use futures_util::{StreamExt, TryStreamExt};
use tokio::{
    process::{Child, Command},
    sync::mpsc,
};
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
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CrawlerState {
    Initializing,
    InProgress(String),
    Complete,
    ShuttingDown,
    Terminated,
}
impl Display for CrawlerState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Initializing => write!(f, "Initializing..."),
            Self::InProgress(url) => write!(f, "{url}"),
            Self::Complete => write!(f, "Complete!"),
            Self::ShuttingDown => write!(f, "Shutting down..."),
            Self::Terminated => write!(f, "Terminated"),
        }
    }
}

pub struct Crawler {
    port: Port,
    driver: Child,
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

        match Self::init_session(port, driver, capabilities, output).await {
            Ok((driver, client, state)) => Ok(Self {
                port,
                driver,
                client,
                state,
                job_queue,
                report_tx,
            }),
            Err(e) => {
                report_tx
                    .send(CrawlerReport {
                        port,
                        state: CrawlerState::Terminated,
                    })
                    .await
                    .expect("UI should still be alive");
                Err(e)
            }
        }
    }
    async fn init_session(
        port: Port,
        driver: PathBuf,
        capabilities: Capabilities,
        output: Output,
    ) -> Result<(Child, Client, State)> {
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

        Ok((driver, client, state))
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

        self.driver.start_kill()?;
        self.report_tx
            .send(CrawlerReport {
                port: self.port,
                state: CrawlerState::Terminated,
            })
            .await?;

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn crawl_loop(&mut self) -> Result<()> {
        while let Some(site) = self.job_queue.try_pop() {
            if let Err(e) = self.crawl(site).await {
                error!(%e, "Error while crawling");
            }

            self.report_tx
                .send(CrawlerReport {
                    port: self.port,
                    state: CrawlerState::Complete,
                })
                .await?;
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
