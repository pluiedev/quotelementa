mod bar_chart;

use std::{collections::BTreeMap, io::Stdout, time::Duration, vec};

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};
use eyre::Result;
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame, Terminal,
};
use tokio::sync::{mpsc, oneshot, watch};
use tracing::info;

use crate::{
    crawler::{CrawlerReport, CrawlerState},
    state::Output,
    util::{Port, Tag},
};

use self::bar_chart::BarChart;

type Backend = CrosstermBackend<Stdout>;

pub struct Tui {
    terminal: Terminal<Backend>,
    app: App,
}
impl Tui {
    pub fn new(app: App) -> Result<Self> {
        let backend = {
            terminal::enable_raw_mode()?;
            let mut stdout = std::io::stdout();
            execute!(stdout, terminal::EnterAlternateScreen)?;
            CrosstermBackend::new(stdout)
        };
        let terminal = Terminal::new(backend)?;

        Ok(Self { terminal, app })
    }
    pub fn end(mut self) -> Result<()> {
        terminal::disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), terminal::LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;

        Ok(())
    }
    pub async fn run(mut self, mut close_rx: oneshot::Receiver<()>) -> Result<()> {
        let mut events = EventStream::new();
        let mut ui_update_ticker = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                _ = &mut close_rx, if self.app.state != AppState::Done => match self.app.state {
                    AppState::Running => {
                        self.app.state = AppState::Done;
                    }
                    AppState::ShuttingDown => {
                        // we're already shutting down anyway
                        break;
                    }
                    _ => unreachable!(),
                },
                Some(event) = events.next() => if self.app.on_event(event?)? {
                    break;
                },
                _ = ui_update_ticker.tick() => {
                    self.app.update().await;
                    let ui = self.app.ui();
                    self.terminal.draw(ui)?;
                }
            }
        }

        self.end()
    }
}

const SPINNER_STATES: [&str; 8] = ["⣼", "⣹", "⢻", "⠿", "⡟", "⣏", "⣧", "⣶"];
type SpinnerState = u8;

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum AppState {
    #[default]
    Running,
    ShuttingDown,
    Done,
}

pub struct App {
    freq: Vec<(String, u64)>,
    output: Output,

    state: AppState,
    shutdown_tx: watch::Sender<()>,

    crawled_sites: usize,
    total_sites: usize,

    crawlers: BTreeMap<Port, (SpinnerState, CrawlerState)>,
    report_rx: mpsc::Receiver<CrawlerReport>,
}
impl App {
    pub fn new(
        output: Output,
        report_rx: mpsc::Receiver<CrawlerReport>,
        total_sites: usize,
        shutdown_tx: watch::Sender<()>,
    ) -> Self {
        Self {
            freq: vec![],
            output,
            state: AppState::default(),
            shutdown_tx,
            crawled_sites: 0,
            total_sites,
            crawlers: BTreeMap::new(),
            report_rx,
        }
    }

    fn on_event(&mut self, event: Event) -> Result<bool> {
        match event {
            Event::Key(key) => match key {
                KeyEvent {
                    code: KeyCode::Char('c'),
                    modifiers: KeyModifiers::CONTROL,
                    ..
                } => {
                    info!("Received Ctrl-C event - issuing shut down");
                    self.state = AppState::ShuttingDown;
                    self.shutdown_tx.send(()).unwrap();
                }
                KeyEvent {
                    code: KeyCode::Enter,
                    ..
                } if self.state == AppState::Done => {
                    return Ok(true);
                }
                _ => {}
            },
            _ => {}
        }
        Ok(false)
    }

