use std::sync::Arc;

use dashmap::DashMap;
use eyre::Result;
use fantoccini::{elements::Element, Client};
use tracing::*;

/// Every single non-deprecated HTML 5 tag.
static HTML_TAGS: phf::Set<&'static str> = phf::phf_set! {
    "a", "abbr", "address", "area", "article", "aside", "audio",
    "b", "base", "bdi", "bdo", "blockquote", "body", "br", "button",
    "canvas", "caption", "cite", "code", "col", "colgroup",
    "data", "datalist", "dd", "del", "details", "dfn", "dialog", "div", "dl", "dt",
    "em", "embed", "fieldset", "figcaption", "figure", "footer", "form",
    "h1", "h2", "h3", "h4", "h5", "h6", "head", "header", "hgroup", "hr", "html",
    "i", "iframe", "img", "input", "ins",
    "kbd",
    "label", "legend", "li", "link",
    "main", "map", "mark", "menu", "meta", "meter",
    "nav", "noscript",
    "object", "ol", "optgroup", "option", "output",
    "p", "picture", "pre", "progress",
    "q",
    "rp", "rt", "ruby",
    "s", "samp", "script", "section", "select", "slot", "small", "source",
    "span", "strong", "style", "sub", "summary", "sup",
    "table", "tbody", "td", "template", "textarea", "tfoot",
    "th", "thead", "time","title", "tr", "track",
    "u", "ul",
    "var", "video",
    "wbr"
};

#[derive(Clone, Debug, Default)]
pub struct Output {
    pub freq: Arc<DashMap<String, usize>>,
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
            window_height,
            window_width,
        })
    }

    pub async fn accept_node(mut self, v: Element) -> Result<Self> {
        let Ok(tag) = v.tag_name().await else {
            warn!(v = ?v.element_id(), "Unable to get name for element - perhaps it has already been removed from the DOM?");
            return Ok(self);
        };

        if !HTML_TAGS.contains(&tag) {
            debug!(
                tag,
                "Found unrecognized tag — might be a web component/XML/SVG/..."
            );
            return Ok(self);
        }

        match tag.as_str() {
            "div" => {
                let (x, y, w, h) = v.rectangle().await?;
                let w = w / self.window_width as f64;
                let h = h / self.window_height as f64;

                trace!("Found div element ({x:.2}, {y:.2}) {w:.2} x {h:.2}");
            }
            _ => {}
        }

        *self.output.freq.entry(tag).or_default() += 1;
        Ok(self)
    }
}
