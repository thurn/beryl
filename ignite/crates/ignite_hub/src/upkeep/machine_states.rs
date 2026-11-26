//! Returning a machine to `active` when its `checks-only` or `off` state ends.
//!
//! A session that sets a machine's state gets an end time by default
//! ([`ignite_core::fleet::SESSION_STATE_HOURS`]), so a mitigation cannot outlive the incident
//! that called for it. If the reason still holds when the state ends, the escalations that
//! found it the first time (a slow platform wait, a silent machine) fire again.

use rusqlite::Connection;

use ignite_core::fleet::MachineState;
use ignite_core::time::Timestamp;
use ignite_store::Result;
use ignite_store::hub::fleet_settings::{self, PutOutcome};

/// The actor recorded in the fleet change log for a state that ended.
pub const ACTOR: &str = "ignite upkeep: state ended";

/// Sets each machine whose state has ended back to `active`, and returns their names. A write
/// that races another edit is left to the next pass.
pub fn lapse(conn: &mut Connection, now: Timestamp) -> Result<Vec<String>> {
    let before = fleet_settings::settings(conn)?;
    let lapsed = before.lapsed_states(now);
    if lapsed.is_empty() {
        return Ok(lapsed);
    }
    let mut proposed = before.clone();
    for name in &lapsed {
        if let Some(setting) = proposed.machines.get_mut(name) {
            setting.state = MachineState::Active;
            setting.state_until = None;
        }
    }
    match fleet_settings::put(conn, before.version, proposed, ACTOR, now)? {
        PutOutcome::Stored(_) => Ok(lapsed),
        PutOutcome::Conflict(_) => Ok(Vec::new()),
    }
}

#[cfg(test)]
mod tests {
    use ignite_core::fleet::{FleetSettings, MachineSetting};
    use ignite_store::hub::open_in_memory;

    use super::*;

    fn set(conn: &mut Connection, state: MachineState, until: Option<Timestamp>, now: Timestamp) {
        let before = fleet_settings::settings(conn).unwrap();
        let mut proposed = before.clone();
        proposed.machines.insert(
            "cygnus".to_owned(),
            MachineSetting {
                state,
                state_until: until,
                ..MachineSetting::default()
            },
        );
        fleet_settings::put(conn, before.version, proposed, "cli-claude-1", now).unwrap();
    }

    fn state(conn: &Connection) -> MachineSetting {
        fleet_settings::settings(conn).unwrap().machine("cygnus")
    }

    /// The 2026-11-26 incident: a justiciar set cygnus to checks-only at 12:42. With a
    /// four-hour end, upkeep returns it to active at 16:42 and records who did.
    #[test]
    fn a_session_state_returns_to_active_when_it_ends() {
        let mut conn = open_in_memory().unwrap();
        let set_at = Timestamp::parse("2026-11-26T12:42:50Z").unwrap();
        let ends = set_at.plus_seconds(4.0 * 3600.0);
        set(&mut conn, MachineState::ChecksOnly, Some(ends), set_at);

        assert!(
            lapse(&mut conn, ends.plus_seconds(-60.0))
                .unwrap()
                .is_empty()
        );
        assert_eq!(state(&conn).state, MachineState::ChecksOnly);

        assert_eq!(lapse(&mut conn, ends).unwrap(), vec!["cygnus".to_owned()]);
        assert_eq!(state(&conn), MachineSetting::default());
        assert_eq!(
            fleet_settings::state_actor(&conn, "cygnus")
                .unwrap()
                .as_deref(),
            Some(ACTOR)
        );
        assert!(
            lapse(&mut conn, ends.plus_seconds(60.0))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn a_state_without_an_end_stays() {
        let mut conn = open_in_memory().unwrap();
        let now = Timestamp::parse("2026-11-26T12:00:00Z").unwrap();
        set(&mut conn, MachineState::Off, None, now);
        assert!(lapse(&mut conn, now.plus_seconds(1e6)).unwrap().is_empty());
        assert_eq!(state(&conn).state, MachineState::Off);
    }

    /// A later change of state without an end time (from the dashboard, say) does not
    /// inherit the earlier end, and `active` never keeps one.
    #[test]
    fn a_new_state_drops_the_old_end() {
        let mut conn = open_in_memory().unwrap();
        let now = Timestamp::parse("2026-11-26T12:00:00Z").unwrap();
        let ends = now.plus_seconds(3600.0);
        set(&mut conn, MachineState::ChecksOnly, Some(ends), now);
        let before = fleet_settings::settings(&conn).unwrap();
        let mut proposed: FleetSettings = before.clone();
        proposed.machines.get_mut("cygnus").unwrap().state = MachineState::Off;
        fleet_settings::put(&mut conn, before.version, proposed, "operator", now).unwrap();
        assert_eq!(state(&conn).state_until, None);
        assert!(lapse(&mut conn, ends).unwrap().is_empty());

        set(&mut conn, MachineState::Active, Some(ends), now);
        assert_eq!(state(&conn).state_until, None);
    }
}
