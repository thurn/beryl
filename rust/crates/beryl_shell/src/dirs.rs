//! The folders an app instance keeps its data, logs and bug reports in.

use std::path::{Path, PathBuf};

use beryl_session::{bugs, journal};
use serde_json::json;

use crate::Profile;

/// Where an instance keeps what it saves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Dirs {
    /// Replays, the games ledger and the deck library: the base data folder, or
    /// `<base>/profiles/<name>` for a named profile.
    pub data: PathBuf,
    /// The journal: `<data>/logs` for a named profile, so that instances running side by
    /// side rotate their logs independently, and otherwise the platform's log folder.
    pub logs: PathBuf,
    /// Bug reports: `$BERYL_BUGS` when it is set; otherwise `<data>/bugs` on a phone or for a
    /// named profile, and the desktop's shared bugs folder ([`bugs::dir`]) for the default
    /// profile on a desktop.
    pub bugs: PathBuf,
}

impl Dirs {
    /// The folders of `profile` under the base data folder `base`, with `log_base` the
    /// platform's log folder.
    pub fn new(base: &Path, log_base: PathBuf, profile: &Profile) -> Dirs {
        let phone = cfg!(any(target_os = "ios", target_os = "android"));
        let set = std::env::var_os("BERYL_BUGS").map(PathBuf::from);
        Self::with(base, log_base, profile, phone, set, bugs::dir)
    }

    /// The desktop folders of `profile`: [`desktop_base`] and [`desktop_logs`], the folders
    /// the Tauri app uses. None when the environment names no home folder.
    pub fn desktop(profile: &Profile) -> Option<Dirs> {
        Some(Self::new(&desktop_base()?, desktop_logs()?, profile))
    }

    fn with(
        base: &Path,
        log_base: PathBuf,
        profile: &Profile,
        phone: bool,
        set_bugs: Option<PathBuf>,
        shared_bugs: impl FnOnce() -> PathBuf,
    ) -> Dirs {
        let data = match profile.name() {
            Some(name) => base.join("profiles").join(name),
            None => base.to_owned(),
        };
        let logs = match profile.name() {
            Some(_) => data.join("logs"),
            None => log_base,
        };
        let bugs = match set_bugs {
            Some(dir) => dir,
            None if phone || profile.name().is_some() => data.join("bugs"),
            None => shared_bugs(),
        };
        Dirs { data, logs, bugs }
    }

    /// Makes these folders the process's: bug reports go to [`Dirs::bugs`], and the journal
    /// is opened in [`Dirs::logs`]. Journals a warning for an invalid `BERYL_PROFILE`.
    pub fn open(&self) {
        bugs::set_dir(self.bugs.clone());
        if let Err(e) = journal::open(&self.logs.join("beryl.jsonl")) {
            journal::warn("app", format!("no journal file: {e}"), json!(null));
        }
        Profile::warn_if_invalid();
    }
}

/// The Tauri app's data folder on a desktop, named for its bundle identifier:
/// `~/Library/Application Support/ca.thurn.beryl` on macOS, `%APPDATA%\ca.thurn.beryl` on
/// Windows.
pub fn desktop_base() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join(IDENTIFIER))
    } else {
        home().map(|home| {
            home.join("Library")
                .join("Application Support")
                .join(IDENTIFIER)
        })
    }
}

/// The Tauri app's log folder on a desktop: `~/Library/Logs/ca.thurn.beryl` on macOS,
/// `%LOCALAPPDATA%\ca.thurn.beryl\logs` on Windows.
pub fn desktop_logs() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(|dir| PathBuf::from(dir).join(IDENTIFIER).join("logs"))
    } else {
        home().map(|home| home.join("Library").join("Logs").join(IDENTIFIER))
    }
}

/// The Tauri app's bundle identifier (`identifier` in `beryl_app`'s `tauri.conf.json`).
const IDENTIFIER: &str = "ca.thurn.beryl";

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    fn data(value: Option<&str>) -> PathBuf {
        let profile = value
            .and_then(|value| Profile::parse(OsStr::new(value)).ok())
            .unwrap_or_default();
        Dirs::with(
            Path::new("app"),
            PathBuf::from("logs"),
            &profile,
            false,
            None,
            || PathBuf::from("shared"),
        )
        .data
    }

    #[test]
    fn named_profiles_have_their_own_data_directories() {
        let profiles = PathBuf::from("app").join("profiles");
        assert_eq!(data(Some("Seat-1")), profiles.join("seat-1"));
        assert_eq!(data(Some("seat-1")), profiles.join("seat-1"));
        assert_eq!(data(Some("seat-2")), profiles.join("seat-2"));
    }

    #[test]
    fn unset_or_invalid_profiles_use_the_default_directory() {
        let base = PathBuf::from("app");
        assert_eq!(data(None), base);
        for invalid in [
            "", "..", "../guest", "a/b", "a\\b", "foo bar", "foo_bar", "é", "NUL", "COM1", "lpt9",
        ] {
            assert_eq!(data(Some(invalid)), base, "{invalid:?}");
        }
        assert_eq!(data(Some(&"a".repeat(256))), base);
    }

    #[test]
    fn named_profiles_and_phones_keep_their_own_logs_and_reports() {
        let shared = || PathBuf::from("shared");
        let named = Profile::parse(OsStr::new("seat-1")).unwrap();
        let dirs = Dirs::with(
            Path::new("app"),
            PathBuf::from("logs"),
            &named,
            false,
            None,
            shared,
        );
        assert_eq!(dirs.logs, dirs.data.join("logs"));
        assert_eq!(dirs.bugs, dirs.data.join("bugs"));
        let default = Profile::default();
        let desktop = Dirs::with(
            Path::new("app"),
            PathBuf::from("logs"),
            &default,
            false,
            None,
            shared,
        );
        assert_eq!(desktop.logs, PathBuf::from("logs"));
        assert_eq!(desktop.bugs, PathBuf::from("shared"));
        let phone = Dirs::with(
            Path::new("app"),
            PathBuf::from("logs"),
            &default,
            true,
            None,
            shared,
        );
        assert_eq!(phone.bugs, PathBuf::from("app").join("bugs"));
        let set = Some(PathBuf::from("set"));
        let chosen = Dirs::with(
            Path::new("app"),
            PathBuf::from("logs"),
            &named,
            true,
            set,
            shared,
        );
        assert_eq!(chosen.bugs, PathBuf::from("set"), "$BERYL_BUGS wins");
    }
}
