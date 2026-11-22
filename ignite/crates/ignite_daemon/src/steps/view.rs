//! Activity snapshots and their status text.

use std::fmt::Write as _;

use serde::Serialize;

use crate::loop_health::span;

/// What an activity is doing, for `status`, `wait` and `doctor`.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct View {
    /// `verb` or `loop`.
    pub kind: &'static str,
    /// The verb or the loop's name.
    pub name: String,
    pub handle: Option<String>,
    pub ticket: Option<String>,
    pub session: Option<String>,
    pub tree: Option<String>,
    pub class: Option<&'static str>,
    pub age_s: f64,
    pub step: Option<String>,
    pub step_age_s: Option<f64>,
    pub waiting_for: Option<String>,
    pub waiting_s: Option<f64>,
    /// What the work waits for is cores in the scheduler's queue, which work of a higher class
    /// holds: the work is not stuck, only behind others (see [`super::waiting_for_cores`]).
    pub waiting_for_cores: bool,
    /// Seconds the work has spent in the scheduler's queue for cores, the wait still open
    /// included.
    pub cores_waited_s: f64,
    /// A stop has been asked for and the work has not ended yet.
    pub stopping: bool,
    /// The work started under an earlier daemon, which did not tell this one its step yet.
    pub adopted: bool,
}

impl View {
    /// The step and what the work waits for, with how long each has lasted, such as
    /// `building rust-tests in landing-3 (4 min), waiting for 8 cores (3 min)`.
    pub fn doing(&self) -> String {
        let mut text = match (&self.step, self.step_age_s) {
            (Some(step), Some(age)) => format!("{step} ({})", span(age)),
            _ if self.adopted => "step not reported since the daemon restarted".to_owned(),
            _ => "no step recorded".to_owned(),
        };
        if let (Some(what), Some(age)) = (&self.waiting_for, self.waiting_s) {
            let _ = write!(text, ", waiting for {what} ({})", span(age));
        }
        if self.stopping {
            text.push_str(", stopping");
        }
        text
    }

    /// The line `ignite status` shows for a running command.
    pub fn line(&self) -> String {
        let mut words = vec![self.handle.clone().unwrap_or_else(|| "-".to_owned())];
        words.push(self.name.clone());
        words.extend(self.ticket.clone());
        words.extend(self.tree.clone());
        words.extend(self.class.map(str::to_owned));
        format!(
            "{} for {}: {}",
            words.join(" "),
            span(self.age_s),
            self.doing()
        )
    }
}
