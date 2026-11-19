//! When a ready release restarts the daemon.
//!
//! On 2026-11-18 the daemon on the hub's machine restarted 16 times in under two hours, once for
//! each landed release that changed Ignite's code, and each restart reran every `ignite start`
//! in progress. A restart now waits for a quiet window: it goes ahead once no newer release has
//! appeared for `quiet_seconds` and nothing else holds it (a landing, when the release changes
//! the landing code, or a newer release still building), but never later than `cap_seconds` after the first release it waited with, so a
//! fix reaches the machine even while releases keep landing. Both numbers come from the
//! `[self_update]` section of `ignite.toml`. A push in progress holds the restart regardless
//! of the cap; [`super::selfupdate::SelfUpdate::after_heartbeat`] checks that before asking
//! here.

use ignite_core::config::SelfUpdateConfig;
use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;

/// The releases a restart that is due waits with.
#[derive(Clone, Debug, PartialEq)]
struct Pending {
    /// When the first of them appeared: the cap counts from here.
    first: Timestamp,
    /// The newest of them, and when it appeared: the quiet window counts from here.
    newest: Commit,
    newest_at: Timestamp,
}

/// What [`RestartWindow::decide`] found.
#[derive(Clone, Debug, PartialEq)]
pub enum Decision {
    /// Restart now, after waiting this many seconds since the first pending release.
    Go { waited: f64 },
    /// Wait. The reason is logged once, when the wait begins.
    Wait,
}

/// Collects the releases that appear while a restart is pending and says when it may go ahead.
#[derive(Debug)]
pub struct RestartWindow {
    config: SelfUpdateConfig,
    pending: Option<Pending>,
    /// Whether the wait has been logged since the first pending release.
    said: bool,
}

impl RestartWindow {
    pub fn new(config: SelfUpdateConfig) -> Self {
        Self {
            config,
            pending: None,
            said: false,
        }
    }

    /// Records that the release `commit` appeared at `now`. A release already recorded as the
    /// newest leaves the quiet window where it is.
    pub fn saw(&mut self, commit: &Commit, now: Timestamp) {
        match &mut self.pending {
            Some(pending) if pending.newest == *commit => {}
            Some(pending) => {
                pending.newest = commit.clone();
                pending.newest_at = now;
            }
            None => {
                self.pending = Some(Pending {
                    first: now,
                    newest: commit.clone(),
                    newest_at: now,
                });
            }
        }
    }

    /// The newest release seen since the last restart or release taken on, if any.
    pub fn newest(&self) -> Option<&Commit> {
        self.pending.as_ref().map(|pending| &pending.newest)
    }

    /// Forgets the pending releases, after a restart failed or a release was taken on without
    /// one.
    pub fn clear(&mut self) {
        self.pending = None;
        self.said = false;
    }

    /// Whether a restart into `target` may go ahead at `now`. `held` names what else holds it
    /// (a landing runs, a newer release builds), if anything. A restart with no recorded
    /// release, such as one into the hub's version, starts its window now.
    pub fn decide(&mut self, target: &Commit, now: Timestamp, held: Option<&str>) -> Decision {
        if self.pending.is_none() {
            self.saw(target, now);
        }
        let Some(pending) = &self.pending else {
            return Decision::Go { waited: 0.0 };
        };
        let waited = now.seconds_since(pending.first);
        if waited >= self.config.cap_seconds {
            return Decision::Go { waited };
        }
        let quiet_for = now.seconds_since(pending.newest_at);
        let why = match held {
            Some(why) => why.to_owned(),
            None if quiet_for < self.config.quiet_seconds => format!(
                "no newer release has appeared for {:.0} s yet",
                self.config.quiet_seconds
            ),
            None => return Decision::Go { waited },
        };
        if !self.said {
            eprintln!(
                "ignite: a new release is ready; the restart waits, at most {:.0} s from the \
                 first release, because {why}",
                self.config.cap_seconds
            );
            self.said = true;
        }
        Decision::Wait
    }

    /// When the pending restart goes ahead if no newer release appears and nothing holds it:
    /// the end of the quiet window, or the cap if that comes first. `None` with nothing
    /// pending.
    pub fn due_at(&self) -> Option<Timestamp> {
        let pending = self.pending.as_ref()?;
        let quiet = pending.newest_at.plus_seconds(self.config.quiet_seconds);
        let cap = pending.first.plus_seconds(self.config.cap_seconds);
        Some(quiet.min(cap))
    }

    /// The latest moment the pending restart can be held to, a push aside. `None` with nothing
    /// pending.
    pub fn cap_at(&self) -> Option<Timestamp> {
        let pending = self.pending.as_ref()?;
        Some(pending.first.plus_seconds(self.config.cap_seconds))
    }
}
