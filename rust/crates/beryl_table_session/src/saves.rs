//! Hosted table files and the information needed to reopen their listeners.

use std::fs::{self, File, OpenOptions};
use std::hash::{BuildHasher, RandomState};
use std::io::{self, BufRead, BufReader, Seek, Write};
use std::path::{Path, PathBuf};

use beryl_protocol::ipc::SavedTable;
use beryl_table::save::{self, SaveError};
use beryl_table::{Chance, Entry, Log, Table};
use serde::{Deserialize, Serialize};

/// Settings and the original transport invite for a new hosted table.
#[derive(Clone, Serialize, Deserialize)]
pub struct HostSettings {
    pub name: String,
    pub invite: String,
    pub secret: String,
    pub starting_life: i32,
    pub starting_hand_size: u16,
}

impl HostSettings {
    pub fn new(
        name: impl Into<String>,
        invite: impl Into<String>,
        secret: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            invite: invite.into(),
            secret: secret.into(),
            starting_life: 20,
            starting_hand_size: 7,
        }
    }
}

#[derive(Serialize, Deserialize)]
pub(super) struct Metadata {
    pub(super) settings: HostSettings,
    pub(super) guest: Option<String>,
}

pub(super) struct Save {
    path: PathBuf,
    file: Option<File>,
    pub(super) metadata: Metadata,
}

impl Save {
    pub(super) fn create(
        directory: &Path,
        settings: HostSettings,
        table: &Table,
        seed: u64,
    ) -> Result<Self, SaveError> {
        fs::create_dir_all(directory)?;
        let metadata = Metadata {
            settings,
            guest: None,
        };
        let fields = serde_json::to_value(&metadata)?;
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        save::write_header_with_metadata(&mut staged, table, seed, Some(fields))?;
        staged.as_file().sync_all()?;
        let id = format!("{:016x}", RandomState::new().hash_one(seed));
        let path = path_for(directory, &id)?;
        let file = staged
            .persist_noclobber(&path)
            .map_err(|error| error.error)?;
        Ok(Self {
            path,
            file: Some(file),
            metadata,
        })
    }

    pub(super) fn open(
        directory: &Path,
        id: &str,
    ) -> Result<(Self, Table, Log, Chance), SaveError> {
        let path = path_for(directory, id)?;
        let mut file = OpenOptions::new().read(true).write(true).open(&path)?;
        let metadata = read_metadata(&file)?;
        let (table, log, chance) = save::open_for_append(&mut file)?;
        Ok((
            Self {
                path,
                file: Some(file),
                metadata,
            },
            table,
            log,
            chance,
        ))
    }

    pub(super) fn id(&self) -> &str {
        self.path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .expect("generated table id")
    }

    pub(super) fn append(&mut self, entry: &Entry) -> Result<(), SaveError> {
        let Some(file) = &mut self.file else {
            return Err(io::Error::other("The table save must be reopened.").into());
        };
        let offset = file.stream_position()?;
        let result = save::append(&mut *file, entry).and_then(|()| Ok(file.sync_data()?));
        // Restore the last complete end before accepting any subsequent action.
        // If the file cannot be repaired, leave it to open_for_append on resume.
        if result.is_err()
            && file
                .set_len(offset)
                .and_then(|()| file.sync_data())
                .and_then(|()| file.seek(io::SeekFrom::Start(offset)))
                .is_err()
        {
            self.file = None;
        }
        result
    }

    pub(super) fn reserve_guest(&mut self, name: &str) -> Result<(), SaveError> {
        if self.file.is_none() {
            return Err(io::Error::other("The table save must be reopened.").into());
        }
        let mut current = BufReader::new(File::open(&self.path)?);
        let mut header = read_header(&mut current)?;
        header["metadata"]["guest"] = serde_json::Value::String(name.to_owned());

        // A guest can redial as soon as Welcome arrives. Persist the name first
        // and replace the header atomically so interruption leaves a usable save.
        let directory = self
            .path
            .parent()
            .expect("save is inside the tables directory");
        let mut staged = tempfile::NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut staged, &header)?;
        staged.write_all(b"\n")?;
        io::copy(&mut current, &mut staged)?;
        staged.as_file().sync_all()?;

