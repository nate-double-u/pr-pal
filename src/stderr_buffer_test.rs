use super::*;

// BUFFER is a process-wide static, so every assertion below lives in a
// single #[test] to guarantee sequential access regardless of how
// `cargo test` schedules threads.
#[test]
fn buffering_lifecycle() {
    // Drain any state left over from a previous run (defensive; BUFFER
    // starts as None so this is a no-op in practice).
    drain();

    // warn() with buffering inactive does not panic and does not get
    // captured anywhere (it falls through to eprintln!).
    warn("not buffered".to_string());
    assert_eq!(drain(), Vec::<String>::new());

    activate();
    warn("first".to_string());
    warn("second".to_string());
    let drained = drain();
    assert_eq!(drained, vec!["first".to_string(), "second".to_string()]);

    // drain() deactivates buffering and clears the buffer: a second
    // drain returns empty, and a subsequent warn() is unbuffered again.
    assert_eq!(drain(), Vec::<String>::new());

    activate();
    assert_eq!(drain(), Vec::<String>::new());

    // buffered_eprintln! routes through warn() while buffering is active.
    // This assertion must stay inside buffering_lifecycle (not its own
    // #[test]): BUFFER is a process-wide static, and cargo test runs
    // #[test] functions in parallel by default, so a second test touching
    // BUFFER concurrently can steal/deactivate the buffer mid-assertion.
    activate();
    buffered_eprintln!("{} {}", "hello", 42);
    assert_eq!(drain(), vec!["hello 42".to_string()]);
}
