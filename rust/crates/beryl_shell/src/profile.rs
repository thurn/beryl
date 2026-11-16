//! The profile an app instance runs as, selected once from `BERYL_PROFILE`. A named profile
//! keeps its data, logs and bug reports apart from the default profile's, so that two
//! instances can run side by side (two seats of a local game, say).

use std::ffi::OsStr;
use std::sync::LazyLock;

use beryl_session::journal;
use serde_json::json;

const ENV: &str = "BERYL_PROFILE";

static CURRENT: LazyLock<Result<Profile, InvalidName>> = LazyLock::new(|| {
    std::env::var_os(ENV)
        .as_deref()
        .map_or(Ok(Profile::default()), Profile::parse)
});

/// A profile: the default one, or one named by a portable folder name.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile(Option<String>);

/// A `BERYL_PROFILE` value that is not a portable folder name.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidName;

impl Profile {
    /// The profile `BERYL_PROFILE` names, or the default profile when it is unset or invalid
    /// ([`Profile::warn_if_invalid`] journals the second).
    pub fn current() -> &'static Profile {
        static DEFAULT: Profile = Profile(None);
        CURRENT.as_ref().unwrap_or(&DEFAULT)
    }

    /// The profile named `value`: ASCII letters, digits and hyphens, at most 255 bytes, and not
    /// a name Windows reserves for a device. The name is lowercased, because the volumes macOS
    /// uses by default fold case, and the folder and the web view's storage use the same name.
    pub fn parse(value: &OsStr) -> Result<Profile, InvalidName> {
        let name = value.to_str().ok_or(InvalidName)?;
        if name.is_empty()
            || name.len() > 255
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        {
            return Err(InvalidName);
        }
        let name = name.to_ascii_lowercase();
        let device = matches!(name.as_str(), "con" | "prn" | "aux" | "nul")
            || matches!(
                name.as_bytes(),
                [b'c', b'o', b'm', b'1'..=b'9'] | [b'l', b'p', b't', b'1'..=b'9']
            );
        if device {
            Err(InvalidName)
        } else {
            Ok(Profile(Some(name)))
        }
    }

    /// The profile's name, or None for the default profile.
    pub fn name(&self) -> Option<&str> {
        self.0.as_deref()
    }

    /// Journals a warning when `BERYL_PROFILE` was set to a name [`Profile::parse`] rejects.
    pub fn warn_if_invalid() {
        if CURRENT.is_err() {
            journal::warn(
                "app",
                "Ignoring BERYL_PROFILE: use a portable folder name with ASCII letters, digits or hyphens",
                json!(null),
            );
        }
    }
}
