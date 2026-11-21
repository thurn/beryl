//! The last fleet notice the daemon heard from the hub.

use ignite_core::fleet::FleetNotice;
use ignite_store::local::pool as settings;
use ignite_store::{Result, StoreError};

use crate::pool::Pool;

const KEY: &str = "fleet";

/// The notice that survives a restart, if the hub has sent one.
pub fn current(pool: &Pool) -> Option<FleetNotice> {
    pool.with_database(|conn| {
        settings::setting(conn, KEY)
            .ok()
            .flatten()
            .and_then(|json| serde_json::from_str(&json).ok())
    })
}

/// Saves a received notice. An older hub may omit it, so callers store only present notices.
pub fn store(pool: &Pool, notice: &FleetNotice) -> Result<()> {
    let encoded = serde_json::to_string(notice)
        .map_err(|error| StoreError::Invalid(format!("fleet notice: {error}")))?;
    pool.with_database(|conn| settings::set_setting(conn, KEY, Some(&encoded)))
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
