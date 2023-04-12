use std::{
    str::FromStr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

use eyre::Result;
use fantoccini::{elements::Element, Client};
use strum::EnumCount;
use tokio::sync::{RwLock, RwLockReadGuard};
use tracing::*;

use crate::util::Tag;

#[derive(Clone, Debug)]
pub struct Freq {
    inner: Arc<RwLock<[u64; Tag::COUNT]>>,
    dirty: Arc<AtomicBool>,
}

impl Freq {
    pub async fn get(&self) -> RwLockReadGuard<'_, [u64; Tag::COUNT]> {
        self.inner.read().await
    }
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Relaxed)
    }
    pub fn mark_dirty(&self) {
        self.dirty.store(true, Ordering::Relaxed);
    }
    pub async fn bump(&self, tag: Tag) {
        let mut inner = self.inner.write().await;
        inner[tag as usize] += 1;
        self.mark_dirty();
    }
}
impl Default for Freq {
    fn default() -> Self {
        Self {
            inner: Arc::new(RwLock::new([0; Tag::COUNT])),
            dirty: Default::default(),
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct Output {
    pub freq: Freq,
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub output: Output,
    pub window_width: u64,
    pub window_height: u64,
}
impl State {
    pub async fn new(output: Output, c: &Client) -> Result<Self> {
        let (window_width, window_height) = c.get_window_size().await?;
        Ok(Self {
            output,
            window_width,
            window_height,
        })
    }

    #[allow(clippy::cast_precision_loss)]
    pub async fn accept_node(self, elem: Element) -> Result<Self> {
        let Ok(tag) = elem.tag_name().await else {
            warn!(v = ?elem.element_id(), "Unable to get name for element - perhaps it has already been removed from the DOM?");
            return Ok(self);
        };

        let Ok(tag) = Tag::from_str(&tag) else {
            debug!(
                tag,
                "Found unrecognized tag — might be a web component/XML/SVG/..."
            );
            return Ok(self);
        };

        match tag {
            Tag::Div => {
                let (x, y, w, h) = elem.rectangle().await?;
                let w = w / self.window_width as f64;
                let h = h / self.window_height as f64;

                trace!("Found div element ({x:.2}, {y:.2}) {w:.2} x {h:.2}");
            }
            _ => {}
        }

        self.output.freq.bump(tag).await;

        Ok(self)
    }
}
