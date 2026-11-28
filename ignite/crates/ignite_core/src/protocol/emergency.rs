//! The emergency pause makes every machine refuse new landings or all new heavy work. The hub
//! keeps the flag, and a session holding an `incident` or `justiciar` ticket can declare it.
//!
//! The holder runs `ignite emergency declare <ticket>`. Daemons hear it
//! in each heartbeat's reply ([`super::hub::HeartbeatReply::emergency`]). They refuse the
//! verbs covered by the scope except for the declaring session and sessions working the
//! emergency ticket or tickets it allows. Running commands continue.
//!
//! The flag lapses [`EMERGENCY_LASTS_S`] after the last declaration.
//! `ignite emergency end` ends it at once, but only for a session that holds the emergency's
//! ticket or names it with `ignite emergency end <ticket>` (see [`ending_refusal`]). It is not
//! the operator's hand-off pause (`ignite pool pause --all`), which is kept apart, so resuming
//! hand-offs never ends an emergency.
//!
//! `ignite emergency adopt <ticket>` moves the emergency in force onto another incident or
//! justiciar ticket that the caller holds, keeping what it pauses, its allowances and when it
//! ends. An incident commander uses it to take over a landings freeze that upkeep declared
//! without losing the freeze's automatic end.

use serde::{Deserialize, Serialize};

use super::tickets::{INCIDENT_LABEL, JUSTICIAR_LABEL};
use crate::ids::{MachineId, SessionId, TicketId};
use crate::time::Timestamp;

/// The ticket labels that grant emergency powers to the holder.
pub const EMERGENCY_LABELS: [&str; 2] = [INCIDENT_LABEL, JUSTICIAR_LABEL];

/// Whether these ticket labels grant emergency powers to the holder.
pub fn has_emergency_label(labels: &[String]) -> bool {
    labels
        .iter()
        .any(|label| EMERGENCY_LABELS.contains(&label.as_str()))
}

/// How long an emergency lasts after it was declared or last renewed: two hours.
pub const EMERGENCY_LASTS_S: f64 = 2.0 * 60.0 * 60.0;

/// Which work pauses during an emergency.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmergencyScope {
    /// New heavy verbs and worker hand-offs.
    #[default]
    All,
    /// New landings only.
    Landings,
}

/// An emergency in force at the hub.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Emergency {
    /// The incident or justiciar ticket where the emergency is tracked.
    pub ticket: TicketId,
    /// The declaring session, which can still run heavy verbs.
    pub session: SessionId,
    /// The machine the session declared it from.
    pub machine: MachineId,
    pub declared_at: Timestamp,
    /// When it was last declared again, which starts its two hours over.
    pub renewed_at: Timestamp,
    /// The work paused by the latest declaration. Older records have a full scope.
    #[serde(default)]
    pub scope: EmergencyScope,
    /// The tickets whose sessions the declaring session lets run heavy verbs.
    #[serde(default)]
    pub allowed: Vec<Allowance>,
    /// Set on the landings freeze that hub upkeep declares: the emergency ends by itself once
    /// `master` holds every landing made before this time. Adopting the emergency keeps it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_master_holds: Option<Timestamp>,
}

/// One exception named by the declaring session. The ticket may run `verb`, or any heavy verb
/// when `verb` is `None`.
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

    /// Whether the emergency ticket or one of its allowances lets `ticket` run `verb`.
    pub fn allows(&self, ticket: &TicketId, verb: &str) -> bool {
        &self.ticket == ticket
            || self
                .allowed
                .iter()
                .any(|a| &a.ticket == ticket && a.verb.as_deref().is_none_or(|v| v == verb))
    }
}

