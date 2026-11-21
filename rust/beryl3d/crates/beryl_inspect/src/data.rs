//! Loads the tools' tuning once from the checkout that built them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use kit_base::data::{Data, profiles};

use crate::recordings;

pub fn directory() -> PathBuf {
    std::env::var_os("BERYL3D_DATA")
        .map_or_else(|| recordings::root().join("data/beryl3d"), PathBuf::from)
}

/// Loads the tuning files in `directory`. Used by the tools and the source check.
pub fn load(directory: &Path) -> Result<Data, String> {
    let read = |name| {
        let path = directory.join(name);
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))
    };
    let space = read("space.json")?;
    let gates = read("gates.json")?;
    let files: Vec<_> = profiles::FILES
        .iter()
        .map(|&name| read(name).map(|text| (name, text)))
        .collect::<Result<_, _>>()?;
    let contents: Vec<_> = files
        .iter()
        .map(|(name, text)| (*name, text.as_str()))
        .collect();
    let effects_dir = directory.join("effects");
    let entries = std::fs::read_dir(&effects_dir)
        .map_err(|error| format!("{}: {error}", effects_dir.display()))?;
    let mut effects = Vec::new();
    for entry in entries {
        let path = entry.map_err(|error| error.to_string())?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let name = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .ok_or_else(|| format!("{}: expected a UTF-8 effect name", path.display()))?;
            let text = std::fs::read_to_string(&path)
                .map_err(|error| format!("{}: {error}", path.display()))?;
            effects.push((name.to_owned(), text));
        }
    }
    effects.sort_unstable_by(|a, b| a.0.cmp(&b.0));
    Data::parse(
        &space,
        &gates,
        &contents,
        effects
            .iter()
            .map(|(name, text)| (name.as_str(), text.as_str())),
    )
    .map_err(|why| format!("{}/{why}", directory.display()))
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
