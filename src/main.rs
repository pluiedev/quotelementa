use std::{
    collections::{HashMap, VecDeque},
    time::Duration, error::Error, convert::Infallible,
};

use anyhow::bail;
use headless_chrome::{
    browser::tab::element::ElementQuad,
    protocol::cdp::DOM::{self, BoxModel, Node},
    types::CurrentBounds,
    Browser, Element, Tab,
};
use indexmap::IndexSet;
use tracing::{debug, info, field::Visit};
use url::Url;

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let browser = Browser::default()?;
    let tab = browser.new_tab()?;

    let mut visitor = CountVisitor::new(&tab)?;

    let mut crawler = Crawler::new(&tab)?;
    crawler.sites.insert(Url::parse("https://en.wikipedia.org")?);
    crawler.crawl_one(&mut visitor)?;

    let sites = crawler
        .sites
        .into_iter()
        .map(|u| u.as_str().to_owned())
        .collect::<Vec<_>>();
    let visited = crawler
        .visited
        .into_iter()
        .map(|u| u.as_str().to_owned())
        .collect::<Vec<_>>();
    info!(?visitor.map, ?sites, ?visited);
    
    Ok(())
}

pub fn get_attribute_value<'a, S: AsRef<str> + 'a>(attrs: &'a [S], key: &'a str) -> Option<&'a str> {
    attrs.iter().position(|s| s.as_ref() == key).and_then(|p| attrs.get(p + 1).map(AsRef::as_ref))
}

pub trait Visitor {
    type Err;

    fn visit(&mut self, node: &Node) -> Result<(), Self::Err>;
}


pub struct Crawler<'a> {
    tab: &'a Tab,
    sites: IndexSet<Url>,
    visited: IndexSet<Url>,
}
impl<'a> Crawler<'a> {
    fn new(tab: &'a Tab) -> anyhow::Result<Self> {
        Ok(Self {
            tab,
            sites: Default::default(),
            visited: Default::default(),
        })
    }
    fn crawl_one<V: Visitor>(&mut self, visitor: &mut V) -> anyhow::Result<()>
    where
        V::Err: Error + Sync + Send + 'static
    {
        let Some(site) = self.sites.pop() else { return Ok(()); };

        if self.visited.contains(&site) {
            debug!(?site, "Site already visited!");
            return Ok(()); // already visited!
        }

        self.tab.navigate_to(site.as_str())?;

        info!("Start sleeping for 5 seconds");
        std::thread::sleep(Duration::from_secs(5));
        info!("Damn, felt like an eternity");

        let body = self.tab.find_element("body")?;
        let body = self.tab.describe_node(body.node_id)?;

        self.accept(&body, visitor)?;
        self.visited.insert(site);

        Ok(())
    }
    fn accept<V: Visitor>(&mut self, node: &Node, visitor: &mut V) -> Result<(), V::Err> {
        if let Some(children) = &node.children {
            for child in children {
                self.accept(child, visitor)?;
            }
        }

        visitor.visit(node)
    }
}

pub struct CountVisitor<'a> {
    tab: &'a Tab,
    dims: CurrentBounds,

    map: HashMap<String, u32>,
}
impl<'a> CountVisitor<'a> {
    fn new(tab: &'a Tab) -> anyhow::Result<Self> {
        let dims = tab.get_bounds()?;
        info!("dims: {} x {}", dims.width, dims.height);

        Ok(Self {
            tab,
            dims,
            map: Default::default(),
        })
    }
    fn visit_div(&mut self, node: &Node) {
        if let Ok(bbox) = self.tab.call_method(DOM::GetBoxModel {
            backend_node_id: Some(node.backend_node_id),
            node_id: None,
            object_id: None,
        }) {
            let margin = ElementQuad::from_raw_points(&bbox.model.margin);
            let BoxModel { width, height, .. } = bbox.model;
            info!(
                "dim: {} x {} | pos: ({}, {})",
                width as f32 / self.dims.width as f32,
                height as f32 / self.dims.height as f32,
                margin.top_left.x,
                margin.top_left.y,
            );
        }
    }
}
impl Visitor for CountVisitor<'_> {
    type Err = Infallible;
    
    fn visit(&mut self, node: &Node) -> Result<(), Self::Err> {
        // we only consider elements.
        if node.node_type == 1 {
            let name = node.node_name.to_ascii_lowercase();

            match name.as_str() {
                "div" => self.visit_div(&node),
                _ => {},
            }

            *self.map.entry(name).or_default() += 1;
        }
        Ok(())
    }
}