//! The idle-shutdown clock for a power machine (docs/plans/machine-hub.md, Idle shutdown).
//! Pure: the hub feeds it observations and acts on the answer.

use std::time::{Duration, Instant};

/// What the hub saw on a `ready` machine.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Observed {
    pub streaming: bool,
    pub game_running: bool,
    pub updating: bool,
}

pub struct IdleWatch {
    pub after: Duration,
    pub since: Option<Instant>,
}

impl IdleWatch {
    pub fn new(after: Duration) -> Self {
        IdleWatch { after, since: None }
    }

    /// `None`: the machine is not `ready` or could not be observed (resets the clock).
    /// True: idle for `after`, shut it down now.
    pub fn observe(&mut self, _now: Instant, _o: Option<Observed>) -> bool {
        false
    }
}
