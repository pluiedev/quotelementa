use std::{path::Path, sync::Arc};

use deadqueue::limited::Queue;
use eyre::Result;
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, BufReader, Lines},
};

use crate::ShutdownRx;

pub struct Assigner {
    source: Lines<BufReader<File>>,
    queue: Arc<Queue<String>>,
}
impl Assigner {
    pub async fn new(source: &Path, queue: Arc<Queue<String>>) -> Result<Self> {
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
                    self.queue.push(site).await;
                }
            }
        }

        Ok(())
    }
}
