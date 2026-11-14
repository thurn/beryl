//! The app instance's profile, selected once from the environment.

use std::ffi::OsStr;
use std::path::PathBuf;
use std::sync::LazyLock;

use beryl_session::{bugs, journal};
use serde_json::json;
use tauri::{AppHandle, Manager};

const ENV: &str = "BERYL_PROFILE";

static PROFILE: LazyLock<Result<Option<String>, InvalidName>> =
    LazyLock::new(|| std::env::var_os(ENV).as_deref().map(parse_name).transpose());

#[derive(Debug)]
struct InvalidName;

fn parse_name(value: &OsStr) -> Result<String, InvalidName> {
    let name = value.to_str().ok_or(InvalidName)?;
    if name.is_empty()
        || name.len() > 255
        || !name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
    {
        return Err(InvalidName);
    }
    // The volumes macOS uses by default fold case. Use the same name for the folder and WebView.
    let name = name.to_ascii_lowercase();
    let device = matches!(name.as_str(), "con" | "prn" | "aux" | "nul")
        || matches!(
            name.as_bytes(),
            [b'c', b'o', b'm', b'1'..=b'9'] | [b'l', b'p', b't', b'1'..=b'9']
        );
    if device { Err(InvalidName) } else { Ok(name) }
}

fn warn_if_invalid() {
    if PROFILE.is_err() {
        journal::warn(
            "app",
            "Ignoring BERYL_PROFILE: use a portable folder name with ASCII letters, digits or hyphens",
            json!(null),
        );
    }
}

pub fn name() -> Option<&'static str> {
    PROFILE.as_ref().ok()?.as_deref()
}

pub fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    Ok(data_path(app.path().app_data_dir()?, name()))
}

fn data_path(base: PathBuf, name: Option<&str>) -> PathBuf {
    match name {
        Some(name) => base.join("profiles").join(name),
        None => base,
    }
}

/// Opens the profile's journal and returns the directory for bug reports.
pub fn setup(app: &AppHandle) -> tauri::Result<PathBuf> {
    // Phones and named profiles keep reports in their own data directory.
    let profile_data = if cfg!(mobile) || name().is_some() {
        let data = data_dir(app)?;
        bugs::set_dir(data.join("bugs"));
        Some(data)
    } else {
        None
    };
    let bug_dir = bugs::dir();
    // Each profile has its own journal so simultaneous instances can rotate their logs independently.
    let log_dir = match profile_data.filter(|_| name().is_some()) {
        Some(data) => Ok(data.join("logs")),
        None => app.path().app_log_dir(),
    };
    match log_dir {
        Ok(dir) => {
            if let Err(e) = journal::open(&dir.join("beryl.jsonl")) {
                journal::warn("app", format!("no journal file: {e}"), json!(null));
            }
        }
        Err(e) => journal::warn("app", format!("no log directory: {e}"), json!(null)),
    }
    warn_if_invalid();
    Ok(bug_dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(value: Option<&str>) -> PathBuf {
        let name = value.and_then(|value| parse_name(OsStr::new(value)).ok());
        data_path(PathBuf::from("app"), name.as_deref())
    }

    #[test]
    fn named_profiles_have_their_own_data_directories() {
        let profiles = PathBuf::from("app").join("profiles");
        assert_eq!(path(Some("Seat-1")), profiles.join("seat-1"));
        assert_eq!(path(Some("seat-1")), profiles.join("seat-1"));
        assert_eq!(path(Some("seat-2")), profiles.join("seat-2"));
    }

    #[test]
    fn unset_or_invalid_profiles_use_the_default_directory() {
        let base = PathBuf::from("app");
        assert_eq!(path(None), base);
        for invalid in [
            "", "..", "../guest", "a/b", "a\\b", "foo bar", "foo_bar", "é", "NUL", "COM1", "lpt9",
        ] {
            assert_eq!(path(Some(invalid)), base, "{invalid:?}");
        }
        assert_eq!(path(Some(&"a".repeat(256))), base);
    }
}
