use std::collections::HashMap;

use eyre::Result;
use fantoccini::{elements::Element, Client};
use tracing::*;

#[derive(Clone, Debug, Default)]
pub struct Output {
    pub freq: HashMap<String, usize>,
}
impl Output {
    pub fn merge(&mut self, other: Self) {
        self.freq.extend(other.freq);
    }
}

#[derive(Clone, Debug, Default)]
pub struct State {
    pub output: Output,
    pub window_width: u64,
    pub window_height: u64,
}
impl State {
    pub async fn new(c: &Client) -> Result<Self> {
        let (window_width, window_height) = c.get_window_size().await?;
        Ok(Self {
            output: Default::default(),
            window_height,
            window_width,
        })
    }

    pub async fn accept_node(mut self, v: Element) -> Result<Self> {
        let Ok(name) = v.tag_name().await else {
            warn!(v = ?v.element_id(), "Unable to get name for element - perhaps it has already been removed from the DOM?");
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

        *self.output.freq.entry(name).or_default() += 1;
        Ok(self)
    }
}
