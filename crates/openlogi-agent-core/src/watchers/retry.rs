//! Retry pacing shared by the HID++ session managers, and the doubling
//! [`Backoff`] every agent retry loop that must not hammer a failing peer
//! paces itself with.

use std::time::Duration;

use tokio::time::Instant;

/// How long a manager waits before retrying a firmware restore that stayed
/// pending, or re-arming a session that ended unexpectedly or found the
/// receiver leased.
pub(super) const RETRY_DELAY: Duration = Duration::from_secs(1);

/// Sleep until `deadline`. A manager with nothing due waits here forever,
/// leaving its `select!` to the arms that can still make progress.
pub(super) async fn wait_for_deadline(deadline: Option<Instant>) {
    if let Some(deadline) = deadline {
        tokio::time::sleep_until(deadline).await;
    } else {
        std::future::pending::<()>().await;
    }
}

/// A retry delay that doubles after every failed attempt, up to a cap, and
/// returns to its initial delay once an attempt succeeds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    initial: Duration,
    max: Duration,
    current: Duration,
}

impl Backoff {
    /// A backoff that first waits `initial` and never longer than `max`.
    #[must_use]
    pub const fn new(initial: Duration, max: Duration) -> Self {
        Self {
            initial,
            max,
            current: initial,
        }
    }

    /// How long to wait before the next attempt.
    #[must_use]
    pub const fn delay(&self) -> Duration {
        self.current
    }

    /// Whether no failure has lengthened the delay since the last reset.
    #[must_use]
    pub fn is_initial(&self) -> bool {
        self.current == self.initial
    }

    /// Lengthen the delay after a failed attempt.
    pub fn failed(&mut self) {
        self.current = self.current.saturating_mul(2).min(self.max);
    }

    /// Return to the initial delay after a successful attempt.
    pub fn reset(&mut self) {
        self.current = self.initial;
    }
}

#[cfg(test)]
mod tests;