/// Why `caller` may not end `emergency`, or `None` when it may. A session may end an emergency
/// that it declared, or one declared on a ticket it holds (`holds_ticket`), or one whose ticket
/// it names on purpose (`named`). Anyone else is probably ending an emergency that another
/// session declared after the one they know of.
pub fn ending_refusal(
    emergency: &Emergency,
    caller: Option<&SessionId>,
    holds_ticket: bool,
    named: Option<&TicketId>,
) -> Option<String> {
    if holds_ticket || named == Some(&emergency.ticket) || caller == Some(&emergency.session) {
        return None;
    }
    Some(format!(
        "refused  emergency {} (declared by {} on {}) is in force, and it was declared on a \
         ticket you do not hold. It may not be the emergency you declared. To end it anyway, \
         name its ticket: `ignite emergency end {}`",
        emergency.ticket, emergency.session, emergency.machine, emergency.ticket
    ))
}

/// `POST /emergency`: one change to the emergency. The reply is an [`EmergencyReply`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "change", rename_all = "snake_case")]
pub enum EmergencyChange {
    /// Declares an emergency from `session` on `machine`, or renews the one in force. A
    /// declaration from another session takes the emergency over, as when the operator appoints
    /// a new incident commander. The new declaration starts with empty allowances.
    Declare {
        ticket: TicketId,
        session: SessionId,
        machine: MachineId,
        #[serde(default)]
        scope: EmergencyScope,
    },
    /// Adds an allowance. Only the emergency's own session may.
    Allow {
        session: SessionId,
        allowance: Allowance,
    },
    /// Moves the emergency in force onto `ticket`, an incident or justiciar ticket that
    /// `session` holds, keeping its scope, its allowances, its end condition and when it
    /// lapses. The ticket it leaves is allowed every heavy verb, as it was while it held the
    /// emergency.
    Adopt {
        ticket: TicketId,
        session: SessionId,
        machine: MachineId,
    },
    /// Ends the emergency in force, unless [`ending_refusal`] refuses `session`. `ticket` is
    /// the emergency the caller means to end, named on purpose.
    End {
        #[serde(default)]
        session: Option<SessionId>,
        #[serde(default)]
        ticket: Option<TicketId>,
    },
}

/// The reply to `POST /emergency`: the emergency after the change, or `None` once it has ended.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EmergencyReply {
    pub emergency: Option<Emergency>,
}

/// The emergency `change` makes of `current` at `now`, `None` once it has ended, or why it is
/// refused. `holds_ticket` says whether the caller holds the emergency's ticket, which lets it
/// end the emergency. The hub and the fakes both apply changes this way.
pub fn apply(
    current: Option<Emergency>,
    change: &EmergencyChange,
    now: Timestamp,
    holds_ticket: bool,
) -> Result<Option<Emergency>, String> {
    let current = current.filter(|e| e.active(now));
    match change {
        EmergencyChange::End { session, ticket } => {
            let Some(current) = current else {
                return Ok(None);
            };
            match ending_refusal(&current, session.as_ref(), holds_ticket, ticket.as_ref()) {
                Some(refusal) => Err(refusal),
                None => Ok(None),
            }
        }
        EmergencyChange::Adopt {
            ticket,
            session,
            machine,
        } => {
            let Some(mut emergency) = current else {
                return Err("no emergency is in force to adopt".to_owned());
            };
            if &emergency.ticket != ticket {
                let left = Allowance {
                    ticket: emergency.ticket.clone(),
                    verb: None,
                };
                if !emergency.allowed.contains(&left) {
                    emergency.allowed.push(left);
                }
            }
            emergency.ticket = ticket.clone();
            emergency.session = session.clone();
            emergency.machine = machine.clone();
            Ok(Some(emergency))
        }
        other => declare_or_allow(current, other, now).map(Some),
    }
}

