use crossterm::event::{KeyEvent, KeyEventKind};
use futures::StreamExt;
use std::io::IsTerminal;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum Event {
    Key(KeyEvent),
    Tick,
    Refresh, // Auto-refresh timer fired
}

pub struct EventHandler {
    rx: mpsc::UnboundedReceiver<Event>,
}

impl EventHandler {
    pub fn new(tick_rate_ms: u64, refresh_interval_secs: u64) -> Self {
        let (tx, rx) = mpsc::unbounded_channel();

        tokio::spawn(async move {
            // There's no terminal to read key events from outside a real TTY
            // (e.g. under `cargo test`). Guard the reader branch on this so it
            // is never polled in that case, since crossterm's EventStream
            // panics ("reader source not set") when polled without one.
            let has_tty = std::io::stdin().is_terminal();
            let mut reader = crossterm::event::EventStream::new();
            let mut tick_interval =
                tokio::time::interval(std::time::Duration::from_millis(tick_rate_ms));
            let mut refresh_interval =
                tokio::time::interval(std::time::Duration::from_secs(refresh_interval_secs));

            // Skip the first tick of refresh interval (it fires immediately)
            refresh_interval.tick().await;

            loop {
                tokio::select! {
                    maybe_event = reader.next(), if has_tty => {
                        if let Some(Ok(crossterm::event::Event::Key(key))) = maybe_event {
                            // Filter for Press only (Windows compatibility)
                            if key.kind == KeyEventKind::Press && tx.send(Event::Key(key)).is_err() {
                                break;
                            }
                        }
                    }
                    _ = tick_interval.tick() => {
                        if tx.send(Event::Tick).is_err() {
                            break;
                        }
                    }
                    _ = refresh_interval.tick() => {
                        if tx.send(Event::Refresh).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        EventHandler { rx }
    }

    pub async fn next(&mut self) -> Event {
        self.rx.recv().await.unwrap_or(Event::Tick)
    }
}

#[cfg(test)]
#[path = "event_test.rs"]
mod tests;
