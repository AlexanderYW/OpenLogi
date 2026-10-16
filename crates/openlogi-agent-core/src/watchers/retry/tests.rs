use std::time::Duration;

use super::Backoff;

const INITIAL: Duration = Duration::from_secs(2);
const MAX: Duration = Duration::from_secs(60);

#[test]
fn each_failure_doubles_the_delay() {
    let mut backoff = Backoff::new(INITIAL, MAX);
    assert_eq!(backoff.delay(), INITIAL);
    assert!(backoff.is_initial());

    backoff.failed();
    assert_eq!(backoff.delay(), INITIAL * 2);
    assert!(!backoff.is_initial());
    backoff.failed();
    assert_eq!(backoff.delay(), INITIAL * 4);
}

#[test]
fn the_delay_never_passes_the_cap() {
    let mut backoff = Backoff::new(INITIAL, MAX);
    for _ in 0..100 {
        backoff.failed();
    }
    assert_eq!(backoff.delay(), MAX);
}

#[test]
fn a_success_returns_to_the_initial_delay() {
    let mut backoff = Backoff::new(INITIAL, MAX);
    backoff.failed();
    backoff.failed();

    backoff.reset();

    assert_eq!(backoff.delay(), INITIAL);
    assert!(backoff.is_initial());
}
