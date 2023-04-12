mod bar_chart;

use std::{io::Stdout, time::Duration};

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
    widgets::{Block, Borders},
    Frame, Terminal,
};
use tokio::sync::{oneshot, watch};
use tui_logger::TuiLoggerWidget;

use crate::state::Output;

use self::bar_chart::BarChart;

type Backend = CrosstermBackend<Stdout>;

pub struct Tui {
    terminal: Terminal<Backend>,
    output: Output,
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

        Ok(Self { terminal, output })
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
        let mut ui_update_ticker = tokio::time::interval(Duration::from_millis(200));

        loop {
            tokio::select! {
                _ = &mut close_rx => break,
                Some(event) = events.next() => match event? {
                    Event::Key(KeyEvent { code: KeyCode::Char('c'), modifiers: KeyModifiers::CONTROL, .. }) => {
                        shutdown_tx.send(()).expect("Subscribers have somehow all stopped");
                    }
                    _ => {}
                },
                _ = ui_update_ticker.tick() => {
                    let ui = self.ui();
                    self.terminal.draw(ui)?;
                }
            }
        }

        self.end()
    }

    fn ui(&mut self) -> impl FnOnce(&mut Frame<Backend>) {
        // this is SUCH a bad API...
        let mut freq: Vec<_> = self
            .output
            .freq
            .iter()
            .map(|v| (v.key().to_owned(), *v.value() as u64))
            .collect();
        freq.sort_by(|a, b| b.1.cmp(&a.1));

        |f| {
            let layout = Layout::default()
                .margin(1)
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(f.size());

            let chart = BarChart::new(freq)
                .block(Block::default().title("Histogram").borders(Borders::ALL))
                .bar_width(10)
                .bar_gap(1);
            f.render_widget(chart, layout[0]);

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

            f.render_widget(widget, layout[1]);
        }
    }
}
