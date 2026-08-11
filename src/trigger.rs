//! Triggers that decide *when* a flight-recorder snapshot should be taken.
//!
//! A [`Trigger`] is a cheap, thread-safe predicate over a captured event.
//! Attach one to a [`FlightRecorderLayer`](crate::FlightRecorderLayer) via
//! [`with_dump_on`](crate::FlightRecorderLayer::with_dump_on) for automatic
//! snapshot-on-failure behaviour — the central value proposition of a flight
//! recorder: pay zero I/O until something goes wrong, then persist the last N
//! events automatically.
//!
//! ```
//! use tracing_flight_recorder::{FlightRecorder, FlightRecorderLayer, LevelTrigger, OnceTrigger};
//! use tracing_subscriber::layer::SubscriberExt;
//! use tracing_subscriber::Layer;
//!
//! let recorder = FlightRecorder::new(1000);
//! // Dump exactly once, automatically, the first time an ERROR fires.
//! let layer = FlightRecorderLayer::new(recorder.clone())
//!     .with_dump_on(OnceTrigger::new(LevelTrigger::error()), std::env::temp_dir(), "incident", 10);
//! # // (subscriber wiring omitted for brevity)
//! # let _ = layer;
//! ```

use crate::capture::CapturedEvent;
use std::sync::atomic::{AtomicBool, Ordering};
use tracing::Level;

/// A predicate that decides whether the buffer should be dumped for a given event.
///
/// Implementations must be cheap (called on every event on the hot path) and
/// thread-safe (`Send + Sync`), because the layer is shared across all
/// subscriber threads.
///
/// Built-in implementations: [`LevelTrigger`] (fires at/above a severity) and
/// [`OnceTrigger`] (fires at most once until reset).
pub trait Trigger: Send + Sync + std::fmt::Debug {
    /// Returns `true` if the buffer should be dumped for this event.
    fn should_dump(&self, event: &CapturedEvent) -> bool;

    /// A short, human-readable name identifying *why* this trigger fires.
    ///
    /// Recorded as the `trigger_reason` in the dump envelope so an operator
    /// reading a snapshot file knows what caused it to be written.
    fn name(&self) -> &str;
}

/// Numeric severity rank: lower is more severe.
///
/// `ERROR` (1) is the most severe, `TRACE` (5) the least. Unknown levels map to
/// `u8::MAX` so they never satisfy any finite threshold.
fn level_rank_str(level: &str) -> u8 {
    match level {
        "ERROR" => 1,
        "WARN" => 2,
        "INFO" => 3,
        "DEBUG" => 4,
        "TRACE" => 5,
        _ => u8::MAX,
    }
}

/// Fires when an event's severity is at or above a configured [`Level`].
///
/// The canonical trigger: "dump the buffer the moment something goes ERROR."
/// "At or above" means *more severe or equally severe*, so
/// `LevelTrigger::new(Level::WARN)` fires on both `WARN` and `ERROR`.
#[derive(Debug)]
pub struct LevelTrigger {
    rank: u8,
    name: String,
}

impl LevelTrigger {
    /// Create a trigger that fires for events at or above `level`.
    ///
    /// `LevelTrigger::new(Level::ERROR)` fires only on `ERROR`.
    /// `LevelTrigger::new(Level::WARN)` fires on `WARN` and `ERROR`.
    #[must_use]
    pub fn new(level: Level) -> Self {
        Self {
            rank: level_rank_str(level.as_str()),
            name: format!("level>={level}"),
        }
    }

    /// Convenience constructor: fires on `ERROR` only.
    #[must_use]
    pub fn error() -> Self {
        Self::new(Level::ERROR)
    }
}

impl Trigger for LevelTrigger {
    fn should_dump(&self, event: &CapturedEvent) -> bool {
        // Lower rank = more severe. Fire when the event is at least as severe.
        level_rank_str(&event.level) <= self.rank
    }

    fn name(&self) -> &str {
        &self.name
    }
}

