use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use pagers_core::events::Event as CoreEvent;

/// Internal event type combining core events with TUI-specific events.
pub(crate) enum TuiEvent {
    Core(CoreEvent),
    CoreDone,
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
    let signal_tx = tui_tx.clone();
    thread::spawn(move || {
        while !term.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
        }
        let _ = signal_tx.send(TuiEvent::Quit);
    });

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
