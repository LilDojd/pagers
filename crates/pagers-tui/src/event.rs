use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pagers_core::events::Event as CoreEvent;

/// Internal event type combining core events with TUI-specific events.
pub(crate) enum TuiEvent {
    Core(CoreEvent),
    CoreDone,
    Quit,
}

/// Spawns input/tick and core-forwarder threads.
/// Returns a receiver for all TUI events.
pub(crate) fn spawn_event_threads(
    core_rx: mpsc::Receiver<CoreEvent>,
) -> mpsc::Receiver<TuiEvent> {
    let (tui_tx, tui_rx) = mpsc::channel::<TuiEvent>();

    // Input thread: polls for Ctrl+C.
    let input_tx = tui_tx.clone();
    thread::spawn(move || {
        use crossterm::event::{self, Event, KeyCode, KeyModifiers};
        loop {
            if event::poll(Duration::from_millis(100)).unwrap_or(false)
                && let Ok(Event::Key(key)) = event::read()
                && key.code == KeyCode::Char('c')
                && key.modifiers.contains(KeyModifiers::CONTROL)
            {
                let _ = input_tx.send(TuiEvent::Quit);
                break;
            }
        }
    });

    // Core event forwarder thread.
    let core_tx = tui_tx;
    thread::spawn(move || {
        while let Ok(event) = core_rx.recv() {
            if core_tx.send(TuiEvent::Core(event)).is_err() {
                return;
            }
        }
        let _ = core_tx.send(TuiEvent::CoreDone);
    });

    tui_rx
}