/// Decorator that allows an inner trigger to fire at most once until reset.
///
/// Wraps any [`Trigger`]; the first time the inner trigger would fire, the dump
/// proceeds and an internal flag is set. Subsequent evaluations return `false`
/// until [`reset`](OnceTrigger::reset) re-arms it. Use this to avoid writing one
/// snapshot per error when a cascade of errors occurs.
///
/// The once-token is consumed as soon as the inner trigger fires — even if the
/// subsequent dump fails (e.g. disk full). This prevents retry storms; call
/// `reset()` to re-arm after resolving the failure. The token claim is atomic
/// (`compare_exchange`), so under concurrent error bursts exactly one dump is
/// produced regardless of thread scheduling.
#[derive(Debug)]
pub struct OnceTrigger<T> {
    inner: T,
    fired: AtomicBool,
}

impl<T: Trigger> OnceTrigger<T> {
    /// Wrap `inner` so it can fire at most once until [`reset`](Self::reset).
    #[must_use]
    pub const fn new(inner: T) -> Self {
        Self {
            inner,
            fired: AtomicBool::new(false),
        }
    }

    /// Re-arm the trigger so it may fire again.
    pub fn reset(&self) {
        self.fired.store(false, Ordering::Release);
    }

    /// Whether this trigger has already fired and not yet been reset.
    #[must_use]
    pub fn has_fired(&self) -> bool {
        self.fired.load(Ordering::Acquire)
    }
}

impl<T: Trigger> Trigger for OnceTrigger<T> {
    fn should_dump(&self, event: &CapturedEvent) -> bool {
        // Fast path: already fired — skip the inner trigger evaluation entirely.
        if self.fired.load(Ordering::Acquire) {
            return false;
        }
        if !self.inner.should_dump(event) {
            return false;
        }
        // Atomically claim the once-token: false → true in a single atomic
        // op. If a concurrent thread already won the race, this returns
        // Err and we return false — exactly one dump is produced.
        self.fired
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn name(&self) -> &str {
        self.inner.name()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(level: &str) -> CapturedEvent {
        CapturedEvent {
            timestamp: chrono::Utc::now(),
            level: level.to_string().into(),
            target: "test".to_string(),
            message: "m".to_string(),
            fields: vec![],
            spans: vec![],
        }
    }

    #[test]
    fn level_trigger_error_fires_only_on_error() {
        let t = LevelTrigger::error();
        assert!(t.should_dump(&event("ERROR")));
        assert!(!t.should_dump(&event("WARN")));
        assert!(!t.should_dump(&event("INFO")));
        assert!(!t.should_dump(&event("DEBUG")));
        assert!(!t.should_dump(&event("TRACE")));
    }

    #[test]
    fn level_trigger_warn_fires_on_warn_and_error() {
        let t = LevelTrigger::new(Level::WARN);
        assert!(t.should_dump(&event("ERROR")));
        assert!(t.should_dump(&event("WARN")));
        assert!(!t.should_dump(&event("INFO")));
    }

    #[test]
    fn level_trigger_name_is_descriptive() {
        let t = LevelTrigger::new(Level::ERROR);
        assert!(t.name().contains("ERROR"));
    }

    #[test]
    fn once_trigger_fires_once_then_blocks() {
        let t = OnceTrigger::new(LevelTrigger::error());
        assert!(t.should_dump(&event("ERROR")), "first ERROR must fire");
        assert!(
            !t.should_dump(&event("ERROR")),
            "second ERROR must not fire"
        );
        assert!(t.has_fired());
    }

    #[test]
    fn once_trigger_reset_rearms() {
        let t = OnceTrigger::new(LevelTrigger::error());
        assert!(t.should_dump(&event("ERROR")));
        assert!(!t.should_dump(&event("ERROR")));
        t.reset();
        assert!(!t.has_fired());
        assert!(t.should_dump(&event("ERROR")), "after reset it fires again");
    }

    #[test]
    fn once_trigger_does_not_fire_on_non_matching() {
        let t = OnceTrigger::new(LevelTrigger::error());
        assert!(!t.should_dump(&event("INFO")));
        assert!(
            !t.has_fired(),
            "non-matching event must not consume the token"
        );
    }
}
