//! What the express lane remembers of the runs that passed (see "An express run that passes"
//! in [`super`]).

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::Connection;

use ignite_core::ids::Commit;
use ignite_core::time::Timestamp;
use ignite_core::traits::Clock;
use ignite_store::local::express_passes;

use super::EarlierPass;

/// How long a passing run is remembered, in seconds.
const REMEMBER_S: f64 = 60.0 * 60.0;

/// What the express lane remembers of the runs that passed: the daemon's database, the clock
/// and a digest of the `[express]` section, so that an edited section never matches a pass.
pub struct PassMemory {
    pub db: Arc<Mutex<Connection>>,
    pub clock: Arc<dyn Clock>,
}

impl PassMemory {
    pub fn new(db: Arc<Mutex<Connection>>, clock: Arc<dyn Clock>) -> Self {
        Self { db, clock }
    }

    /// The digest of `express`, covering every setting that decides what runs where, with the
    /// machine's `cores`. It is of the section a run used, which is the tree's own.
    pub fn digest(express: &ignite_core::config::Express, cores: u32) -> String {
        // FNV-1a over the serialized section: stable across runs and Rust versions, unlike
        // the standard library's hasher.
        let text = serde_json::to_string(express).unwrap_or_default() + &cores.to_string();
        let mut digest: u64 = 0xcbf2_9ce4_8422_2325;
        for byte in text.bytes() {
            digest ^= u64::from(byte);
            digest = digest.wrapping_mul(0x0100_0000_01b3);
        }
        format!("{digest:016x}")
    }

    /// How many seconds ago the commands passed on `tree_id`, if they did within the hour.
    pub(super) fn passed_ago(&self, tree_id: &str, section: &str) -> Option<f64> {
        let now = self.clock.now();
        let since = now.plus_seconds(-REMEMBER_S).to_string();
        let db = self.db.lock().unwrap_or_else(PoisonError::into_inner);
        let at = express_passes::passed_at(&db, tree_id, section, &since).ok()??;
        let at = Timestamp::parse(&at).ok()?;
        Some(now.seconds_since(at).max(0.0))
    }

    /// Remembers that the commands passed on `tree_id`, in `place` when it is known. A failure
    /// to write is ignored: the next landing then runs the commands, as it did before there was
    /// a memory.
    pub(super) fn remember(
        &self,
        tree_id: &str,
        section: &str,
        place: Option<&express_passes::Place>,
    ) {
        let now = self.clock.now();
        let forget = now.plus_seconds(-REMEMBER_S).to_string();
        let db = self.db.lock().unwrap_or_else(PoisonError::into_inner);
        let at = now.to_string();
        let _ = express_passes::record(&db, tree_id, section, &at, &forget, place);
    }

    /// The last pass of the section with digest `section` remembered within the hour for the
    /// working tree at `tree`.
    pub(super) fn last_in(&self, tree: &Path, section: &str) -> Option<EarlierPass> {
        let now = self.clock.now();
        let since = now.plus_seconds(-REMEMBER_S).to_string();
        let path = tree.to_string_lossy();
        let db = self.db.lock().unwrap_or_else(PoisonError::into_inner);
        let last = express_passes::last_in(&db, &path, section, &since).ok()??;
        let at = Timestamp::parse(&last.at).ok()?;
        Some(EarlierPass {
            head: Commit::new(last.place.head).ok()?,
            changed: last.place.changed,
            ago_s: now.seconds_since(at).max(0.0),
        })
    }
}
