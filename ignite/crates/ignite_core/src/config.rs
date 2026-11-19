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
    Windows,
}

impl Os {
    pub fn as_str(self) -> &'static str {
        match self {
            Os::Macos => "macos",
            Os::Windows => "windows",
        }
    }

    /// The operating system this executable was built for, if Ignite supports it.
    pub fn current() -> Option<Os> {
        if cfg!(target_os = "macos") {
            Some(Os::Macos)
        } else if cfg!(windows) {
            Some(Os::Windows)
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
pub struct Config {
    pub repository: Repository,
    pub hub: HubConfig,
    pub gate: Gate,
    #[serde(default)]
    pub improvement: Improvement,
    #[serde(default)]
    pub rules: Rules,
    #[serde(default)]
    pub tip: Tip,
    #[serde(default)]
    pub pool: PoolConfig,
    /// Machines by hostname.
    pub machine: BTreeMap<String, Machine>,
    /// Edits whose rebuild cost `ignite rebuild` measures.
    #[serde(default)]
    pub rebuild: Vec<RebuildEdit>,
    /// How each daemon sends its machine's efficiency rows to the hub; none sends nothing.
    #[serde(default)]
    pub metrics: Option<Metrics>,
    /// Landings that change only Ignite run these commands instead of the gate; none sends
    /// every landing through the gate.
    #[serde(default)]
    pub express: Option<Express>,
    /// Formatters `ignite land` runs on the files a landing changes before its checks; none
    /// formats nothing.
    #[serde(default)]
    pub format: crate::formatting::Format,
    /// When a daemon restarts into a new release of Ignite.
    #[serde(default)]
    pub self_update: SelfUpdateConfig,
}

/// When a daemon restarts into a new release of Ignite (see "Processes" in the design document).
/// A restart waits until no newer release has appeared for `quiet_seconds`, so that releases
/// landing one after another make one restart into the newest, but never longer than
/// `cap_seconds` after the first release it waits with.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct SelfUpdateConfig {
    #[serde(default = "default_quiet_seconds")]
    pub quiet_seconds: f64,
    #[serde(default = "default_cap_seconds")]
    pub cap_seconds: f64,
}

impl Default for SelfUpdateConfig {
    fn default() -> Self {
        Self {
            quiet_seconds: default_quiet_seconds(),
            cap_seconds: default_cap_seconds(),
        }
    }
}

fn default_quiet_seconds() -> f64 {
    90.0
}

fn default_cap_seconds() -> f64 {
    600.0
}

/// The express lane (see "Express landings" in the design document). A landing whose every
/// changed file matches `paths` runs `commands` in one fixed worktree, `tree`, on cores the
/// scheduler never hands out (`express_cores` on the machine), instead of the gate. It exists
/// so that a fix to Ignite's own scheduling never waits in the queue it is fixing.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Express {
    /// Path patterns, relative to the checkout's root, as in a family's `inputs`.
    pub paths: Vec<String>,
    /// The fixed worktree the commands run in, relative to the checkout's root. It keeps its
    /// ignored files, such as a Cargo target directory, from one landing to the next.
    pub tree: PathBuf,
    /// The directory, relative to the worktree, that the commands run in.
    pub dir: String,
    /// The commands, each split on whitespace and run without a shell, in order. The landing
    /// passes when every one exits 0.
    pub commands: Vec<String>,
    /// The check families the commands stand in for. `ignite test` of checks only in these
    /// families, and `ignite check`, run the commands on the express cores in the agent's own
    /// tree when the tree changes only `paths`, instead of asking the scheduler for cores.
    /// None leaves both verbs to the scheduler.
    #[serde(default)]
    pub families: Vec<String>,
}

impl Express {
    /// Whether every one of `changed` matches `paths`; false for no paths at all.
    pub fn covers(&self, changed: &[String]) -> bool {
        !changed.is_empty()
            && changed.iter().all(|path| {
                self.paths
                    .iter()
                    .any(|p| crate::inputs::path_matches(p, path))
            })
    }
}

