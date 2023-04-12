use std::{io::Stdout, time::Duration};

use crossterm::{
    event::{Event, EventStream, KeyCode, KeyEvent, KeyModifiers},
    execute, terminal,
};
use eyre::Result;
use futures_util::StreamExt;
use ratatui::{
    backend::CrosstermBackend,
    widgets::{Block, Borders},
    Frame, Terminal,
};
use tokio::sync::{oneshot, watch};
use tui_logger::TuiLoggerWidget;

type Backend = CrosstermBackend<Stdout>;

pub struct Tui {
    terminal: Terminal<Backend>,
}
impl Tui {
    pub fn new() -> Result<Self> {
        let backend = {
            terminal::enable_raw_mode()?;
            let mut stdout = std::io::stdout();
            execute!(stdout, terminal::EnterAlternateScreen)?;
            CrosstermBackend::new(stdout)
        };
        let terminal = Terminal::new(backend)?;

        Ok(Self { terminal })
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
        |f| {
            let size = f.size();
            let block = Block::default().title("Block").borders(Borders::ALL);
            let widget = TuiLoggerWidget::default().block(block);
            f.render_widget(widget, size)
        }
    }
}
