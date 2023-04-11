use std::time::Duration;

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use eyre::Result;
use ratatui::{
    backend::{Backend, CrosstermBackend},
    widgets::{Block, Borders},
    Frame, Terminal,
};
use tui_logger::TuiLoggerWidget;

pub fn tui() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    terminal.draw(ui)?;
    std::thread::sleep(Duration::from_millis(5000));
    terminal.draw(ui)?;
    std::thread::sleep(Duration::from_millis(5000));

    // restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}
fn ui(f: &mut Frame<impl Backend>) {
    let size = f.size();
    // let block = Block::default().title("Block").borders(Borders::ALL);
	let widget = TuiLoggerWidget::default();
    f.render_widget(widget, size)
}
