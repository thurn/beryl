//! `ignite.toml`: what Ignite knows about the repository it serves and the machines that serve it.
//!
//! Ignite's code names no path, branch or tool of any one repository. The remote, the names of
//! the integration and release branches, where build trees live, where check manifests live,
//! each machine's capacity, the landing gate's values and the rebuild edits all come from here.
//! Every table refuses keys it does not know, so a misspelt key is an error, not a default.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::manifest::RebuildEdit;

/// An operating system a machine runs or a check must pass on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Os {
    Macos,
}

impl Os {
    pub fn as_str(self) -> &'static str {
        match self {
            Os::Macos => "macos",
        }
    }

    /// The operating system this executable was built for, if Ignite supports it.
    pub fn current() -> Option<Os> {
        if cfg!(target_os = "macos") {
            Some(Os::Macos)
        } else {
            None
        }
    }
}

impl fmt::Display for Os {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The whole of `ignite.toml`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub repository: Repository,
    pub hub: HubConfig,
    pub gate: Gate,
    #[serde(default)]
    pub improvement: Improvement,
    #[serde(default)]
    pub rules: Rules,
    /// Machines by hostname.
    pub machine: BTreeMap<String, Machine>,
    /// Edits whose rebuild cost `ignite rebuild` measures.
    #[serde(default)]
    pub rebuild: Vec<RebuildEdit>,
    /// How each daemon sends its machine's efficiency rows to the hub; none sends nothing.
    #[serde(default)]
    pub metrics: Option<Metrics>,
}

/// The efficiency dashboard's commands, which each daemon runs for its own machine when the
/// session transcripts it reads have grown (see `ignite_daemon::metrics`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Metrics {
    /// Directories whose files growing means there is something new to ingest. A leading `~`
    /// is the user's home directory.
    pub watch: Vec<String>,
    /// The ingest, as a program and its arguments, run from the main checkout.
    pub ingest: Vec<String>,
    /// The export of this machine's rows as JSON lines. `{since}` is replaced by the UTC date
    /// of the last export sent (or `all`) and `{out}` by the file to write.
    pub export: Vec<String>,
}

/// The repository Ignite delivers to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Repository {
    /// The git remote that holds both branches, such as `origin`.
    pub remote: String,
    /// The branch that landings go to.
    pub staging: String,
    /// The branch that tip verification moves forward.
    pub master: String,
    /// Where build trees live, relative to the checkout's root.
    pub trees: PathBuf,
    /// The directory of check manifests (`*.toml`), relative to the checkout's root.
    pub checks: PathBuf,
}

/// Where the hub is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HubConfig {
    /// The hub's base address, such as `https://hub.example.ts.net:8443`.
    pub url: String,
    /// The project name in the hub's routes, `/<project>/api`.
    pub project: String,
}

/// The landing gate's values. Only a commit changes them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Gate {
    /// Expected failures per billion instructions above which a check runs at the gate.
    pub threshold: f64,
    /// Billions of instructions per landing that checks chosen by score may spend.
    pub cap: f64,
}

/// The share of hand-offs that improvement tickets get.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Improvement {
    /// The share when the backlog is small.
    pub min_share: f64,
    /// The share when the backlog is large.
    pub max_share: f64,
    /// The backlog's saving, as a fraction of last week's total, at or below which the share is
    /// `min_share`.
    pub backlog_low: f64,
    /// The backlog fraction at or above which the share is `max_share`.
    pub backlog_high: f64,
    /// How many recent hand-offs to each machine the share is counted over.
    pub window: u32,
}

impl Default for Improvement {
    fn default() -> Self {
        Self {
            min_share: 0.15,
            max_share: 0.30,
            backlog_low: 0.05,
            backlog_high: 0.20,
            window: 20,
        }
    }
}

/// How the improvement rules file tickets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rules {
    /// Rules whose findings show on the dashboard and file no ticket, such as one that
    /// compares model tiers, because that choice stays with people.
    pub report_only: Vec<String>,
    /// Checks that `never-caught` never names, each a family (`clippy`) or a whole check name
    /// (`family:name`). They are cheap, and what they guard is too basic to give up for want
    /// of a recent failure.
    pub protected: Vec<String>,
}

