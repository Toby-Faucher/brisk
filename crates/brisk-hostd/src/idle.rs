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
    pub fn observe(&mut self, now: Instant, o: Option<Observed>) -> bool {
        match o {
            Some(o) if !(o.streaming || o.game_running || o.updating) => {
                // Stays true on later idle observations, in case the shutdown failed.
                now.saturating_duration_since(*self.since.get_or_insert(now)) >= self.after
            }
            _ => {
                self.since = None;
                false
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Observed = Observed {
        streaming: false,
        game_running: false,
        updating: false,
    };

    fn watch() -> (IdleWatch, Instant) {
        (IdleWatch::new(Duration::from_secs(60)), Instant::now())
    }

    #[test]
    fn starts_idle_and_waits_for_threshold() {
        let (mut w, t) = watch();
        assert!(!w.observe(t, Some(QUIET)));
        assert_eq!(w.since, Some(t));
        assert!(!w.observe(t + Duration::from_secs(59), Some(QUIET)));
        assert!(w.observe(t + Duration::from_secs(60), Some(QUIET)));
    }

    #[test]
    fn repeats_true_once_due() {
        let (mut w, t) = watch();
        w.observe(t, Some(QUIET));
        for s in [60, 90, 600] {
            assert!(w.observe(t + Duration::from_secs(s), Some(QUIET)));
        }
    }

    #[test]
    fn none_resets() {
        let (mut w, t) = watch();
        w.observe(t, Some(QUIET));
        assert!(!w.observe(t + Duration::from_secs(100), None));
        assert_eq!(w.since, None);
        assert!(!w.observe(t + Duration::from_secs(101), Some(QUIET)));
        assert!(!w.observe(t + Duration::from_secs(160), Some(QUIET)));
        assert!(w.observe(t + Duration::from_secs(161), Some(QUIET)));
    }

    #[test]
    fn each_activity_flag_resets() {
        for i in 0..3 {
            let (mut w, t) = watch();
            w.observe(t, Some(QUIET));
            let mut o = QUIET;
            [&mut o.streaming, &mut o.game_running, &mut o.updating][i].clone_from(&true);
            assert!(
                !w.observe(t + Duration::from_secs(100), Some(o)),
                "flag {i}"
            );
            assert_eq!(w.since, None);
            // The clock restarts from the next idle observation.
            assert!(!w.observe(t + Duration::from_secs(110), Some(QUIET)));
            assert!(w.observe(t + Duration::from_secs(170), Some(QUIET)));
        }
    }
}
