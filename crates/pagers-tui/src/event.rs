use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pagers_core::Cancellation;
use pagers_core::events::Event as CoreEvent;
use pagers_core::mincore::DefaultPageMap;
use ratatui_crossterm::crossterm;

/// Internal event type combining core events with TUI-specific events.
pub(crate) enum TuiEvent<PM = DefaultPageMap> {
    Core(CoreEvent<PM>),
    Quit,
}

pub(crate) fn spawn_event_threads<PM: Send + 'static>(
    core_rx: mpsc::Receiver<CoreEvent<PM>>,
    cancellation: Cancellation,
) -> mpsc::Receiver<TuiEvent<PM>> {
    let (tui_tx, tui_rx) = mpsc::channel::<TuiEvent<PM>>();

    let signal_cancellation = cancellation.clone();
    let signal_tx = tui_tx.clone();
    thread::spawn(move || {
        signal_cancellation.wait();
        let _ = signal_tx.send(TuiEvent::Quit);
    });

    let key_cancellation = cancellation;
    thread::spawn(move || {
        while !key_cancellation.is_cancelled() {
            if crossterm::event::poll(Duration::from_millis(100)).unwrap_or(false)
                && let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read()
            {
                let is_quit = key.code == crossterm::event::KeyCode::Char('q')
                    || (key.code == crossterm::event::KeyCode::Char('c')
                        && key
                            .modifiers
                            .contains(crossterm::event::KeyModifiers::CONTROL));
                if is_quit {
                    key_cancellation.cancel();
                    return;
                }
            }
        }
    });

    let core_tx = tui_tx;
    thread::spawn(move || {
        while let Ok(event) = core_rx.recv() {
            if core_tx.send(TuiEvent::Core(event)).is_err() {
                return;
            }
        }
    });

    tui_rx
}
