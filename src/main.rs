use eyre::{Context, Result};
use fantoccini::{elements::Element, ClientBuilder, Locator, Client};
use futures_util::{StreamExt, TryStreamExt};
use std::collections::HashMap;
use tracing::info;

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
        let name = v.tag_name().await?;

        match name.as_str() {
            "div" => {
                let (x, y, w, h) = v.rectangle().await?;
                let w = w / self.window_width as f64;
                let h = h / self.window_height as f64;
                
                info!("({x}, {y}) {w} x {h}");
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

    let c = ClientBuilder::native()
        .connect("http://localhost:4444")
        .await
        .wrap_err("failed to connect to WebDriver!")?;

    c.set_ua("Quotelementa-Crawler").await?;

    c.goto("https://pluie.me").await?;

    let element = c.find(Locator::Css("body")).await?;
    let elements = element.find_all(Locator::Css("*")).await?;

    let processor = ElementProcessor::new(&c).await?;
    let processor = futures_util::stream::iter(elements)
        .map(Ok::<_, eyre::Report>)
        .try_fold(processor, ElementProcessor::process_node)
        .await?;

    info!(?processor.freq);

    c.close().await?;
    Ok(())
}