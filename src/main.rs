use deadqueue::limited::Queue;
use eyre::{Context, Result};
use fantoccini::{elements::Element, Client, ClientBuilder, Locator};
use futures_util::{StreamExt, TryStreamExt};
use std::{collections::HashMap, process::Stdio, sync::Arc};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader},
    process::Command,
    sync::watch,
    task::JoinSet,
};
use tracing::*;

#[derive(Clone, Default)]
struct ElementProcessor {
    freq: HashMap<String, usize>,
    window_width: u64,
    window_height: u64,
}
impl ElementProcessor {
    async fn new(c: &Client) -> Result<Self> {
        let (window_width, window_height) = c.get_window_size().await?;
        Ok(Self {
            freq: Default::default(),
            window_height,
            window_width,
        })
    }
    async fn process_node(mut self, v: Element) -> Result<Self> {
        let Ok(name) = v.tag_name().await else {
            warn!(?v, "Unable to get name for element - perhaps it has already been removed from the DOM?");
            return Ok(self);
        };

        if name.contains('-') {
            trace!(?name, "Found web component - ignoring");
            return Ok(self);
        }

        match name.as_str() {
            "div" => {
                let (x, y, w, h) = v.rectangle().await?;
                let w = w / self.window_width as f64;
                let h = h / self.window_height as f64;

                trace!("Found div element ({x:.2}, {y:.2}) {w:.2} x {h:.2}");
            }
            _ => {}
        }

        *self.freq.entry(name).or_default() += 1;
        Ok(self)
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let (tx, rx) = watch::channel(());
    ctrlc::set_handler(move || tx.send(()).unwrap())?;

    let queue = Arc::new(Queue::<String>::new(10));

    let mut set = JoinSet::new();

    for port in 4444..=4446 {
        let queue = Arc::clone(&queue);
        let rx = rx.clone();
        set.spawn(async move {
            let instance = Instance::new(port, queue).await.unwrap();
            instance.run(rx).await
        });
    }

    let mut sites = BufReader::new(File::open("top-1m.csv").await?);
    let mut buf = String::new();

    loop {
        tokio::select! {
            Some(Ok(res)) = set.join_next() => {
                let res: ElementProcessor = res?;
                info!(?res.freq);
            }
            line = sites.read_line(&mut buf), if queue.len() < 10 => {
                _ = line?;
                let Some((_, site)) = buf.split_once(",") else { continue; };
                let site = format!("https://{}", site.trim());

                queue.push(site).await;
                buf.clear();
            }
            else => {
                break;
            }
        }
    }

    Ok(())
}

struct Instance {
    port: u32,
    client: Client,
    processor: ElementProcessor,

    queue: Arc<Queue<String>>,
}
impl Instance {
    #[tracing::instrument(skip(queue))]
    async fn new(port: u32, queue: Arc<Queue<String>>) -> Result<Self> {
        info!("Initializing crawler instance");

        let log_path = format!("webdriver-{port}.log");
        let log_file = std::fs::File::create(&log_path)?;
        debug!(?log_path, "WebDriver log file created");

        let driver = Command::new("geckodriver")
            .arg("-p")
            .arg(port.to_string())
            .stdout(Stdio::from(log_file.try_clone()?))
            .stderr(Stdio::from(log_file))
            .spawn()?;
        debug!(id = driver.id(), "WebDriver spawned");

        let url = format!("http://localhost:{port}");
        let client = ClientBuilder::native()
            .connect(&url)
            .await
            .wrap_err("failed to connect to WebDriver!")?;

        info!(?url, "Crawler instance initialized");

        client.set_ua("Quotelementa-Crawler").await?;

        let processor = ElementProcessor::new(&client).await?;

        Ok(Self {
            port,
            client,
            processor,
            queue,
        })
    }

    #[tracing::instrument(skip_all, fields(port = self.port))]
    async fn run(mut self, mut shutdown_rx: watch::Receiver<()>) -> Result<ElementProcessor> {
        loop {
            tokio::select! {
                _ = shutdown_rx.changed() => {
                    info!("Shutdown received (work either exhausted or aborted) - exiting");
                    break;
                }
                Err(e) = self.crawl_loop() => error!("Error while crawling site: {e}")
            }
        }
        self.client.close().await?;

        Ok(self.processor)
    }

    #[tracing::instrument(skip(self))]
    async fn crawl_loop(&mut self) -> Result<()> {
        loop {
            info!(?self.port, "Crawler waiting for work");

            let site = self.queue.pop().await;
            self.crawl(site).await?;
        }
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

        self.processor = futures_util::stream::iter(elements)
            .map(Ok::<_, eyre::Report>)
            .try_fold(
                std::mem::take(&mut self.processor),
                ElementProcessor::process_node,
            )
            .await?;

        info!(?self.processor.freq, "Crawling complete");
        Ok(())
    }
}
