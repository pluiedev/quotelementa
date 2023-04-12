use std::path::Path;

use eyre::Result;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader, Lines},
};
use url::Url;

use crate::{JobQueue, ShutdownRx};

pub struct Assigner {
    source: Lines<BufReader<File>>,
    queue: JobQueue,
}
impl Assigner {
    pub async fn new(source: &Path, queue: JobQueue) -> Result<Self> {
        let source = BufReader::new(File::open(source).await?).lines();

        Ok(Self { source, queue })
    }

    #[tracing::instrument(skip_all)]
    pub async fn run(mut self, mut rx: ShutdownRx) -> Result<()> {
        loop {
            tokio::select! {
                _ = rx.changed() => break,

                site = self.source.next_line() => {
                    let Some(mut site) = site? else { break; };

                    site.insert_str(0, "https://");
                    self.queue.push(Url::parse(&site)?).await;
                }
            }
        }

        Ok(())
    }
}
