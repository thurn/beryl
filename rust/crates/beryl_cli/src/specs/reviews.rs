//! Which specs a human has reviewed on the spec page: `spec-reviews.json`, beside the app's bug
//! reports and outside any checkout, so every worktree's `just specs` shares it. It maps each
//! reviewed card's name to when it was marked, and a card stays reviewed until unmarked.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::SystemTime;

/// Reviewed cards by name, each with when it was marked (UTC).
pub type Reviews = BTreeMap<String, String>;

/// Where reviews are kept: `$BERYL_SPEC_REVIEWS`, else
/// `~/Library/Application Support/ca.thurn.beryl/spec-reviews.json`.
pub fn path() -> PathBuf {
    if let Some(path) = std::env::var_os("BERYL_SPEC_REVIEWS") {
        return PathBuf::from(path);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join("Library/Application Support/ca.thurn.beryl/spec-reviews.json")
}

/// The reviews in `path`: none if it is missing or unreadable.
pub fn load(path: &Path) -> Reviews {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default()
}

/// Marks `card` reviewed, or not, in `path`, and returns the reviews after. The server answers
/// each request on its own thread, so writes take turns, and each replaces the file whole.
pub fn set(path: &Path, card: &str, reviewed: bool) -> Result<Reviews, String> {
    static WRITING: Mutex<()> = Mutex::new(());
    let _turn = WRITING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut reviews = load(path);
    if reviewed {
        reviews.insert(
            card.to_owned(),
            beryl_session::journal::utc(SystemTime::now()),
        );
    } else {
        reviews.remove(card);
    }
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let text = serde_json::to_string_pretty(&reviews).map_err(|e| e.to_string())?;
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, text).map_err(|e| format!("{}: {e}", partial.display()))?;
    std::fs::rename(&partial, path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(reviews)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn marks_and_unmarks() {
        let dir = std::env::temp_dir().join(format!("beryl-reviews-{}", std::process::id()));
        let path = dir.join("nested/spec-reviews.json");
        assert!(load(&path).is_empty());
        set(&path, "Llanowar Elves", true).unwrap();
        set(&path, "Sol Ring", true).unwrap();
        let reviews = set(&path, "Llanowar Elves", false).unwrap();
        assert_eq!(reviews.keys().collect::<Vec<_>>(), ["Sol Ring"]);
        assert_eq!(load(&path), reviews);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
