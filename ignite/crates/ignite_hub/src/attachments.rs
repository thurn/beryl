//! The hub's attachment files: one file per image in `attachments/` beside the database, named
//! by the SHA-256 of its bytes, so the same image attached twice is stored once. The rows that
//! say which ticket calls a file what are in `ignite_store::hub::attachments`.
//!
//! An upload is written to a temporary file in the same directory and renamed into place, so a
//! reader never sees half a file. A temporary file left by an upload that failed, and a file no
//! row names, are deleted by upkeep once they are an hour old (`upkeep::attachments`).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use sha2::{Digest, Sha256};

use ignite_core::time::Timestamp;

/// Numbers the temporary files of uploads in flight, so two at once never share one.
static UPLOADS: AtomicU64 = AtomicU64::new(0);

/// The directory of attachment files and the budget their total must stay under.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AttachmentFiles {
    dir: PathBuf,
    budget: u64,
}

/// One file in the directory.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredFile {
    pub name: String,
    pub modified: Timestamp,
}

impl AttachmentFiles {
    pub fn new(dir: PathBuf, budget: u64) -> AttachmentFiles {
        AttachmentFiles { dir, budget }
    }

    /// The files of the hub whose database is `database`: `attachments/` in the same directory.
    pub fn beside(database: &Path, budget: u64) -> AttachmentFiles {
        AttachmentFiles::new(database.with_file_name("attachments"), budget)
    }

    /// The most the stored files may total, in bytes.
    pub fn budget(&self) -> u64 {
        self.budget
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Stores `bytes` and returns their SHA-256. A file already stored is left as it is.
    pub fn write(&self, bytes: &[u8]) -> io::Result<String> {
        let sha256 = sha256_hex(bytes);
        let path = self.dir.join(&sha256);
        if path.is_file() {
            // Upkeep deletes a file no kept attachment names once it is an hour old, so the
            // file this upload reuses is made new until its row is written.
            fs::File::options()
                .append(true)
                .open(&path)?
                .set_modified(std::time::SystemTime::now())?;
            return Ok(sha256);
        }
        fs::create_dir_all(&self.dir)?;
        let n = UPLOADS.fetch_add(1, Ordering::Relaxed);
        let temporary = self.dir.join(format!(".upload-{}-{n}", std::process::id()));
        if let Err(error) =
            fs::write(&temporary, bytes).and_then(|()| fs::rename(&temporary, &path))
        {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Ok(sha256)
    }

    /// The bytes of the file `sha256`.
    pub fn read(&self, sha256: &str) -> io::Result<Vec<u8>> {
        if !is_sha256(sha256) {
            return Err(io::Error::new(io::ErrorKind::NotFound, "not a file name"));
        }
        fs::read(self.dir.join(sha256))
    }

    /// Deletes the file `name`; one already gone is not an error.
    pub fn delete(&self, name: &str) -> io::Result<()> {
        if name.is_empty() || name.contains(['/', '\\']) {
            return Ok(());
        }
        match fs::remove_file(self.dir.join(name)) {
            Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        }
    }

    /// Every file in the directory, with when it was last written; none when the directory does
    /// not exist yet.
    pub fn files(&self) -> io::Result<Vec<StoredFile>> {
        let entries = match fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error),
        };
        let mut files = Vec::new();
        for entry in entries {
            let entry = entry?;
            let metadata = entry.metadata()?;
            if !metadata.is_file() {
                continue;
            }
            let millis = metadata
                .modified()?
                .duration_since(UNIX_EPOCH)
                .map_or(0, |age| i64::try_from(age.as_millis()).unwrap_or(i64::MAX));
            files.push(StoredFile {
                name: entry.file_name().to_string_lossy().into_owned(),
                modified: Timestamp::from_millis(millis),
            });
        }
        files.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(files)
    }
}

/// The SHA-256 of `bytes` in lowercase hexadecimal.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::with_capacity(64), |mut out, byte| {
            let _ = write!(out, "{byte:02x}");
            out
        })
}

/// Whether `text` is a SHA-256 in lowercase hexadecimal, the only names a file is served by.
pub fn is_sha256(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_bytes_are_stored_once_under_their_hash() {
        let dir = ignite_core::test_files::temp_dir()
            .join(format!("ignite-attachment-files-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let files = AttachmentFiles::new(dir.clone(), 100);
        let first = files.write(b"GIF89a").unwrap();
        let second = files.write(b"GIF89a").unwrap();
        assert_eq!(first, second);
        assert!(is_sha256(&first));
        assert_eq!(files.files().unwrap().len(), 1);
        assert_eq!(files.read(&first).unwrap(), b"GIF89a");
        assert!(files.read("../hub.db").is_err());
        files.delete(&first).unwrap();
        files.delete(&first).unwrap();
        assert!(files.files().unwrap().is_empty());
        let _ = fs::remove_dir_all(&dir);
    }
}