/// The efficiency dashboard's commands, which each daemon runs for its own machine when the
/// session transcripts it reads have grown (see `ignite_daemon::metrics`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Metrics {
    /// Directories whose files growing means there is something new to ingest. A leading `~`
    /// is the user's home directory.
    pub watch: Vec<String>,
    /// The ingest, as a program and its arguments, run from the main checkout.
    pub ingest: Vec<String>,
    /// The export of this machine's rows as JSON lines. `{since}` is replaced by the UTC date
    /// of the last export sent (or `all`) and `{out}` by the file to write.
    pub export: Vec<String>,
    /// The dashboard's findings as one JSON report on standard output. It runs after each
    /// export is sent, and its report is posted to the hub as one row from the source
    /// `findings`, which the hub's improvement rules read. Empty runs nothing.
    #[serde(default)]
    pub findings: Vec<String>,
}

/// The repository Ignite delivers to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// The branch daemons take Ignite's own executable from. Unset, it is `master`. A separate
    /// branch lets a fix to Ignite reach every machine without waiting for tip verification of
    /// everything else, which an Ignite bug can itself be blocking.
    #[serde(default)]
    pub release: Option<String>,
    /// The check family that must pass on a landed commit that changed Ignite's own source
    /// before that commit moves `release`. Unset, landings never move `release`.
    #[serde(default)]
    pub release_family: Option<String>,
}

impl Repository {
    /// The branch daemons build Ignite's executable from: `release`, or else `master`.
    pub fn release_branch(&self) -> &str {
        self.release.as_deref().unwrap_or(&self.master)
    }
}

/// Where the hub is.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct HubConfig {
    /// The hub's base address, such as `https://hub.example.ts.net:8443`.
    pub url: String,
    /// The project name in the hub's routes, `/<project>/api`.
    pub project: String,
    /// Older names the dashboard answered to. A page address under one redirects to the same
    /// address under `project`, keeping the query, so bookmarks keep working.
    #[serde(default)]
    pub aliases: Vec<String>,
    /// A file of HTML, relative to the repository root, that the dashboard shows at the left
    /// of every page's header in place of the name "Ignite". The hub reads it when it starts.
    #[serde(default)]
    pub wordmark: Option<String>,
}

/// The landing gate's values. Only a commit changes them.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Gate {
    /// Expected failures per billion instructions above which a check runs at the gate.
    pub threshold: f64,
    /// Seconds of running checks that the checks chosen by score may spend per landing, at the
    /// rate `land`-class check commands achieve (see [`crate::budget`]). Builds are not counted.
    #[serde(default = "default_gate_seconds")]
    pub seconds: f64,
}

fn default_gate_seconds() -> f64 {
    60.0
}

fn default_tip_seconds() -> f64 {
    300.0
}

/// The share of hand-offs that improvement tickets get.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
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

/// How tip verification divides a round's checks into claims (see "How long a round takes" in
/// `docs/ignite.md`).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Tip {
    /// The measured work one claim holds, in CPU seconds: a family's checks are cut into
    /// batches whose recorded costs add up to about this much.
    pub claim_seconds: f64,
    /// Seconds of running checks, at the rate `tip`-class check commands achieve, that a round
    /// spends on the best-scored checks before `master` moves. The rest run afterwards.
    #[serde(default = "default_tip_seconds")]
    pub seconds: f64,
}

impl Default for Tip {
    fn default() -> Self {
        Self {
            claim_seconds: 60.0,
            seconds: default_tip_seconds(),
        }
    }
}

/// How the improvement rules file tickets.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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

