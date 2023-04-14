use std::path::Path;

use eyre::{Context, ContextCompat, Result};
use tokio::{
    fs::File,
    io::{AsyncBufReadExt, AsyncReadExt, AsyncSeekExt, BufReader, Lines},
};
use url::Url;

use crate::{JobQueue, ShutdownRx};

async fn read_largest_index(f: &mut BufReader<File>) -> Result<usize> {
    // TODO: make this work for not just specifically engineered input
    let mut v = Vec::with_capacity(128);
    if f.seek(std::io::SeekFrom::End(-128)).await.is_err() {
        f.rewind().await?;
    }
    dbg!(f.read_to_end(&mut v).await?);

    let mut iter = v.iter();
    let first_lb = iter.by_ref().rposition(|&x| x == b'\n').unwrap();
    let second_lb = iter.rposition(|&x| x == b'\n').unwrap();

    let s = std::str::from_utf8(&v[second_lb + 1..first_lb])
        .wrap_err("Expected last line of input to be UTF-8")?;
    let (idx, _) = s
        .split_once(',')
        .wrap_err("Expected last line of input to be comma-separated")?;
    let idx = usize::from_str_radix(idx, 10)
        .wrap_err("Expected first entry of last line of input to be a numeric index")?;

    f.rewind().await?;
    Ok(idx)
}

pub struct Assigner {
    source: Lines<BufReader<File>>,
    queue: JobQueue,
}
impl Assigner {
    pub async fn new(source: &Path, queue: JobQueue) -> Result<(Self, usize)> {
        let mut source = BufReader::new(File::open(source).await?);
        let sites_count = read_largest_index(&mut source).await?;

        Ok((
            Self {
                source: source.lines(),
                queue,
            },
            sites_count,
        ))
    }

    #[tracing::instrument(skip_all)]
    pub async fn run(mut self, mut rx: ShutdownRx) -> Result<()> {
        loop {
            tokio::select! {
                _ = rx.changed() => break,

                site = self.source.next_line() => {
                    let Some(mut site) = site? else { break; };
                    let idx = site.find(',').unwrap() + 1;
                    site.insert_str(idx, "https://");

                    self.queue.push(Url::parse(&site[idx..])?).await;
                }
            }
        }

        Ok(())
    }
}
