use std::fmt::Write as _;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::{Deck, DeckError};

/// Saved deck lists in a directory supplied by the caller.
#[derive(Clone, Debug)]
pub struct Decks(PathBuf);

impl Decks {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Self(directory.into())
    }

    /// Saves a deck by name, replacing an existing deck with that same name.
    pub fn save(&self, deck: &Deck) -> Result<(), DeckError> {
        let path = self.path(&deck.name)?;
        let bytes = serde_json::to_vec_pretty(deck)?;
        fs::create_dir_all(&self.0)?;
        self.replace(&path, &bytes)?;
        Ok(())
    }

    /// Lists the names of saved decks, in sorted order.
    pub fn list(&self) -> Result<Vec<String>, DeckError> {
        let directory = match fs::read_dir(&self.0) {
            Ok(directory) => directory,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(error.into()),
        };
        let mut names = Vec::new();
        for entry in directory {
            let entry = entry?;
            if entry.file_type()?.is_file()
                && let Some(name) = entry.file_name().to_str().and_then(deck_name)
            {
                names.push(name);
            }
        }
        names.sort_unstable();
        Ok(names)
    }

    pub fn load(&self, name: &str) -> Result<Deck, DeckError> {
        Ok(serde_json::from_slice(&fs::read(self.path(name)?)?)?)
    }

    pub fn delete(&self, name: &str) -> Result<(), DeckError> {
        Ok(fs::remove_file(self.path(name)?)?)
    }

    fn path(&self, name: &str) -> Result<PathBuf, DeckError> {
        if name.trim().is_empty() {
            return Err(DeckError::EmptyName);
        }
        // Encode every byte so that punctuation, path separators, Unicode normalization and
        // case-insensitive filesystems cannot make two distinct deck names share a file.
        let mut filename = String::from("deck-");
        for byte in name.bytes() {
            write!(filename, "{byte:02x}").expect("writing to a String cannot fail");
        }
        filename.push_str(".json");
        Ok(self.0.join(filename))
    }

    fn replace(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        // A partial write must not replace the last saved copy. Keep the temporary file
        // beside the destination so the rename stays on the same filesystem.
        let (temporary, mut file) = loop {
            let serial = NEXT.fetch_add(1, Ordering::Relaxed);
            let temporary = self
                .0
                .join(format!(".deck-{}-{serial}.tmp", std::process::id()));
            match OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
            {
                Ok(file) => break (temporary, file),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
                Err(error) => return Err(error),
            }
        };
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        let result = written.and_then(|()| fs::rename(&temporary, path));
        if result.is_err() {
            let _ = fs::remove_file(temporary);
        }
        result
    }
}

fn deck_name(filename: &str) -> Option<String> {
    let hex = filename.strip_prefix("deck-")?.strip_suffix(".json")?;
    let (pairs, remainder) = hex.as_bytes().as_chunks::<2>();
    if pairs.is_empty() || !remainder.is_empty() {
        return None;
    }
    let bytes = pairs
        .iter()
        .map(|&[high, low]| Some(hex_digit(high)? * 16 + hex_digit(low)?))
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes)
        .ok()
        .filter(|name| !name.trim().is_empty())
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}
