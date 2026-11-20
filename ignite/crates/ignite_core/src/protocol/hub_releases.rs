//! Why a ticket was given back (see [`super::Release`]), which decides whether the hub counts
//! the release against the ticket.

use serde::{Deserialize, Serialize};

/// Why a ticket was released. The hub counts a release that says something about the ticket
/// itself, a worker giving up or Ignite taking the ticket back from a worker, and two counted
/// releases by different sessions stop the hub offering the ticket until a justiciar has looked
/// at it. A release caused by the worker's circumstances (its plan limit, a hold on its app, its
/// machine going offline, an emergency) is not counted, because the next worker would not meet
/// the same obstacle.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ReleaseCause {
    /// The worker ran `ignite release` itself. A daemon built before causes were sent says
    /// nothing, which is read as this.
    #[default]
    GaveUp,
    /// Ignite took the ticket back: the worker stopped twice without landing or releasing, it
    /// stalled, or its turns were interrupted too many times in a row.
    TakenBack,
    /// The worker's plan limit was reached.
    PlanLimit,
    /// The worker's app was put on hold on its machine, for example because it was logged out.
    CliHold,
    /// The worker's machine went offline.
    Offline,
    /// An emergency pause ended the work.
    Emergency,
    /// A cause a newer build sent that this one does not know. It is not counted.
    #[serde(other)]
    Unknown,
}

impl ReleaseCause {
    /// Whether the release says something about the ticket, so that the hub counts it.
    #[must_use]
    pub fn counts(self) -> bool {
        matches!(self, ReleaseCause::GaveUp | ReleaseCause::TakenBack)
    }

    /// The cause as it is stored and sent.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            ReleaseCause::GaveUp => "gave-up",
            ReleaseCause::TakenBack => "taken-back",
            ReleaseCause::PlanLimit => "plan-limit",
            ReleaseCause::CliHold => "cli-hold",
            ReleaseCause::Offline => "offline",
            ReleaseCause::Emergency => "emergency",
            ReleaseCause::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ReleaseCause;

    #[test]
    fn only_a_worker_giving_up_or_ignite_taking_the_ticket_back_counts() {
        let counted: Vec<ReleaseCause> = [
            ReleaseCause::GaveUp,
            ReleaseCause::TakenBack,
            ReleaseCause::PlanLimit,
            ReleaseCause::CliHold,
            ReleaseCause::Offline,
            ReleaseCause::Emergency,
            ReleaseCause::Unknown,
        ]
        .into_iter()
        .filter(|cause| cause.counts())
        .collect();
        assert_eq!(counted, [ReleaseCause::GaveUp, ReleaseCause::TakenBack]);
    }

    #[test]
    fn a_cause_reads_back_as_written_and_an_unknown_one_is_not_counted() {
        for cause in [ReleaseCause::PlanLimit, ReleaseCause::TakenBack] {
            let text = serde_json::to_string(&cause).unwrap();
            assert_eq!(text, format!("\"{}\"", cause.as_str()));
            assert_eq!(serde_json::from_str::<ReleaseCause>(&text).unwrap(), cause);
        }
        let newer: ReleaseCause = serde_json::from_str("\"budget\"").unwrap();
        assert_eq!(newer, ReleaseCause::Unknown);
        assert!(!newer.counts());
    }
}