/// What the worker pool's apps accept.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PoolConfig {
    /// The model ID each app's session tool accepts for each `tier:` label, as
    /// `app -> tier -> id`. Tier labels on tickets and in a pool member's `models` list stay the
    /// vocabulary for matching a ticket to a worker; this table only says what to ask the app
    /// for once a worker has been chosen.
    #[serde(default)]
    pub models: BTreeMap<String, BTreeMap<String, String>>,
    /// The owner of a ticket that neither it nor any ancestor names, until a person picks
    /// another default on the dashboard (see [`crate::owner`]).
    #[serde(default)]
    pub default_owner: Option<crate::owner::Owner>,
}

impl PoolConfig {
    /// The model to ask `app` for when a ticket names `tier`. An app with no table here takes
    /// the tier name itself. `None` means the app has a table and it has no ID for the tier, so
    /// the app cannot be asked for that tier at all.
    pub fn model_id(&self, app: &str, tier: &str) -> Option<String> {
        match self.models.get(app) {
            None => Some(tier.to_owned()),
            Some(table) => table.get(tier).cloned(),
        }
    }
}

/// One machine's declared capacity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    /// Cores the scheduler never hands out, kept for the express lane's commands (see
    /// [`Express`]). Zero turns the express lane off on the machine.
    #[serde(default)]
    pub express_cores: u32,
    /// The app whose workers take a ticket owned by this machine without an app named, such as
    /// one the hub filed for a fault seen here (see [`crate::owner`]). `None` lets any app's
    /// workers on the machine take it.
    #[serde(default)]
    pub home_app: Option<String>,
}

