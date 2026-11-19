//! The emergency pause: a flag the justiciar sets at the hub (`POST /emergency`) that makes every
//! machine refuse the verbs that start heavy work until the emergency ends.
//!
//! The justiciar's session declares it with `ignite emergency declare <ticket>`. Daemons hear it
//! in each heartbeat's reply ([`super::hub::HeartbeatReply::emergency`]) and refuse `start`,
//! `test`, `check`, `land` and the other verbs that build, from every session except the
//! justiciar's and the tickets it allows. Commands already running are not stopped.
//!
//! A flag nobody renews lapses [`EMERGENCY_LASTS_S`] after it was last declared, so a justiciar
//! that dies cannot leave the fleet stopped. `ignite pool resume --all` ends it at once.

use serde::{Deserialize, Serialize};

use crate::ids::{MachineId, SessionId, TicketId};
use crate::time::Timestamp;

/// How long an emergency lasts after it was declared or last renewed: two hours.
pub const EMERGENCY_LASTS_S: f64 = 2.0 * 60.0 * 60.0;

/// An emergency in force at the hub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Emergency {
    /// The justiciar ticket the emergency is worked on.
    pub ticket: TicketId,
    /// The justiciar's session, which is never refused.
    pub session: SessionId,
    /// The machine the justiciar declared it from.
    pub machine: MachineId,
    pub declared_at: Timestamp,
    /// When it was last declared again, which starts its two hours over.
    pub renewed_at: Timestamp,
    /// The tickets whose sessions the justiciar lets run heavy verbs.
    #[serde(default)]
    pub allowed: Vec<Allowance>,
}

/// One exception the justiciar named: `ticket` may run `verb`, or any heavy verb when `verb` is
/// `None`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Allowance {
    pub ticket: TicketId,
    #[serde(default)]
    pub verb: Option<String>,
}

impl Emergency {
    /// When the emergency lapses unless it is renewed first.
    pub fn expires_at(&self) -> Timestamp {
        self.renewed_at.plus_seconds(EMERGENCY_LASTS_S)
    }

    /// Whether it is still in force at `now`.
    pub fn active(&self, now: Timestamp) -> bool {
        now < self.expires_at()
    }

    /// Whether the justiciar's ticket or one of its allowances lets `ticket` run `verb`.
    pub fn allows(&self, ticket: &TicketId, verb: &str) -> bool {
        &self.ticket == ticket
            || self
                .allowed
                .iter()
                .any(|a| &a.ticket == ticket && a.verb.as_deref().is_none_or(|v| v == verb))
    }
}

/// `POST /emergency`: one change to the emergency. The reply is an [`EmergencyReply`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum EmergencyChange {
    /// Declares an emergency from `session` on `machine`, or renews the one in force. A
    /// declaration from another session takes the emergency over, as when the operator hands it
    /// to a new justiciar; its allowances start empty.
    Declare {
        ticket: TicketId,
        session: SessionId,
        machine: MachineId,
    },
    /// Adds an allowance. Only the emergency's own session may.
    Allow {
        session: SessionId,
        allowance: Allowance,
    },
}

/// The reply to `POST /emergency`: the emergency after the change.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmergencyReply {
    pub emergency: Option<Emergency>,
}

/// The emergency `change` makes of `current` at `now`, or why it is refused. The hub and the
/// fakes both apply changes this way.
pub fn apply(
    current: Option<Emergency>,
    change: &EmergencyChange,
    now: Timestamp,
) -> Result<Emergency, String> {
    let current = current.filter(|e| e.active(now));
    match change {
        EmergencyChange::Declare {
            ticket,
            session,
            machine,
        } => Ok(match current {
            Some(mut same) if &same.session == session && &same.ticket == ticket => {
                same.renewed_at = now;
                same
            }
            _ => Emergency {
                ticket: ticket.clone(),
                session: session.clone(),
                machine: machine.clone(),
                declared_at: now,
                renewed_at: now,
                allowed: Vec::new(),
            },
        }),
        EmergencyChange::Allow { session, allowance } => {
            let Some(mut emergency) = current else {
                return Err("no emergency is in force".to_owned());
            };
            if &emergency.session != session {
                return Err(format!(
                    "only the justiciar's session {} may allow tickets",
                    emergency.session
                ));
            }
            if !emergency.allowed.contains(allowance) {
                emergency.allowed.push(allowance.clone());
            }
            Ok(emergency)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn declare(session: &str, ticket: &str) -> EmergencyChange {
        EmergencyChange::Declare {
            ticket: TicketId::new(ticket).unwrap(),
            session: SessionId::new(session).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
        }
    }

    fn allow(session: &str, ticket: &str, verb: Option<&str>) -> EmergencyChange {
        EmergencyChange::Allow {
            session: SessionId::new(session).unwrap(),
            allowance: Allowance {
                ticket: TicketId::new(ticket).unwrap(),
                verb: verb.map(str::to_owned),
            },
        }
    }

    #[test]
    fn a_declaration_lasts_two_hours_and_renewing_starts_them_over() {
        let start = Timestamp::EPOCH;
        let first = apply(None, &declare("local_j", "TJUS1"), start).unwrap();
        assert!(first.active(start.plus_seconds(EMERGENCY_LASTS_S - 1.0)));
        assert!(!first.active(start.plus_seconds(EMERGENCY_LASTS_S)));
        let later = start.plus_seconds(3600.0);
        let renewed = apply(Some(first), &declare("local_j", "TJUS1"), later).unwrap();
        assert_eq!(renewed.declared_at, start);
        assert!(renewed.active(start.plus_seconds(EMERGENCY_LASTS_S + 60.0)));
    }

    #[test]
    fn only_the_justiciar_allows_and_an_allowance_names_a_verb_or_every_verb() {
        let now = Timestamp::EPOCH;
        let emergency = apply(None, &declare("local_j", "TJUS1"), now).unwrap();
        let refused = apply(
            Some(emergency.clone()),
            &allow("local_w", "TWORK", None),
            now,
        );
        assert!(refused.unwrap_err().contains("local_j"));
        let emergency = apply(
            Some(emergency),
            &allow("local_j", "TWORK", Some("land")),
            now,
        )
        .unwrap();
        let work = TicketId::new("TWORK").unwrap();
        assert!(emergency.allows(&work, "land"));
        assert!(!emergency.allows(&work, "test"));
        assert!(emergency.allows(&TicketId::new("TJUS1").unwrap(), "test"));
    }

    #[test]
    fn a_lapsed_emergency_is_declared_afresh_and_cannot_be_added_to() {
        let start = Timestamp::EPOCH;
        let old = apply(None, &declare("local_j", "TJUS1"), start).unwrap();
        let later = start.plus_seconds(EMERGENCY_LASTS_S + 1.0);
        assert!(apply(Some(old.clone()), &allow("local_j", "TWORK", None), later).is_err());
        let fresh = apply(Some(old), &declare("local_j", "TJUS1"), later).unwrap();
        assert_eq!(fresh.declared_at, later);
    }
}
