mod bar_chart;

use std::{io::Stdout, time::Duration, vec};

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};
use eyre::Result;
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Style},
    text::{Span, Spans},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};
use tokio::sync::{oneshot, watch};
use tui_logger::TuiLoggerWidget;

use crate::{state::Output, util::Tag};

use self::bar_chart::BarChart;

type Backend = CrosstermBackend<Stdout>;

pub struct Tui {
    terminal: Terminal<Backend>,
    app: App,
}
impl Tui {
    pub fn new(output: Output) -> Result<Self> {
        let backend = {
            terminal::enable_raw_mode()?;
            let mut stdout = std::io::stdout();
            execute!(stdout, terminal::EnterAlternateScreen)?;
            CrosstermBackend::new(stdout)
        };
        let terminal = Terminal::new(backend)?;

        let app = App {
            freq: vec![],
            output,
        };

        Ok(Self { terminal, app })
    }
    pub fn end(mut self) -> Result<()> {
        terminal::disable_raw_mode()?;
        execute!(self.terminal.backend_mut(), terminal::LeaveAlternateScreen)?;
        self.terminal.show_cursor()?;

        Ok(())
    }
    pub async fn run(
        mut self,
        shutdown_tx: watch::Sender<()>,
        mut close_rx: oneshot::Receiver<()>,
    ) -> Result<()> {
        let mut events = EventStream::new();
        let mut ui_update_ticker = tokio::time::interval(Duration::from_millis(100));

        loop {
            tokio::select! {
                _ = &mut close_rx => break,
                Some(event) = events.next() => match event? {
                    Event::Key(KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. }) => {
                        shutdown_tx.send(()).unwrap();
                    }
                    _ => {}
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

struct App {
    freq: Vec<(String, u64)>,
    output: Output,
}
impl App {
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
    }

    fn ui(&mut self) -> impl FnOnce(&mut Frame<'_, Backend>) + '_ {
        |f| {
            let layout = Layout::default()
                .margin(1)
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(f.size());
            let bottom = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
                .split(layout[1]);

            let chart = BarChart::new(&self.freq)
                .block(Block::default().title("Histogram").borders(Borders::ALL))
                .bar_width(10)
                .bar_gap(1);
            f.render_widget(chart, layout[0]);

            let info = Paragraph::new(vec![
                Spans::from(Span::from("4444")),
                Spans::from(Span::from("4445")),
                Spans::from(Span::from("4446")),
            ])
            .block(Block::default().borders(Borders::ALL))
            .alignment(Alignment::Center);
            f.render_widget(info, bottom[0]);

            let widget = TuiLoggerWidget::default()
                .block(Block::default().title("Log").borders(Borders::ALL))
                .output_separator(' ')
                .output_level(None)
                .output_file(false)
                .output_line(false)
                .style_warn(Style::default().fg(Color::Yellow))
                .style_error(Style::default().fg(Color::Red))
                .style_trace(Style::default().fg(Color::Magenta))
                .style_debug(Style::default().fg(Color::Blue));
            f.render_widget(widget, bottom[1]);
        }
    }
}
