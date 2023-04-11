use std::{
    path::Path,
    process::{Command, Stdio},
    sync::Arc,
};

use deadqueue::limited::Queue;
use eyre::{Context, Result};
use fantoccini::{wd::Capabilities, Client, ClientBuilder, Locator};
use futures_util::{StreamExt, TryStreamExt};
use tracing::*;

use crate::{state::State, ShutdownRx};

pub struct Crawler {
    port: u16,
    client: Client,
    pub state: State,

    queue: Arc<Queue<String>>,
}
impl Crawler {
    #[tracing::instrument(skip_all, fields(port = port))]
    pub async fn new(
        driver: &Path,
        port: u16,
        queue: Arc<Queue<String>>,
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

        let state = State::new(&client).await?;

        Ok(Self {
            port,
            client,
            state,
            queue,
        })
    }

    #[tracing::instrument(skip_all, fields(port = self.port))]
    pub async fn run(mut self, mut shutdown_rx: ShutdownRx) -> Result<State> {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    info!("Shutdown received - exiting");
                    break;
                }
                res = self.crawl_loop() => match res {
                    Ok(()) => break,
                    Err(e) => error!("Error while crawling site: {e}")
                }
            }
        }
        self.client.close().await?;

        Ok(self.state)
    }

    #[tracing::instrument(skip(self))]
    async fn crawl_loop(&mut self) -> Result<()> {
        while self.queue.available() > 0 {
            info!(?self.port, "Crawler waiting for work");

            let site = self.queue.pop().await;
            self.crawl(site).await?;
        }

        info!("No work remains! Exiting");
        Ok(())
    }

    #[tracing::instrument(skip(self))]
    async fn crawl(&mut self, site: String) -> Result<()> {
        info!("Crawling started");

        if let Err(e) = self.client.goto(&site).await {
            error!(?site, "Failed to navigate to site: {e}");
            return Ok(());
        }

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

        info!("Crawling complete");
        Ok(())
    }
}
