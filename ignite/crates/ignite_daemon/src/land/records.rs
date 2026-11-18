//! Landing records: what a landing pushed, kept on the daemon's disk until the hub knows.
//!
//! A landing writes its record before it pushes, in the state `pushing`. A push the remote
//! accepts moves it to `pushed`; a push that is refused or never reaches the remote deletes it.
//! A `pushed` record is sent to the hub (`POST /landed`, which closes the ticket) and deleted
//! once the hub accepts it. While the hub cannot be reached, the record stays and is sent by a
//! later landing or by the daemon's sender, so a landing is told to the hub exactly once and
//! never pushed a second time to get there.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::hub::Landed;
use ignite_core::traits::{Error, Hub, Result};

/// Where a landing's push stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RecordState {
    /// Written before the push. Found after a restart, it means the daemon stopped mid-push.
    Pushing,
    /// The remote accepted the push; the hub has not been told yet.
    Pushed,
}

/// One landing, as written to disk.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct LandingRecord {
    pub landed: Landed,
    /// The commit pushed to the integration branch.
    pub commit: Commit,
    pub state: RecordState,
}

/// The records of one daemon, one JSON file per ticket in one directory.
#[derive(Clone, Debug)]
pub struct Records {
    dir: PathBuf,
}

impl Records {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    fn path(&self, ticket: &TicketId) -> PathBuf {
        self.dir.join(format!("{ticket}.json"))
    }

    /// The ticket's record, if one is on disk.
    pub fn get(&self, ticket: &TicketId) -> Result<Option<LandingRecord>> {
        match fs::read_to_string(self.path(ticket)) {
            Ok(text) => serde_json::from_str(&text)
                .map(Some)
                .map_err(|e| Error::Invalid(format!("landing record of {ticket}: {e}"))),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    /// Writes the record and flushes it to the disk before returning, through a temporary file
    /// renamed over the old one, so a crash leaves either the old record or the new one.
    pub fn write(&self, record: &LandingRecord) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.path(&record.landed.ticket);
        let temporary = path.with_extension("json.tmp");
        let text = serde_json::to_vec_pretty(record).map_err(|e| Error::Invalid(e.to_string()))?;
        let mut file = fs::File::create(&temporary)?;
        file.write_all(&text)?;
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        Ok(())
    }

    /// Deletes the ticket's record; a missing one is not an error.
    pub fn remove(&self, ticket: &TicketId) -> Result<()> {
        match fs::remove_file(self.path(ticket)) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => Ok(()),
        }
    }

    /// Every record on disk, in the order of their file names.
    pub fn all(&self) -> Result<Vec<LandingRecord>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut names: Vec<String> = entries
            .filter_map(std::result::Result::ok)
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter_map(|name| name.strip_suffix(".json").map(str::to_owned))
            .collect();
        names.sort();
        let mut records = Vec::new();
        for name in names {
            let ticket = TicketId::new(name).map_err(|e| Error::Invalid(e.to_string()))?;
            records.extend(self.get(&ticket)?);
        }
        Ok(records)
    }

    /// Tells the hub about one pushed landing, and deletes its record once the hub accepts it.
    pub fn deliver(&self, hub: &dyn Hub, record: &LandingRecord) -> Result<()> {
        hub.landed(&record.landed)?;
        self.remove(&record.landed.ticket)
    }

    /// Tells the hub about every pushed landing it has not heard of. Returns the tickets it
    /// closed. It stops at the first sign that the hub cannot be reached; a record the hub
    /// refuses for another reason stays on disk, with a message on standard error.
    pub fn deliver_all(&self, hub: &dyn Hub) -> Result<Vec<TicketId>> {
        let mut closed = Vec::new();
        for record in self.all()? {
            if record.state != RecordState::Pushed {
                continue;
            }
            match self.deliver(hub, &record) {
                Ok(()) => closed.push(record.landed.ticket),
                Err(error @ Error::Unreachable(_)) => return Err(error),
                Err(error) => eprintln!(
                    "ignite: the landing of {} was not recorded: {error}",
                    record.landed.ticket
                ),
            }
        }
        Ok(closed)
    }
}
