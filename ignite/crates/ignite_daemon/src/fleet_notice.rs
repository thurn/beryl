//! The last fleet notice the daemon heard from the hub.

use ignite_core::fleet::{FleetNotice, MachineState};
use ignite_core::protocol::hub::HeartbeatReply;
use ignite_store::local::pool as settings;
use ignite_store::{Result, StoreError};
use rusqlite::Connection;

use crate::pool::Pool;

const KEY: &str = "fleet";

/// The notice that survives a restart, if the hub has sent one.
pub fn current(pool: &Pool) -> Option<FleetNotice> {
    pool.with_database(|conn| read(conn))
}

pub(crate) fn read(conn: &Connection) -> Option<FleetNotice> {
    settings::setting(conn, KEY)
        .ok()
        .flatten()
        .and_then(|json| serde_json::from_str(&json).ok())
}

/// Applies machine policy sent by the hub. A reply from an older hub has no fleet notice.
pub fn heard(pool: &Pool, reply: &HeartbeatReply) -> Result<()> {
    pool.set_paused_everywhere(reply.paused_everywhere);
    if let Some(notice) = &reply.fleet {
        store(pool, notice)?;
    }
    Ok(())
}

pub(crate) fn state_line(notice: &FleetNotice) -> String {
    match &notice.state_actor {
        Some(actor) => format!("machine is {} (set by {actor})", notice.state),
        None => format!("machine is {}", notice.state),
    }
}

pub(crate) fn is_off(pool: &Pool) -> bool {
    current(pool).is_some_and(|notice| notice.state == MachineState::Off)
}

/// Saves a received notice. An older hub may omit it, so callers store only present notices.
pub fn store(pool: &Pool, notice: &FleetNotice) -> Result<()> {
    pool.with_database(|conn| write(conn, notice))
}

pub(crate) fn write(conn: &Connection, notice: &FleetNotice) -> Result<()> {
    let encoded = serde_json::to_string(notice)
        .map_err(|error| StoreError::Invalid(format!("fleet notice: {error}")))?;
    settings::set_setting(conn, KEY, Some(&encoded))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::fleet::MachineState;

    #[test]
    fn the_daemon_keeps_the_last_notice_on_disk() {
        let path = ignite_core::test_files::temp_dir().join("fleet-notice.db");
        let pool = Pool::new(ignite_store::local::open(&path).unwrap());
        assert_eq!(current(&pool), None);
        let mut notice = FleetNotice {
            version: 1,
            state: MachineState::ChecksOnly,
            ..FleetNotice::default()
        };
        store(&pool, &notice).unwrap();
        assert_eq!(current(&pool), Some(notice.clone()));

        notice.version += 1;
        notice.state = MachineState::Off;
        store(&pool, &notice).unwrap();
        drop(pool);

        let restarted = Pool::new(ignite_store::local::open(&path).unwrap());
        assert_eq!(current(&restarted), Some(notice));
    }
}
