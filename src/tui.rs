mod bar_chart;

use std::{collections::HashMap, io::Stdout, time::Duration, vec};

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};
use eyre::Result;
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use tokio::sync::{mpsc, oneshot, watch};

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
                _ = &mut close_rx => break,
                Some(event) = events.next() => self.app.on_event(event?)?,
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

pub struct App {
    freq: Vec<(String, u64)>,
    output: Output,

    is_shutting_down: bool,
    shutdown_tx: watch::Sender<()>,

    crawlers: HashMap<Port, (SpinnerState, CrawlerState)>,
    report_rx: mpsc::Receiver<CrawlerReport>,
}
impl App {
    pub fn new(
        output: Output,
        report_rx: mpsc::Receiver<CrawlerReport>,
        shutdown_tx: watch::Sender<()>,
    ) -> Self {
        Self {
            freq: vec![],
            output,
            is_shutting_down: false,
            shutdown_tx,
            crawlers: HashMap::new(),
            report_rx,
        }
    }

    fn on_event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Key(KeyEvent {
                code: KeyCode::Char('c'),
                modifiers: KeyModifiers::CONTROL,
                ..
            }) => {
                self.is_shutting_down = true;
                self.shutdown_tx.send(()).unwrap();
            }
            _ => {}
        }
        Ok(())
    }

    async fn update(&mut self) {
        if self.output.freq.is_dirty() {
            // kinda jank but... oh well
            let freq = self.output.freq.get().await;
            self.freq = freq
                .iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    if let Some(tag) = Tag::from_repr(i) {
                        Some((tag.to_string(), *v as u64))
                    } else {
                        None
                    }
                })
                .collect();
            self.freq.sort_by(|(_, v1), (_, v2)| v2.cmp(&v1));
        }
        while let Ok(report) = self.report_rx.try_recv() {
            self.crawlers.insert(report.port, (0, report.state));
        }
    }

    fn ui(&mut self) -> impl FnOnce(&mut Frame<'_, Backend>) + '_ {
        let crawlers: Vec<_> = self
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

            let info = Paragraph::new(crawlers)
                .block(Block::default().title("Status").borders(Borders::ALL));
            f.render_widget(info, layout[0]);

            let chart = BarChart::new(&self.freq)
                .block(Block::default().title("Histogram").borders(Borders::ALL))
                .bar_width(10)
                .bar_gap(1);
            f.render_widget(chart, layout[1]);

            if self.is_shutting_down {
                let status = Paragraph::new(vec![
                    Spans::from(" Press CTRL+C again to force quit.                           "),
                    Spans::from(" (You might have to terminate leftover WebDriver processes!) ")
                ])
                    .block(Block::default().borders(Borders::ALL))
                    .style(Style::default().fg(Color::Red).bg(Color::Gray));
                f.render_widget(
                    status,
                    Rect {
                        x: 0,
                        y: f.size().bottom() - 4,
                        width: 63,
                        height: 4,
                    },
                );
            }
        }
    }
}

impl CrawlerState {
    pub fn spinner_color(&self) -> Color {
        match self {
            Self::Initializing => Color::Yellow,
            Self::InProgress(_) => Color::LightGreen,
            Self::Idle => Color::White,
            Self::RanOutOfTasks => Color::DarkGray,
            Self::ShuttingDown => Color::LightRed,
        }
    }
    pub fn should_spinner_spin(&self) -> bool {
        matches!(
            self,
            Self::Initializing | Self::InProgress(_) | Self::ShuttingDown
        )
    }
}
