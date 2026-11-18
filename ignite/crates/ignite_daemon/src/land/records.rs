//! Landing records: what a landing pushed, kept on the daemon's disk until the hub knows.
//!
//! A landing writes its record before it pushes, in the state `pushing`. A push the remote
//! accepts moves it to `pushed`; a push the remote refuses deletes it. A push that fails with no
//! answer leaves it `pushing`, because the remote may have taken the commit before the failure;
//! the daemon's sender settles such a record once it is old enough (see `resolve`).
//! A `pushed` record is sent to the hub (`POST /landed`, which closes the ticket) and deleted
//! once the hub accepts it. While the hub cannot be reached, the record stays and is sent by a
//! later landing or by the daemon's sender, so a landing is told to the hub exactly once and
//! never pushed a second time to get there.

use std::fs;
use std::io::Write as _;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use ignite_core::ids::{Commit, TicketId, TreeId};
use ignite_core::protocol::hub::Landed;
use ignite_core::time::Timestamp;
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
    /// When the landing wrote the record in the state `pushing`. A record from before this
    /// field existed has none and counts as old.
    #[serde(default)]
    pub written_at: Option<Timestamp>,
    /// The ticket tree the landing came from, so that whoever settles a `pushing` record can
    /// release it. A record from before this field existed has none.
    #[serde(default)]
    pub tree: Option<TreeId>,
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
            // A file that is not UTF-8 is as bad as one that is not JSON.
            Err(error) if error.kind() == std::io::ErrorKind::InvalidData => Err(Error::Invalid(
                format!("landing record of {ticket}: {error}"),
            )),
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

    /// Deletes the ticket's record only if it is for `commit`. A landing that is refused removes
    /// the record it wrote, never one that another landing of the same ticket wrote since.
    pub fn remove_if(&self, ticket: &TicketId, commit: &Commit) -> Result<()> {
        match self.get(ticket) {
            Ok(Some(record)) if record.commit == *commit => self.remove(ticket),
            // A record that cannot be read is not this landing's; `all` sets it aside.
            Ok(_) | Err(Error::Invalid(_)) => Ok(()),
            Err(error) => Err(error),
        }
    }

    /// Every readable record on disk, in the order of their file names. A file that is not a
    /// valid record is moved aside to `<name>.json.bad` and reported on standard error, so one
    /// bad file cannot stop the delivery of the others.
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
            let parsed = TicketId::new(name.clone())
                .map_err(|e| Error::Invalid(e.to_string()))
                .and_then(|ticket| self.get(&ticket));
            match parsed {
                Ok(record) => records.extend(record),
                Err(Error::Io(error)) => return Err(Error::Io(error)),
                Err(error) => self.set_aside(&name, &error),
            }
        }
        Ok(records)
    }

    /// Renames `<name>.json` to `<name>.json.bad` so it is read no more but stays for a person
    /// to look at.
    fn set_aside(&self, name: &str, error: &Error) {
        let from = self.dir.join(format!("{name}.json"));
        let to = self.dir.join(format!("{name}.json.bad"));
        match fs::rename(&from, &to) {
            Ok(()) => eprintln!(
                "ignite: landing record {name}.json is unreadable ({error}); moved to {}",
                to.display()
            ),
            Err(rename) => eprintln!(
                "ignite: landing record {name}.json is unreadable ({error}) and could not be \
                 moved aside: {rename}"
            ),
        }
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