        // Windows requires both handles to the destination to be closed before
        // persist can replace it. If replacement fails, continue with the old save.
        drop(current);
        drop(self.file.take());
        match staged.persist(&self.path) {
            Ok(file) => self.file = Some(file),
            Err(error) => {
                self.file = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .open(&self.path)
                    .and_then(|mut file| {
                        file.seek(io::SeekFrom::End(0))?;
                        Ok(file)
                    })
                    .ok();
                return Err(error.error.into());
            }
        }
        self.metadata.guest = Some(name.to_owned());
        Ok(())
    }
}

pub(super) fn list(directory: &Path) -> Result<Vec<SavedTable>, SaveError> {
    let entries = match fs::read_dir(directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    let mut tables = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_file()
            && path.extension().is_some_and(|ext| ext == "jsonl")
            && let Some(id) = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .filter(|id| valid_id(id))
        {
            // An unreadable save should not hide the other tables on the start screen.
            match File::open(&path)
                .map_err(SaveError::from)
                .and_then(read_metadata)
            {
                Ok(metadata) => tables.push(SavedTable {
                    id: id.to_owned(),
                    host: metadata.settings.name,
                    guest: metadata.guest,
                }),
                Err(error) => eprintln!("Cannot read table {id}: {error}"),
            }
        }
    }
    tables.sort_unstable_by(|left, right| left.id.cmp(&right.id));
    Ok(tables)
}

pub(super) fn invite(directory: &Path, id: &str) -> Result<String, SaveError> {
    Ok(read_metadata(File::open(path_for(directory, id)?)?)?
        .settings
        .invite)
}

fn valid_id(id: &str) -> bool {
    id.len() == 16
        && id
            .bytes()
            .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
}

fn path_for(directory: &Path, id: &str) -> io::Result<PathBuf> {
    if !valid_id(id) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Invalid table id.",
        ));
    }
    Ok(directory.join(format!("{id}.jsonl")))
}

fn read_metadata(reader: impl io::Read) -> Result<Metadata, SaveError> {
    let header = read_header(BufReader::new(reader))?;
    Ok(serde_json::from_value(
        header
            .get("metadata")
            .cloned()
            .ok_or(SaveError::MissingHeader)?,
    )?)
}

fn read_header(mut reader: impl BufRead) -> Result<serde_json::Value, SaveError> {
    let mut line = Vec::new();
    reader.read_until(b'\n', &mut line)?;
    if !line.ends_with(b"\n") {
        return Err(SaveError::MissingHeader);
    }
    Ok(serde_json::from_slice(&line)?)
}

#[cfg(all(test, windows))]
mod tests {
    use std::os::windows::fs::OpenOptionsExt;

    use beryl_table::action::Adjustment;
    use beryl_table::{Action, Seat};

    use super::*;

    #[test]
    fn failed_guest_reservation_preserves_the_log_and_allows_retry() {
        const FILE_SHARE_READ_WRITE: u32 = 0x0003;
        let directory = tempfile::tempdir().unwrap();
        let mut table = Table::new(2);
        let mut log = Log::new(&table);
        let mut chance = Chance::fresh(42);
        let settings = HostSettings::new("Host", "invite", "secret");
        let mut save = Save::create(directory.path(), settings, &table, 42).unwrap();
        let id = save.id().to_owned();
        let action = Action::Life {
            seat: Seat(0),
            adjustment: Adjustment::Add(1),
        };
        let entry = log
            .apply(&mut table, Seat(0), &action, &mut chance)
            .unwrap();
        save.append(entry).unwrap();
        let before = fs::read(&save.path).unwrap();

        // Allow reads and writes to the original, but deny replacing it.
        let reader = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ_WRITE)
            .open(&save.path)
            .unwrap();
        assert!(save.reserve_guest("Guest").is_err());
        assert!(save.metadata.guest.is_none());
        assert_eq!(fs::read(&save.path).unwrap(), before);
        let entry = log
            .apply(&mut table, Seat(0), &action, &mut chance)
            .unwrap();
        save.append(entry).unwrap();
        drop(reader);

        save.reserve_guest("Guest").unwrap();
        drop(save);
        let (save, _, replayed, _) = Save::open(directory.path(), &id).unwrap();
        assert_eq!(save.metadata.guest.as_deref(), Some("Guest"));
        assert_eq!(replayed.entries(), log.entries());
    }
}
