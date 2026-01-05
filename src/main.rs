mod app;
mod audio;
mod config;
mod events;
mod library;
mod ui;

use std::io::{self, stdout};
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyEventKind, MouseButton, MouseEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;
use events::Action;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create app
    let mut app = App::new()?;

    // Main loop
    let result = run_app(&mut terminal, &mut app).await;

    // Restore terminal
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    if let Err(e) = result {
        eprintln!("Error: {}", e);
    }

    Ok(())
}

async fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> Result<()> {
    let tick_rate = Duration::from_millis(100);

    while app.running {
        // Draw UI
        terminal.draw(|frame| {
            ui::render(app, frame.area(), frame.buffer_mut());
        })?;

        // Process any pending scan events
        app.process_scan_events();

        // Process artwork fetch events
        app.process_artwork_events();

        // Process watched folder events (file create/modify/delete)
        app.process_watch_events();

        // Check if current track ended
        app.check_track_ended()?;

        // Handle input with timeout
        if event::poll(tick_rate)? {
            match event::read()? {
                Event::Key(key) => {
                    // Only handle key press events (not release)
                    if key.kind == KeyEventKind::Press {
                        let action = Action::from_key_event(key);
                        app.handle_action(action)?;
                    }
                }
                Event::Mouse(mouse) => {
                    let action = match mouse.kind {
                        MouseEventKind::Down(MouseButton::Left) => {
                            Action::MouseClick { x: mouse.column, y: mouse.row }
                        }
                        MouseEventKind::Drag(MouseButton::Left) => {
                            Action::MouseDrag { x: mouse.column, y: mouse.row }
                        }
                        MouseEventKind::ScrollUp => {
                            Action::MouseScrollUp { x: mouse.column, y: mouse.row }
                        }
                        MouseEventKind::ScrollDown => {
                            Action::MouseScrollDown { x: mouse.column, y: mouse.row }
                        }
                        _ => Action::None,
                    };
                    if action != Action::None {
                        app.handle_action(action)?;
                    }
                }
                _ => {}
            }
        }

        // Clear status message after a while
        // (In a real app, you'd use a timer)
    }

    Ok(())
}