fn declare_or_allow(
    current: Option<Emergency>,
    change: &EmergencyChange,
    now: Timestamp,
) -> Result<Emergency, String> {
    match change {
        EmergencyChange::Declare {
            ticket,
            session,
            machine,
            scope,
        } => Ok(match current {
            Some(mut same) if &same.session == session && &same.ticket == ticket => {
                same.renewed_at = now;
                same.scope = *scope;
                same
            }
            _ => Emergency {
                ticket: ticket.clone(),
                session: session.clone(),
                machine: machine.clone(),
                declared_at: now,
                renewed_at: now,
                scope: *scope,
                allowed: Vec::new(),
                until_master_holds: None,
            },
        }),
        EmergencyChange::Allow { session, allowance } => {
            let Some(mut emergency) = current else {
                return Err("no emergency is in force".to_owned());
            };
            if &emergency.session != session {
                return Err(format!(
                    "only the declaring session {} may allow tickets. It declared emergency {}, \
                     which is in force now; another session may have declared one since",
                    emergency.session, emergency.ticket
                ));
            }
            if !emergency.allowed.contains(allowance) {
                emergency.allowed.push(allowance.clone());
            }
            Ok(emergency)
        }
        EmergencyChange::Adopt { .. } | EmergencyChange::End { .. } => {
            unreachable!("`apply` handles adoption and ending")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The emergency a declaration or allowance leaves.
    fn applied(
        current: Option<Emergency>,
        change: &EmergencyChange,
        now: Timestamp,
    ) -> Result<Emergency, String> {
        apply(current, change, now, false).map(|e| e.expect("the change leaves an emergency"))
    }

    fn declare(session: &str, ticket: &str) -> EmergencyChange {
        EmergencyChange::Declare {
            ticket: TicketId::new(ticket).unwrap(),
            session: SessionId::new(session).unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            scope: EmergencyScope::All,
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
        let first = applied(None, &declare("local_j", "TJUS1"), start).unwrap();
        assert!(first.active(start.plus_seconds(EMERGENCY_LASTS_S - 1.0)));
        assert!(!first.active(start.plus_seconds(EMERGENCY_LASTS_S)));
        let later = start.plus_seconds(3600.0);
        let renewed = applied(Some(first), &declare("local_j", "TJUS1"), later).unwrap();
        assert_eq!(renewed.declared_at, start);
        assert!(renewed.active(start.plus_seconds(EMERGENCY_LASTS_S + 60.0)));
    }

    #[test]
    fn a_renewal_changes_scope_and_keeps_allowances() {
        let now = Timestamp::EPOCH;
        let change = |scope| EmergencyChange::Declare {
            ticket: TicketId::new("TJUS1").unwrap(),
            session: SessionId::new("local_j").unwrap(),
            machine: MachineId::new("m-1").unwrap(),
            scope,
        };
        let first = applied(None, &change(EmergencyScope::Landings), now).unwrap();
        assert_eq!(first.scope, EmergencyScope::Landings);
        let allowed = applied(Some(first), &allow("local_j", "TWORK", Some("land")), now).unwrap();
        let renewed = applied(
            Some(allowed),
            &change(EmergencyScope::All),
            now.plus_seconds(60.0),
        )
        .unwrap();
        assert_eq!(renewed.scope, EmergencyScope::All);
        assert_eq!(renewed.declared_at, now);
        assert!(renewed.allows(&TicketId::new("TWORK").unwrap(), "land"));
    }

    #[test]
    fn old_declarations_and_records_default_to_a_full_pause() {
        let old = serde_json::json!({
            "ticket": "TJUS1",
            "session": "local_j",
            "machine": "m-1",
        });
        let mut declaration = old.clone();
        declaration["change"] = serde_json::json!("declare");
        assert_eq!(
            serde_json::from_value::<EmergencyChange>(declaration).unwrap(),
            declare("local_j", "TJUS1")
        );

        let mut record = old;
        record["declared_at"] = serde_json::json!(Timestamp::EPOCH);
        record["renewed_at"] = serde_json::json!(Timestamp::EPOCH);
        assert_eq!(
            serde_json::from_value::<Emergency>(record).unwrap().scope,
            EmergencyScope::All
        );
    }

    #[test]
    fn only_the_declaring_session_grants_allowances_for_one_or_every_verb() {
        let now = Timestamp::EPOCH;
        let emergency = applied(None, &declare("local_j", "TJUS1"), now).unwrap();
        let refused = applied(
            Some(emergency.clone()),
            &allow("local_w", "TWORK", None),
            now,
        );
        assert!(refused.unwrap_err().contains("local_j"));
        let emergency = applied(
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
    fn only_the_holder_or_a_named_ticket_ends_an_emergency() {
        let now = Timestamp::EPOCH;
        let emergency = applied(None, &declare("local_j", "TJUS1"), now).unwrap();
        let other = SessionId::new("local_w").unwrap();
        let ticket = TicketId::new("TJUS1").unwrap();
        let refusal = ending_refusal(&emergency, Some(&other), false, None).unwrap();
        assert!(
            refusal.contains("TJUS1") && refusal.contains("local_j"),
            "{refusal}"
        );
        assert!(ending_refusal(&emergency, None, false, None).is_some());
        let wrong = TicketId::new("TOTHR").unwrap();
        assert!(ending_refusal(&emergency, Some(&other), false, Some(&wrong)).is_some());
        assert!(ending_refusal(&emergency, Some(&other), false, Some(&ticket)).is_none());
        assert!(ending_refusal(&emergency, Some(&other), true, None).is_none());
        assert!(ending_refusal(&emergency, Some(&emergency.session), false, None).is_none());
    }

    #[test]
    fn a_lapsed_emergency_is_declared_afresh_and_cannot_be_added_to() {
        let start = Timestamp::EPOCH;
        let old = applied(None, &declare("local_j", "TJUS1"), start).unwrap();
        let later = start.plus_seconds(EMERGENCY_LASTS_S + 1.0);
        assert!(applied(Some(old.clone()), &allow("local_j", "TWORK", None), later).is_err());
        let fresh = applied(Some(old), &declare("local_j", "TJUS1"), later).unwrap();
        assert_eq!(fresh.declared_at, later);
    }

    #[test]
    fn adopting_keeps_scope_allowances_and_end_and_allows_the_ticket_left() {
        let now = Timestamp::EPOCH;
        let mut freeze = applied(None, &declare("ignite-upkeep", "JFRZ1"), now).unwrap();
        freeze.scope = EmergencyScope::Landings;
        freeze.until_master_holds = Some(now);
        let freeze = applied(Some(freeze), &allow("ignite-upkeep", "RFIX1", None), now).unwrap();
        let adopt = EmergencyChange::Adopt {
            ticket: TicketId::new("IINC1").unwrap(),
            session: SessionId::new("local_i").unwrap(),
            machine: MachineId::new("m-2").unwrap(),
        };
        let later = now.plus_seconds(60.0);
        let adopted = apply(Some(freeze.clone()), &adopt, later, false)
            .unwrap()
            .unwrap();
        assert_eq!(adopted.ticket.as_str(), "IINC1");
        assert_eq!(adopted.session.as_str(), "local_i");
        assert_eq!(adopted.scope, EmergencyScope::Landings);
        assert_eq!(adopted.until_master_holds, Some(now));
        assert_eq!(adopted.renewed_at, freeze.renewed_at);
        assert!(adopted.allows(&TicketId::new("RFIX1").unwrap(), "land"));
        assert!(adopted.allows(&TicketId::new("JFRZ1").unwrap(), "land"));
        assert!(apply(None, &adopt, later, false).is_err());
    }

    #[test]
    fn ending_follows_the_ending_refusal() {
        let now = Timestamp::EPOCH;
        let emergency = applied(None, &declare("local_j", "TJUS1"), now).unwrap();
        let end = |session: &str, ticket: Option<&str>| EmergencyChange::End {
            session: Some(SessionId::new(session).unwrap()),
            ticket: ticket.map(|t| TicketId::new(t).unwrap()),
        };
        assert!(apply(Some(emergency.clone()), &end("local_w", None), now, false).is_err());
        assert_eq!(
            apply(
                Some(emergency.clone()),
                &end("local_w", Some("TJUS1")),
                now,
                false
            ),
            Ok(None)
        );
        assert_eq!(
            apply(Some(emergency), &end("local_w", None), now, true),
            Ok(None)
        );
        assert_eq!(apply(None, &end("local_w", None), now, false), Ok(None));
    }
}
