//! `ignite.toml`: what Ignite knows about the repository it serves and the machines that serve it.
//!
//! Ignite's code names no path, branch or tool of any one repository. The remote, the names of
//! the integration and release branches, where build trees live, where check manifests live,
//! fixed resources, the landing gate's values and the rebuild edits all come from here.
//! The parser reports unrecognized keys and keeps loading the file.

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::manifest::RebuildEdit;

#[path = "config_cli.rs"]
mod cli;
#[path = "config_land.rs"]
mod land;
pub use cli::CliProgram;
pub use land::{Land, LandMode, Revert};

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

/// The user's home directory: `HOME`, or on Windows, where only Git Bash sets `HOME`,
/// `USERPROFILE`. A login service started by the Task Scheduler and a client run from
/// PowerShell have no `HOME`, so without the fallback each would find Ignite's files under a
/// different relative directory and the client could not reach the daemon's pipe.
pub fn home_dir(var: impl Fn(&str) -> Option<std::ffi::OsString>) -> Option<PathBuf> {
    var("HOME")
        .filter(|home| !home.is_empty())
        .or_else(|| var("USERPROFILE").filter(|home| !home.is_empty()))
        .map(PathBuf::from)
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
    #[serde(default)]
    pub machine: BTreeMap<String, Machine>,
    /// Shared resource capacities derived from each machine's logical cores.
    #[serde(default)]
    pub resources: BTreeMap<String, Resource>,
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
    /// How `ignite land` lands a change; unset is the strict landing.
    #[serde(default)]
    pub land: Land,
    /// What a tip round reverts; unset reverts nothing.
    #[serde(default)]
    pub revert: Revert,
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
    15.0
}

fn default_cap_seconds() -> f64 {
    600.0
}

/// A landing whose every changed file matches `paths` runs `commands` in its ticket tree on
/// reserved express cores. This lets fixes to the scheduler proceed without waiting for the gate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Express {
    /// Path patterns, relative to the checkout's root, as in a family's `inputs`.
    pub paths: Vec<String>,
    /// The former shared express worktree path, retained in configuration for older daemons.
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
    /// Path patterns for files under `paths` that no command builds or runs, or that a command
    /// only checks the formatting of, which the landing's `[format]` rules rewrite after its
    /// rebase. A commit landed after an express pass that edits one of the change's own files
    /// leaves the pass standing when the file matches one of these (see
    /// `ignite_daemon::land::express`). Nearly every Ignite change adds a paragraph to the design
    /// document and a line to a data file that no command builds, so without this list any
    /// landing that rebased over another Ignite change ran the commands again (65 to 71 s on
    /// 2026-11-19).
    #[serde(default)]
    pub inert: Vec<String>,
}

impl Express {
    /// Whether `path` matches `inert`.
    pub fn is_inert(&self, path: &str) -> bool {
        self.inert
            .iter()
            .any(|p| crate::inputs::path_matches(p, path))
    }

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
    /// The ticket pages' spend windows as JSON lines, one session window per line, with
    /// `{since}` and `{out}` as for `export`. It runs after each export is sent, and its windows
    /// are posted to the hub from the source `spend`, which the hub keeps in `ticket_spend`
    /// for the ticket pages' efficiency section. Empty runs nothing.
    #[serde(default)]
    pub spend: Vec<String>,
}

/// The repository Ignite delivers to.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    /// The git remote that holds both branches, such as `origin`.
    pub remote: String,
    /// The branch that landings go to. This and the other refs of this section are either a
    /// branch name or a full ref name such as `refs/ignite/staging` (see
    /// [`crate::refname`]).
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
    /// The branch a tip round moves once the round's build checks pass at the tip, which new
    /// trees start from. Unset, there is no such branch.
    #[serde(default)]
    pub green: Option<String>,
    /// What `ignite release` saves a ticket's work as on the remote: this prefix followed by
    /// the ticket's ID, such as `refs/ignite/tickets/` for `refs/ignite/tickets/TNFZ6`. Unset,
    /// it is [`DEFAULT_SAVED_PREFIX`], the branch `ignite/<ticket>`.
    #[serde(default)]
    pub saved: Option<String>,
    /// Paths that count as test files when classifying a landed check fix.
    #[serde(default)]
    pub test_paths: Vec<String>,
}

/// The prefix of a released ticket's saved work when `repository.saved` is unset.
pub const DEFAULT_SAVED_PREFIX: &str = "ignite/";

impl Repository {
    /// The branch daemons build Ignite's executable from: `release`, or else `master`.
    pub fn release_branch(&self) -> &str {
        self.release.as_deref().unwrap_or(&self.master)
    }