impl Default for Rules {
    fn default() -> Self {
        let names = |list: &[&str]| list.iter().map(|&name| name.to_owned()).collect();
        Self {
            report_only: names(&["tier-gap"]),
            protected: names(&["app", "ui-smoke", "clippy", "pyre", "ui-types", "source"]),
        }
    }
}

impl Rules {
    /// Whether `check`, named `family:name`, is protected by its own name or its family's.
    pub fn is_protected(&self, check: &str) -> bool {
        let family = check.split_once(':').map_or(check, |(family, _)| family);
        self.protected
            .iter()
            .any(|entry| entry == check || entry == family)
    }
}

/// One machine's declared capacity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    /// The operating system, when the declaration states it.
    pub os: Option<Os>,
    pub ticket_trees: u32,
    pub landing_trees: u32,
    /// The disk the trees may use, in gigabytes.
    pub trees_disk_gb: Option<u32>,
    /// The check families the machine can run; `None` means every family.
    pub families: Option<Vec<String>>,
    /// Cores per machine that higher classes cannot take.
    #[serde(default)]
    pub floor: Floor,
    /// Scarce resources by name, with each one's capacity.
    #[serde(default)]
    pub resources: BTreeMap<String, u32>,
}

/// Cores reserved for the `batch` and `tip` classes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Floor {
    #[serde(default)]
    pub batch: u32,
    #[serde(default)]
    pub tip: u32,
}

/// A configuration file that cannot be used, naming the file and, where known, the key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigError {
    pub file: String,
    pub key: String,
    pub message: String,
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            write!(f, "{}: {}", self.file, self.message)
        } else {
            write!(f, "{}: {}: {}", self.file, self.key, self.message)
        }
    }
}

impl std::error::Error for ConfigError {}

impl Config {
    /// Parses and validates the text of `ignite.toml`; `file` names it in errors.
    pub fn parse(file: &str, text: &str) -> Result<Config, ConfigError> {
        let config: Config = toml::from_str(text).map_err(|error| ConfigError {
            file: file.to_owned(),
            key: String::new(),
            message: error.to_string().trim_end().to_owned(),
        })?;
        config.validate().map_err(|(key, message)| ConfigError {
            file: file.to_owned(),
            key,
            message,
        })?;
        Ok(config)
    }

    /// The declaration of the machine with this hostname.
    pub fn machine(&self, hostname: &str) -> Option<&Machine> {
        self.machine.get(hostname)
    }

    fn validate(&self) -> Result<(), (String, String)> {
        let fail = |key: &str, message: &str| Err((key.to_owned(), message.to_owned()));
        for (key, value) in [
            ("repository.remote", &self.repository.remote),
            ("repository.staging", &self.repository.staging),
            ("repository.master", &self.repository.master),
            ("hub.url", &self.hub.url),
            ("hub.project", &self.hub.project),
        ] {
            if value.trim().is_empty() {
                return fail(key, "must not be empty");
            }
        }
        if self.repository.staging == self.repository.master {
            return fail("repository.master", "must differ from repository.staging");
        }
        if !positive(self.gate.threshold) {
            return fail("gate.threshold", "must be above zero");
        }
        if !positive(self.gate.cap) {
            return fail("gate.cap", "must be above zero");
        }
        let share = self.improvement;
        if !(0.0..=1.0).contains(&share.min_share) || !(0.0..=1.0).contains(&share.max_share) {
            return fail("improvement", "shares must be between 0 and 1");
        }
        if share.min_share > share.max_share {
            return fail(
                "improvement.min_share",
                "must not exceed improvement.max_share",
            );
        }
        if !positive(share.backlog_high - share.backlog_low) {
            return fail(
                "improvement.backlog_low",
                "must be below improvement.backlog_high",
            );
        }
        if share.window == 0 {
            return fail("improvement.window", "must be above zero");
        }
        if let Some(metrics) = &self.metrics {
            if metrics.ingest.is_empty() {
                return fail("metrics.ingest", "must name a program");
            }
            if !metrics.export.iter().any(|arg| arg.contains("{out}")) {
                return fail("metrics.export", "must contain {out}");
            }
        }
        if self.machine.is_empty() {
            return fail("machine", "declares no machine");
        }
        for (name, machine) in &self.machine {
            if machine.ticket_trees == 0 {
                return fail(
                    &format!("machine.{name}.ticket_trees"),
                    "must be above zero",
                );
            }
            if machine.landing_trees == 0 {
                return fail(
                    &format!("machine.{name}.landing_trees"),
                    "must be above zero",
                );
            }
        }
        let mut seen = std::collections::BTreeSet::new();
        for (index, edit) in self.rebuild.iter().enumerate() {
            if edit.name.is_empty() || !seen.insert(edit.name.as_str()) {
                return fail(
                    &format!("rebuild[{index}].name"),
                    "must be present and unique",
                );
            }
            if edit.products.is_empty() {
                return fail(
                    &format!("rebuild[{index}].products"),
                    "names no build product",
                );
            }
        }
        Ok(())
    }
}

