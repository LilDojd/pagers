use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use pagers_core::events::Event as CoreEvent;
use pagers_core::mincore::DefaultPageMap;

/// Internal event type combining core events with TUI-specific events.
pub(crate) enum TuiEvent<PM = DefaultPageMap> {
    Core(CoreEvent<PM>),
    Quit,
}

pub(crate) fn spawn_event_threads<PM: Send + 'static>(
    core_rx: mpsc::Receiver<CoreEvent<PM>>,
    term: Arc<AtomicBool>,
) -> mpsc::Receiver<TuiEvent<PM>> {
    let (tui_tx, tui_rx) = mpsc::channel::<TuiEvent<PM>>();

    let signal_term = Arc::clone(&term);
    let signal_tx = tui_tx.clone();
    thread::spawn(move || {
        while !signal_term.load(Ordering::Relaxed) {
            thread::sleep(Duration::from_millis(50));
        }
        let _ = signal_tx.send(TuiEvent::Quit);
    });

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
