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
    file: File,
    failed: bool,
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
            file,
            metadata,
            failed: false,
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
                file,
                metadata,
                failed: false,
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
        if self.failed {
            return Err(io::Error::other("The table save must be reopened.").into());
        }
        let offset = self.file.stream_position()?;
        let result =
            save::append(&mut self.file, entry).and_then(|()| Ok(self.file.sync_data()?));
        if result.is_err() {
            // Restore the last complete end before accepting any subsequent action.
            // If the file cannot be repaired, leave it to open_for_append on resume.
            self.failed = self
                .file
                .set_len(offset)
                .and_then(|()| self.file.sync_data())
                .and_then(|()| self.file.seek(io::SeekFrom::Start(offset)))
                .is_err();
        }
        result
    }

    pub(super) fn reserve_guest(&mut self, name: &str) -> Result<(), SaveError> {
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

        self.file = staged.persist(&self.path).map_err(|error| error.error)?;
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