/// Whether `value` is above zero; false for NaN.
fn positive(value: f64) -> bool {
    value > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The design document's examples, joined into one file with the sections they leave out.
    const DOCUMENT_EXAMPLES: &str = r#"
[repository]
remote = "origin"
staging = "staging"
master = "master"
trees = ".worktrees"
checks = "ignite/checks"

[hub]
url = "https://hub.example.ts.net:8443"
project = "demo"

[gate]                 # example values
threshold = 0.004      # expected failures per billion instructions
cap = 400              # billions of instructions per landing

[machine.jarvis]
ticket_trees = 24
landing_trees = 3
trees_disk_gb = 400

[machine.jarvis.resources]
browser-pages = 9
oracle = 1
bench-vm = 1

[machine.multivac]
os = "macos"
ticket_trees = 6
landing_trees = 2
families = ["rust", "clippy", "format", "source-rules", "python"]
floor = { batch = 8, tip = 2 }

[[rebuild]]
name = "kernel-body"
edit = "append a no-op statement to one function in the kernel crate"
products = ["clippy", "rust-tests"]
[[rebuild]]
name = "one-card"
edit = "append a no-op statement to one card file"
products = ["clippy", "rust-tests"]
"#;

    #[test]
    fn parses_the_document_examples() {
        let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
        assert!((config.gate.threshold - 0.004).abs() < 1e-12);
        assert_eq!(config.improvement, Improvement::default());
        assert_eq!(config.rules, Rules::default());
        let jarvis = config.machine("jarvis").unwrap();
        assert_eq!((jarvis.ticket_trees, jarvis.landing_trees), (24, 3));
        assert_eq!(jarvis.resources["browser-pages"], 9);
        assert_eq!(jarvis.families, None);
        let multivac = config.machine("multivac").unwrap();
        assert_eq!(multivac.os, Some(Os::Macos));
        assert_eq!(multivac.floor, Floor { batch: 8, tip: 2 });
        assert_eq!(config.rebuild[1].name, "one-card");
    }

    #[test]
    fn unknown_keys_are_errors_that_name_the_file() {
        let text = DOCUMENT_EXAMPLES.replace("cap = 400", "cap = 400\ncapp = 1");
        let error = Config::parse("ignite.toml", &text).unwrap_err();
        assert_eq!(error.file, "ignite.toml");
        assert!(error.message.contains("capp"), "{error}");
    }

    #[test]
    fn semantic_errors_name_the_key() {
        let text = DOCUMENT_EXAMPLES.replace("landing_trees = 2", "landing_trees = 0");
        let error = Config::parse("ignite.toml", &text).unwrap_err();
        assert_eq!(error.key, "machine.multivac.landing_trees");
        assert_eq!(
            error.to_string(),
            "ignite.toml: machine.multivac.landing_trees: must be above zero"
        );
        let text = DOCUMENT_EXAMPLES.replace("name = \"one-card\"", "name = \"kernel-body\"");
        assert_eq!(
            Config::parse("ignite.toml", &text).unwrap_err().key,
            "rebuild[1].name"
        );
    }

    #[test]
    fn the_repository_configuration_parses() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../ignite.toml");
        let text = std::fs::read_to_string(path).unwrap();
        let config = Config::parse("ignite.toml", &text).unwrap();
        assert!(config.machine.len() >= 2);
        assert_eq!(config.rules, Rules::default());
    }

    #[test]
    fn a_check_is_protected_by_its_own_name_or_its_family() {
        let rules = Rules {
            report_only: Vec::new(),
            protected: vec!["lint".to_owned(), "tests:boots".to_owned()],
        };
        assert!(rules.is_protected("lint:workspace"));
        assert!(rules.is_protected("tests:boots"));
        assert!(!rules.is_protected("tests:other"));
        assert!(!rules.is_protected("linter:workspace"));
    }
}
