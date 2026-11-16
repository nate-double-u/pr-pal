use super::*;

// `EventHandler::new` had zero unit coverage: its spawned task multiplexes
// a tick timer, a refresh timer, and a terminal key-event stream over a
// single channel, but nothing exercised that the timers actually drive
// `next()`. These tests use a paused tokio clock so the virtual time
// advances instantly instead of sleeping in real time.

#[tokio::test(start_paused = true)]
async fn tick_events_fire_before_a_far_off_refresh() {
    // Fast tick, refresh effectively disabled for the duration of the test.
    let mut events = EventHandler::new(10, 3600);

    for _ in 0..3 {
        match events.next().await {
            Event::Tick => {}
            other => panic!("expected Event::Tick, got {other:?}"),
        }
    }
}

// LOCKED: regression for pr-pal#21 (crossterm's EventStream panics with
// "reader source not set" when polled without a real TTY, e.g. under
// `cargo test`, which previously killed the spawned task and starved the
// refresh timer).
#[tokio::test(start_paused = true)]
async fn refresh_event_fires_after_the_configured_interval() {
    // Tick effectively disabled, refresh set to the minimum interval so it
    // is the only timer that can fire within the test's virtual time.
    let mut events = EventHandler::new(3_600_000, 1);

    match events.next().await {
        Event::Refresh => {}
        other => panic!("expected Event::Refresh, got {other:?}"),
    }
}