/// Cores reserved for the `batch` and `tip` classes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
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
        Self::parse_with_unknown(file, text).map(|(config, _)| config)
    }

    /// Parses like [`Config::parse`] and also returns the keys it did not know, such as
    /// `repository.release`, so that the caller can warn about them.
    ///
    /// An unknown key is not an error. A key added to `ignite.toml` reaches every machine
    /// with the commit that adds it, often before that machine's daemon has been rebuilt from
    /// a commit that knows the key. Refusing the file would stop that daemon, and with it
    /// the self-update that would have taught it the key.
    pub fn parse_with_unknown(
        file: &str,
        text: &str,
    ) -> Result<(Config, Vec<String>), ConfigError> {
        let (config, unknown): (Config, _) =
            parse_lenient(text).map_err(|message| ConfigError {
                file: file.to_owned(),
                key: String::new(),
                message,
            })?;
        config.validate().map_err(|(key, message)| ConfigError {
            file: file.to_owned(),
            key,
            message,
        })?;
        Ok((config, unknown))
    }

    /// The `[express]` section of the `ignite.toml` in `text`, or `None` when the text does not
    /// parse or has no such section. A tree's own file is read this way, so that a tree judges
    /// itself by the express commands it has rather than those of the daemon's release.
    pub fn express_section(text: &str) -> Option<Express> {
        Self::parse("ignite.toml", text).ok()?.express
    }

    /// The declaration of the machine with this hostname. Hostnames are compared without regard
    /// to case, as DNS compares them: macOS reports `JARVIS` for a machine declared as `jarvis`.
    pub fn machine(&self, hostname: &str) -> Option<&Machine> {
        self.machine.get(hostname).or_else(|| {
            self.machine
                .iter()
                .find(|(name, _)| name.eq_ignore_ascii_case(hostname))
                .map(|(_, machine)| machine)
        })
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
        if !positive(self.gate.seconds) {
            return fail("gate.seconds", "must be above zero");
        }
        if !positive(self.tip.seconds) {
            return fail("tip.seconds", "must be above zero");
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
        if !positive(self.tip.claim_seconds) {
            return fail("tip.claim_seconds", "must be above zero");
        }
        if let Some(metrics) = &self.metrics {
            if metrics.ingest.is_empty() {
                return fail("metrics.ingest", "must name a program");
            }
            if !metrics.export.iter().any(|arg| arg.contains("{out}")) {
                return fail("metrics.export", "must contain {out}");
            }
        }
        for (app, table) in &self.pool.models {
            for (tier, id) in table {
                if id.trim().is_empty() {
                    return fail(&format!("pool.models.{app}.{tier}"), "must not be empty");
                }
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
            if edit.file.is_empty() || edit.append.is_empty() {
                return fail(
                    &format!("rebuild[{index}].file"),
                    "an edit needs a file and the text it appends",
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

/// Deserializes TOML, collecting the dotted path of every key the type does not declare
/// instead of refusing it. The message of an error is the parser's, trimmed.
pub(crate) fn parse_lenient<T: serde::de::DeserializeOwned>(
    text: &str,
) -> Result<(T, Vec<String>), String> {
    let deserializer =
        toml::Deserializer::parse(text).map_err(|e| e.to_string().trim_end().to_owned())?;
    let mut unknown = Vec::new();
    let value = serde_ignored::deserialize(deserializer, |path| unknown.push(path.to_string()))
        .map_err(|e: toml::de::Error| e.to_string().trim_end().to_owned())?;
    Ok((value, unknown))
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
seconds = 60           # of running checks per landing, builds not counted

[machine.jarvis]
ticket_trees = 24
landing_trees = 3
trees_disk_gb = 400

[machine.jarvis.resources]
browser-pages = 9
oracle = 1
bench-vm = 1

[machine.cygnus]
os = "windows"
ticket_trees = 6
landing_trees = 2
families = ["rust", "clippy", "format", "source-rules", "python"]
floor = { batch = 8, tip = 2 }

[[rebuild]]
name = "kernel-body"
edit = "append a no-op item to a module of the kernel crate"
file = "rust/crates/kernel/src/counters.rs"
append = "\nconst _: () = ();\n"
products = ["clippy", "rust-tests"]
[[rebuild]]
name = "one-card"
edit = "append a no-op item to one card file"
file = "rust/cards/one_card.rs"
append = "\nconst _: () = ();\n"
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
        let cygnus = config.machine("cygnus").unwrap();
        assert_eq!(cygnus.os, Some(Os::Windows));
        assert_eq!(cygnus.floor, Floor { batch: 8, tip: 2 });
        assert_eq!(config.rebuild[1].name, "one-card");
    }

    #[test]
    fn machines_are_found_whatever_the_hostname_case() {
        let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
        let declared = config.machine("jarvis").unwrap();
        assert!(std::ptr::eq(config.machine("JARVIS").unwrap(), declared));
        assert!(config.machine("Jarvis").is_some());
        assert!(config.machine("nobody").is_none());
    }

    #[test]
    fn the_express_lane_covers_a_landing_only_when_every_changed_file_matches() {
        let text = format!(
            "{DOCUMENT_EXAMPLES}\n[express]\npaths = [\"ignite/**\", \"ignite.toml\"]\n\
             tree = \".worktrees/express\"\ndir = \"ignite\"\ncommands = [\"cargo fmt --check\"]\n\
             families = [\"ignite\"]\n"
        );
        let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
        assert!(unknown.is_empty(), "{unknown:?}");
        let express = config.express.unwrap();
        let paths = |names: &[&str]| names.iter().map(|n| (*n).to_owned()).collect::<Vec<_>>();
        assert!(express.covers(&paths(&["ignite/crates/a.rs", "ignite.toml"])));
        assert!(!express.covers(&paths(&["ignite/crates/a.rs", "rust/Cargo.toml"])));
        assert!(!express.covers(&[]));
        assert_eq!(express.families, ["ignite"]);
        assert_eq!(config.machine["jarvis"].express_cores, 0);
    }

    #[test]
    fn unknown_keys_are_reported_but_not_refused() {
        let text = DOCUMENT_EXAMPLES
            .replace("seconds = 60", "seconds = 60\nsecondz = 1")
            .replace("[repository]", "[repository]\nfuture_key = \"x\"");
        let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
        assert_eq!(
            config,
            Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap()
        );
        assert_eq!(unknown.len(), 2, "{unknown:?}");
        assert!(
            unknown.iter().any(|k| k.ends_with("secondz")),
            "{unknown:?}"
        );
        assert!(
            unknown.contains(&"repository.future_key".to_owned()),
            "{unknown:?}"
        );
        let (_, none) = Config::parse_with_unknown("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
        assert!(none.is_empty(), "{none:?}");
    }

    #[test]
    fn a_tip_claim_defaults_to_a_minute_and_must_be_above_zero() {
        let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
        assert!((config.tip.claim_seconds - 60.0).abs() < f64::EPSILON);
        let text = format!("{DOCUMENT_EXAMPLES}\n[tip]\nclaim_seconds = 30\n");
        let config = Config::parse("ignite.toml", &text).unwrap();
        assert!((config.tip.claim_seconds - 30.0).abs() < f64::EPSILON);
        let text = format!("{DOCUMENT_EXAMPLES}\n[tip]\nclaim_seconds = 0\n");
        let error = Config::parse("ignite.toml", &text).unwrap_err();
        assert_eq!(error.key, "tip.claim_seconds");
    }

    /// A file written before the two budgets has `cap` and no `seconds`: it still parses, with
    /// the default budgets, and `cap` is reported as a key this build does not know.
    #[test]
    fn a_file_from_before_the_budgets_parses_with_the_default_seconds() {
        let text = DOCUMENT_EXAMPLES.replace("seconds = 60", "cap = 8890");
        let (config, unknown) = Config::parse_with_unknown("ignite.toml", &text).unwrap();
        assert!((config.gate.seconds - 60.0).abs() < f64::EPSILON);
        assert!((config.tip.seconds - 300.0).abs() < f64::EPSILON);
        assert_eq!(unknown, ["gate.cap"]);
        let text = format!("{DOCUMENT_EXAMPLES}\n[tip]\nclaim_seconds = 60\nseconds = 0\n");
        assert_eq!(
            Config::parse("ignite.toml", &text).unwrap_err().key,
            "tip.seconds"
        );
    }

    #[test]
    fn syntax_errors_still_name_the_file() {
        let text = DOCUMENT_EXAMPLES.replace("seconds = 60", "seconds = = 60");
        let error = Config::parse("ignite.toml", &text).unwrap_err();
        assert_eq!(error.file, "ignite.toml");
    }

    #[test]
    fn semantic_errors_name_the_key() {
        let text = DOCUMENT_EXAMPLES.replace("landing_trees = 2", "landing_trees = 0");
        let error = Config::parse("ignite.toml", &text).unwrap_err();
        assert_eq!(error.key, "machine.cygnus.landing_trees");
        assert_eq!(
            error.to_string(),
            "ignite.toml: machine.cygnus.landing_trees: must be above zero"
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
        assert!(config.machine.len() >= 3);
        assert_eq!(config.rules, Rules::default());
    }

    #[test]
    fn a_model_id_comes_from_the_apps_table_or_is_the_tier_when_there_is_none() {
        let text = format!("{DOCUMENT_EXAMPLES}\n[pool.models.claude]\nopus = \"the-big-one\"\n");
        let config = Config::parse("ignite.toml", &text).unwrap();
        assert_eq!(
            config.pool.model_id("claude", "opus"),
            Some("the-big-one".to_owned())
        );
        assert_eq!(config.pool.model_id("claude", "sonnet"), None);
        assert_eq!(
            config.pool.model_id("codex", "sonnet"),
            Some("sonnet".to_owned())
        );
        let config = Config::parse("ignite.toml", DOCUMENT_EXAMPLES).unwrap();
        assert_eq!(config.pool, PoolConfig::default());
        let text = format!("{DOCUMENT_EXAMPLES}\n[pool.models.claude]\nopus = \" \"\n");
        assert_eq!(
            Config::parse("ignite.toml", &text).unwrap_err().key,
            "pool.models.claude.opus"
        );
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
