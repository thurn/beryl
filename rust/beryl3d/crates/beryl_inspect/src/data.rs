//! Loads the tools' tuning once from the checkout that built them.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use kit_base::data::Data;

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
    Data::parse(&space, &gates).map_err(|why| format!("{}/{why}", directory.display()))
}

/// Shares one validated set with every client started by this tool invocation.
pub fn checkout() -> Result<Arc<Data>, String> {
    static DATA: OnceLock<Result<Arc<Data>, String>> = OnceLock::new();
    DATA.get_or_init(|| load(&directory()).map(Arc::new))
        .clone()
}
