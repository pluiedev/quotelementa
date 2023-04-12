use std::{
    path::PathBuf,
    process::{Command, Stdio},
};

use eyre::{Context, Result};
use fantoccini::{wd::Capabilities, Client, ClientBuilder, Locator};
use futures_util::{StreamExt, TryStreamExt};
use tracing::*;
use url::Url;

use crate::{
    state::{Output, State},
    JobQueue, ShutdownRx,
};

pub struct Crawler {
    port: u16,
    client: Client,
    pub state: State,

    job_queue: JobQueue,
}
impl Crawler {
    #[tracing::instrument(skip_all, fields(port = port))]
    pub async fn new(
        driver: PathBuf,
        port: u16,
        output: Output,
        job_queue: JobQueue,
        capabilities: Capabilities,
    ) -> Result<Self> {
        info!("Initializing crawler instance");

        let log_path = format!("webdriver-{port}.log");
        let log_file = std::fs::File::create(&log_path)?;
        debug!(?log_path, "WebDriver log file created");

        let driver = Command::new(driver)
            .arg(format!("--port={port}"))
            .stdout(Stdio::from(log_file.try_clone()?))
            .stderr(Stdio::from(log_file))
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
            client,
            state,
            job_queue,
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

        tokio::select! {
            _ = shutdown_rx.changed() => {
                info!("Forcibly shutting down!");
            }
            res = self.client.close() => res?,
        }

        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn crawl_loop(&mut self) -> Result<()> {
        while self.job_queue.available() > 0 {
            info!("Crawler {}: Waiting for work", self.port);

            let site = self.job_queue.pop().await;
            self.crawl(site).await?;
        }

        info!("No work remains! Exiting");
        Ok(())
    }

    #[tracing::instrument(skip_all, fields(url = url.as_str()))]
    async fn crawl(&mut self, url: Url) -> Result<()> {
        info!("Crawler {}: Crawling {}", self.port, url);

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