    /// The prefix of released tickets' saved work: `saved`, or else [`DEFAULT_SAVED_PREFIX`].
    pub fn saved_prefix(&self) -> &str {
        self.saved.as_deref().unwrap_or(DEFAULT_SAVED_PREFIX)
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
    /// The most the hub's stored attachment files may total, in MiB; unset is
    /// [`crate::protocol::attachments::DEFAULT_BUDGET_MB`]. Upkeep removes the files of closed
    /// tickets to stay under it, and an upload is refused while open tickets alone fill it.
    #[serde(default)]
    pub attachment_budget_mb: Option<u64>,
}

impl HubConfig {
    /// The attachment budget in bytes.
    #[must_use]
    pub fn attachment_budget_bytes(&self) -> u64 {
        self.attachment_budget_mb
            .unwrap_or(crate::protocol::attachments::DEFAULT_BUDGET_MB)
            .saturating_mul(1024 * 1024)
    }
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
    /// Checks that stay on when upkeep files a remedy, each a family (`clippy`) or a whole
    /// check name (`family:name`). The `never-caught` rule also skips these checks.
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
    /// The CLI model ID for each provider and ticket tier, as `provider -> tier -> id`.
    #[serde(default)]
    pub models: BTreeMap<String, BTreeMap<String, String>>,
    /// The reasoning effort each app uses for tickets without an `effort:` label.
    #[serde(default)]
    pub default_effort: BTreeMap<String, String>,
    /// `[pool.cli.<app>]`: the program each app's CLI turns run, by app.
    #[serde(default)]
    pub cli: BTreeMap<String, CliProgram>,
}

impl PoolConfig {
    /// The reasoning effort for a hand-off whose ticket has no `effort:` label.
    pub fn default_effort(&self, app: &str) -> &str {
        self.default_effort
            .get(app)
            .map_or("medium", String::as_str)
    }

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

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Resource {
    pub per_core: f64,
}

/// Overrides for one machine.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Machine {
    /// The operating system, when the declaration states it.
    pub os: Option<Os>,
    /// The check families the machine can run; `None` means every family.
    pub families: Option<Vec<String>>,
    /// The families among `families` that the machine lists, builds and runs in tip rounds;
    /// `None` means every family in `families`. The landing gate ignores this list and runs
    /// every family in `families`.
    ///
    /// A slow machine whose results are needed only for the families that must pass on its
    /// platform names those here. A family left out is one this machine never lists or builds
    /// in a tip round: the round keys its checks from the listing another machine kept at the
    /// hub and waits for another machine's results, as for a family the machine does not run.
    pub tip_families: Option<Vec<String>>,
    /// Scarce resources by name, with each one's capacity.
    pub resources: BTreeMap<String, u32>,
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

    /// Whether some declared machine can verify `family`'s checks in tip rounds: one whose
    /// `families` list and `tip_families` list, where it has them, both name the family, and
    /// which runs `os` when `os` is given. A machine whose operating system is not declared
    /// counts only when `os` is `None`.
    pub fn runs_family(&self, os: Option<Os>, family: &str) -> bool {
        let names = |list: &Option<Vec<String>>| {
            list.as_ref()
                .is_none_or(|list| list.iter().any(|f| f == family))
        };
        self.machine.is_empty()
            || self.machine.values().any(|machine| {
                os.is_none_or(|os| machine.os == Some(os))
                    && names(&machine.families)
                    && names(&machine.tip_families)
            })
    }

    /// Each family in `manifest` that no declared machine can run on a platform it must pass
    /// on, with that platform (`None` for a family that may pass anywhere and that no machine
    /// runs at all). Tip verification waits for a pass on each platform a family names, so a
    /// family in this list keeps `master` from moving forever.
    pub fn unrunnable_families(
        &self,
        manifest: &crate::manifest::Manifest,
    ) -> Vec<(String, Option<Os>)> {
        let mut missing = Vec::new();
        for (name, declared) in &manifest.families {
            let platforms = match &declared.family.platforms {
                crate::manifest::Platforms::Any(_) => vec![None],
                crate::manifest::Platforms::Each(list) => list.iter().copied().map(Some).collect(),
            };
            for os in platforms {
                if !self.runs_family(os, name) {
                    missing.push((name.clone(), os));
                }
            }
        }
        missing
    }

    /// The `[pool]` table: nonempty model IDs and efforts.
    fn validate_pool(&self) -> Result<(), (String, String)> {
        let fail = |key: &str, message: &str| Err((key.to_owned(), message.to_owned()));
        for (app, table) in &self.pool.models {
            for (tier, id) in table {
                if id.trim().is_empty() {
                    return fail(&format!("pool.models.{app}.{tier}"), "must not be empty");
                }
            }
        }
        for (app, effort) in &self.pool.default_effort {
            if effort.trim().is_empty() {
                return fail(&format!("pool.default_effort.{app}"), "must not be empty");
            }
        }
        Ok(())
    }

    /// `[repository]`'s refs and `[land]`: `staging`, `master` and `green` are three different
    /// refs, however each is written, `saved` is not empty, and the base age is positive.
    fn validate_landing(&self) -> Result<(), (String, String)> {
        let fail = |key: &str, message: &str| Err((key.to_owned(), message.to_owned()));
        if self
            .repository
            .saved
            .as_deref()
            .is_some_and(|saved| saved.trim().is_empty())
        {
            return fail("repository.saved", "must not be empty");
        }
        if crate::refname::remote_ref(&self.repository.staging)
            == crate::refname::remote_ref(&self.repository.master)
        {
            return fail("repository.master", "must differ from repository.staging");
        }
        let same =
            |a: &str, b: &str| crate::refname::remote_ref(a) == crate::refname::remote_ref(b);
        if let Some(green) = &self.repository.green
            && (green.trim().is_empty()
                || same(green, &self.repository.staging)
                || same(green, &self.repository.master))
        {
            return fail(
                "repository.green",
                "must name a branch other than repository.staging and repository.master",
            );
        }
        if !positive(self.land.max_base_age_hours) {
            return fail("land.max_base_age_hours", "must be above zero");
        }
        Ok(())
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
        self.validate_landing()?;
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
        self.validate_pool()?;
        self.validate_cli()?;
        for (name, resource) in &self.resources {
            if !resource.per_core.is_finite() || resource.per_core < 0.0 {
                return fail(
                    &format!("resources.{name}.per_core"),
                    "must be finite and nonnegative",
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
#[path = "config_tests.rs"]
mod tests;
