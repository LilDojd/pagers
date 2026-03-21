use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pagers_core::events::Event as CoreEvent;

/// Internal event type combining core events with TUI-specific events.
pub(crate) enum TuiEvent {
    Core(CoreEvent),
    Quit,
}

/// Spawns signal-watcher and core-forwarder threads.
/// Returns a receiver for all TUI events.
pub(crate) fn spawn_event_threads(
    core_rx: mpsc::Receiver<CoreEvent>,
    term: Arc<AtomicBool>,
) -> mpsc::Receiver<TuiEvent> {
    let (tui_tx, tui_rx) = mpsc::channel::<TuiEvent>();

    // Polls the termination flag set by signal-hook in the CLI crate.
    let signal_term = Arc::clone(&term);
    let signal_tx = tui_tx.clone();
    thread::spawn(move || {
        while !signal_term.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
        }
        let _ = signal_tx.send(TuiEvent::Quit);
    });

    // Raw mode swallows SIGINT, so poll for Ctrl+C as a keypress.
    let key_term = Arc::clone(&term);
    let key_tx = tui_tx.clone();
    thread::spawn(move || {
        while !key_term.load(Ordering::Relaxed) {
            if crossterm::event::poll(Duration::from_millis(100)).unwrap_or(false)
                && let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read()
                && key.code == crossterm::event::KeyCode::Char('c')
                && key
                    .modifiers
                    .contains(crossterm::event::KeyModifiers::CONTROL)
            {
                key_term.store(true, Ordering::Relaxed);
                let _ = key_tx.send(TuiEvent::Quit);
                return;
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
