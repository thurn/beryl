//! The `[land]` and `[revert]` sections of `ignite.toml`: how `ignite land` decides that a change
//! may land, and when a tip round reverts a landing. Every key is optional,
//! and the defaults are the strict landing that runs the gate on the rebased commit, with no
//! automatic revert.

use serde::{Deserialize, Serialize};

/// How `ignite land` lands a change that touches none of Ignite's own source.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LandMode {
    /// Rebase the ticket tree onto the tip and run the gate there, again after every lost race.
    #[default]
    Strict,
    /// Land by a verdict: a gate that passed for the same files on the same base. The commits
    /// are rebased onto the tip in git's object database and pushed without checking again,
    /// unless the rebase conflicts or the base is older than `max_base_age_hours`.
    Verdict,
}

/// `[land]`.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Land {
    #[serde(default)]
    pub mode: LandMode,
    /// In verdict mode, a base committed longer ago than this is rebased onto the tip and
    /// checked again before the change lands.
    #[serde(default = "default_max_base_age_hours")]
    pub max_base_age_hours: f64,
}

impl Default for Land {
    fn default() -> Self {
        Self {
            mode: LandMode::default(),
            max_base_age_hours: default_max_base_age_hours(),
        }
    }
}

fn default_max_base_age_hours() -> f64 {
    12.0
}

/// `[revert]`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Revert {
    /// A tip round reverts the landing that made a build check fail (a check of a family
    /// declared `builds = true`), instead of only reporting a regression.
    #[serde(default)]
    pub broken_build: bool,
    /// How long the oldest unverified landing may wait before a required non-build check can
    /// trigger a revert. Unset leaves those failures to regression tickets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stall_minutes: Option<u64>,
}

#[cfg(test)]
mod tests {
    use crate::config::Config;

    const MINIMAL: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = ".worktrees"
checks = "checks"

[hub]
url = "https://hub.example.ts.net:8443"
project = "demo"

[gate]
threshold = 0.004
"#;

    #[test]
    fn a_file_without_the_new_sections_keeps_todays_behaviour() {
        let (config, unknown) = Config::parse_with_unknown("ignite.toml", MINIMAL).unwrap();
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(config.land.mode, super::LandMode::Strict);
        assert!((config.land.max_base_age_hours - 12.0).abs() < f64::EPSILON);
        assert!(!config.revert.broken_build);
        assert_eq!(config.repository.green, None);
    }

    #[test]
    fn the_new_sections_parse() {
        let text = MINIMAL.replace(
            "checks = \"checks\"",
            "checks = \"checks\"\ngreen = \"green\"",
        ) + "\n[land]\nmode = \"verdict\"\nmax_base_age_hours = 6\n\n\
             [revert]\nbroken_build = true\n";
        let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(config.repository.green.as_deref(), Some("green"));
        assert_eq!(config.land.mode, super::LandMode::Verdict);
        assert!((config.land.max_base_age_hours - 6.0).abs() < f64::EPSILON);
        assert!(config.revert.broken_build);
    }

    #[test]
    fn green_must_be_a_branch_of_its_own_and_the_base_age_positive() {
        for branch in ["staging", "master"] {
            let text = MINIMAL.replace(
                "checks = \"checks\"",
                &format!("checks = \"checks\"\ngreen = \"{branch}\""),
            );
            let error = Config::parse("ignite.toml", &text).unwrap_err();
            assert_eq!(error.key, "repository.green");
        }
        // The same ref named two ways is still the same ref.
        let text = MINIMAL.replace(
            "checks = \"checks\"",
            "checks = \"checks\"\ngreen = \"refs/heads/staging\"",
        );
        let error = Config::parse("ignite.toml", &text).unwrap_err();
        assert_eq!(error.key, "repository.green");
        let text = format!("{MINIMAL}\n[land]\nmax_base_age_hours = 0\n");
        let error = Config::parse("ignite.toml", &text).unwrap_err();
        assert_eq!(error.key, "land.max_base_age_hours");
    }

    #[test]
    fn refs_may_be_full_names_and_saved_work_has_a_prefix() {
        let (config, _) = Config::parse_with_unknown("ignite.toml", MINIMAL).unwrap();
        assert_eq!(config.repository.saved_prefix(), "ignite/");
        let text = MINIMAL
            .replace("staging = \"staging\"", "staging = \"refs/ignite/staging\"")
            .replace(
                "checks = \"checks\"",
                "checks = \"checks\"\ngreen = \"refs/ignite/green\"\n\
                 release = \"refs/ignite/release\"\nsaved = \"refs/ignite/tickets/\"",
            );
        let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
        assert!(unknown.is_empty(), "{unknown:?}");
        assert_eq!(config.repository.staging, "refs/ignite/staging");
        assert_eq!(config.repository.release_branch(), "refs/ignite/release");
        assert_eq!(config.repository.saved_prefix(), "refs/ignite/tickets/");
        let error = Config::parse(
            "ignite.toml",
            &MINIMAL.replace("checks = \"checks\"", "checks = \"checks\"\nsaved = \" \""),
        )
        .unwrap_err();
        assert_eq!(error.key, "repository.saved");
    }
}
