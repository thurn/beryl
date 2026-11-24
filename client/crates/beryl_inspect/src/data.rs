//! Reads runtime tuning from the checkout for the tools and the app's watcher.

use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use std::time::SystemTime;

use kit_base::data::{Data, profiles};

use crate::recordings;

pub use kit_base::data::TUNING_FILE_NAMES as TUNING_FILES;

pub fn directory() -> PathBuf {
    std::env::var_os("BERYL_CLIENT_DATA")
        .map_or_else(|| recordings::root().join("data/client"), PathBuf::from)
}

/// Loads the tuning files in `directory`. Used by the tools and the source check.
pub fn load(directory: &Path) -> Result<Data, String> {
    Snapshot::read(directory)?.parse()
}

#[derive(PartialEq, Eq)]
struct Contents {
    bytes: Vec<u8>,
    modified: Option<SystemTime>,
}

impl Contents {
    fn read(path: &Path) -> io::Result<Self> {
        let mut file = File::open(path)?;
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)?;
        let modified = file
            .metadata()
            .and_then(|metadata| metadata.modified())
            .ok();
        Ok(Self { bytes, modified })
    }

    fn text(&self, name: &str) -> Result<&str, String> {
        std::str::from_utf8(&self.bytes).map_err(|error| format!("{name}: {error}"))
    }
}

/// One read of the tuning, profiles and effect directory. Comparing reads detects edits
/// even when a file's length and modification time match its previous version.
#[derive(PartialEq, Eq)]
pub struct Snapshot {
    directory: PathBuf,
    tuning: BTreeMap<&'static str, Contents>,
    effects: BTreeMap<String, Contents>,
}

impl Snapshot {
    /// Captures the files without parsing them.
    pub fn read(directory: &Path) -> Result<Self, String> {
        let read = |path: &Path| {
            Contents::read(path).map_err(|error| format!("{}: {error}", path.display()))
        };
        let tuning = TUNING_FILES
            .into_iter()
            .chain(profiles::FILES)
            .map(|name| read(&directory.join(name)).map(|contents| (name, contents)))
            .collect::<Result<_, _>>()?;
        let effects_dir = directory.join("effects");
        let effects_error = |error| format!("{}: {error}", effects_dir.display());
        let mut effects = BTreeMap::new();
        for entry in fs::read_dir(&effects_dir).map_err(effects_error)? {
            let path = entry.map_err(effects_error)?.path();
            if path
                .extension()
                .is_some_and(|extension| extension == "json")
            {
                let name = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .ok_or_else(|| format!("{}: expected a UTF-8 effect name", path.display()))?;
                effects.insert(name.to_owned(), read(&path)?);
            }
        }
        Ok(Self {
            directory: directory.to_owned(),
            tuning,
            effects,
        })
    }

    /// Validates the captured bytes without opening the files again.
    pub fn parse(&self) -> Result<Data, String> {
        let text = |name: &str| {
            self.tuning
                .get(name)
                .ok_or_else(|| format!("{name}: missing tuning file"))?
                .text(name)
        };
        let parse = || {
            let profiles = profiles::FILES
                .into_iter()
                .map(|name| text(name).map(|contents| (name, contents)))
                .collect::<Result<Vec<_>, _>>()?;
            let effects = self
                .effects
                .iter()
                .map(|(name, contents)| {
                    contents
                        .text(&format!("effects/{name}.json"))
                        .map(|contents| (name.as_str(), contents))
                })
                .collect::<Result<Vec<_>, _>>()?;
            Data::parse(|name| text(name).map(str::to_owned), &profiles, effects)
        };
        parse().map_err(|error| format!("{}/{error}", self.directory.display()))
    }
}

/// Shares one validated set with every client started by this tool invocation.
pub fn checkout() -> Result<Arc<Data>, String> {
    checkout_ref().map(Arc::clone)
}

pub(crate) fn checkout_ref() -> Result<&'static Arc<Data>, String> {
    static DATA: OnceLock<Result<Arc<Data>, String>> = OnceLock::new();
    DATA.get_or_init(|| load(&directory()).map(Arc::new))
        .as_ref()
        .map_err(Clone::clone)
}