    async fn update(&mut self) {
        if self.output.freq.is_dirty() {
            // kinda jank but... oh well
            let freq = self.output.freq.get().await;
            self.freq = freq
                .iter()
                .enumerate()
                .filter_map(|(i, v)| Tag::from_repr(i).map(|tag| (tag.to_string(), *v)))
                .collect();
            self.freq.sort_by(|(_, v1), (_, v2)| v2.cmp(v1));
        }
        while let Ok(report) = self.report_rx.try_recv() {
            match report.state {
                CrawlerState::Complete => {
                    self.crawled_sites += 1;
                }
                CrawlerState::Terminated => {
                    self.crawlers.remove(&report.port);
                }
                _ => {
                    self.crawlers.insert(report.port, (0, report.state));
                }
            }
        }
    }

    fn ui(&mut self) -> impl FnOnce(&mut Frame<'_, Backend>) + '_ {
        let status: Vec<_> = self
            .crawlers
            .iter_mut()
            .map(|(k, (spinner, v))| {
                let spinner = if v.should_spinner_spin() {
                    *spinner = (*spinner + 1) & 0b111;
                    SPINNER_STATES[*spinner as usize]
                } else {
                    *spinner = 0;
                    "⣿"
                };
                let spinner = Span::styled(spinner, Style::default().fg(v.spinner_color()));

                Spans::from(vec![
                    Span::from(" "),
                    Span::from(k.to_string()),
                    Span::from(" "),
                    spinner,
                    Span::from(" "),
                    Span::from(v.to_string()),
                ])
            })
            .collect();

        |f| {
            let layout = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Max(40), Constraint::Percentage(70)])
                .split(f.size());
            let left = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Min(5)])
                .split(layout[0]);

            let chart = BarChart::new(&self.freq)
                .block(Block::default().title(" Histogram ").borders(Borders::ALL))
                .bar_width(10)
                .bar_gap(1);
            f.render_widget(chart, layout[1]);

            {
                let block = Block::default()
                    .title(" Active Crawlers ")
                    .borders(Borders::ALL);
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Percentage(70), Constraint::Max(1)])
                    .split(block.inner(left[0]));
                let status = Paragraph::new(status);
                f.render_widget(block, left[0]);
                f.render_widget(status, split[0]);

                let ratio = self.crawled_sites as f64 / self.total_sites as f64;
                f.render_widget(
                    Gauge::default()
                        .gauge_style(Style::default().fg(Color::LightGreen))
                        .label(format!(
                            "{:.1}% ({}/{})",
                            ratio * 100.0,
                            self.crawled_sites,
                            self.total_sites
                        ))
                        .ratio(ratio),
                    split[1],
                );
            }
            {
                let block = Block::default().borders(Borders::ALL);
                let split = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([
                        Constraint::Percentage(40),
                        Constraint::Min(3),
                        Constraint::Percentage(40),
                    ])
                    .split(block.inner(left[1]));

                let status = match self.state {
                    AppState::Running => Paragraph::new(vec![
                        Spans::from(""),
                        Spans::from(" quotelementa v0.1.0 "),
                        Spans::from(""),
                    ]),
                    AppState::ShuttingDown => {
                        Paragraph::new(vec![Spans::from(" Press CTRL+C again to force quit. ")])
                    }
                    AppState::Done => Paragraph::new(vec![
                        Spans::from(" Everything done! "),
                        Spans::from(""),
                        Spans::from(" Press <ENTER> to exit "),
                    ])
                    .style(Style::default().fg(Color::LightYellow)),
                };
                let status = status
                    .wrap(Wrap { trim: false })
                    .alignment(ratatui::layout::Alignment::Center);

                f.render_widget(block, left[1]);
                f.render_widget(status, split[1]);
            }
        }
    }
}

impl CrawlerState {
    pub fn spinner_color(&self) -> Color {
        match self {
            Self::Initializing => Color::Yellow,
            Self::InProgress(_) => Color::LightGreen,
            Self::ShuttingDown => Color::LightRed,
            _ => Color::DarkGray,
        }
    }
    pub fn should_spinner_spin(&self) -> bool {
        matches!(
            self,
            Self::Initializing | Self::InProgress(_) | Self::ShuttingDown
        )
    }
}
