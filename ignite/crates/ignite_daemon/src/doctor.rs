//! The machine's state and health: `ignite status` prints what the daemon is doing, and
//! `ignite doctor` checks that the machine can do its work and says what to fix.
//!
//! Both read the machine through traits ([`Host`], [`TreeSource`], [`StatusSection`]), so the tests
//! run them against fakes. Each `doctor` finding is one line that names the problem and what to
//! do about it.

use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;

use ignite_core::config::{Config, Machine, Os};
use ignite_core::ids::{Commit, Seq};
use ignite_core::manifest::{Manifest, Platforms};
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_os::hostcpu::HostCpu;
use serde_json::json;

use crate::loop_health::LoopHealth;
use crate::sched;
use crate::server::{Context, Verb};
use crate::update::ExecutableStore;

mod capacity;
pub mod listener;
mod loops;
mod mirror;
mod running;
mod scanning;
mod tip_round;
mod trees;

pub use tip_round::{GreenAt, TipRoundSection};

pub(crate) use capacity::{CapacitySection, FleetSection, machine_facts};
pub use loops::LoopsSection;
pub use running::{ExecutableOf, RunningSection};
pub use trees::{GitTrees, TreeInfo, TreeSource, TreesSection};

/// The most lines `status` prints.
const MAX_LINES: usize = 40;

/// Facts about the machine that need the operating system.
pub trait Host {
    fn hostname(&self) -> String;
    fn cores(&self) -> u32;
    fn physical_cores(&self) -> u32 {
        self.cores()
    }
    fn memory_mb(&self) -> u64;
    /// Gigabytes free on the disk that holds `path`.
    fn disk_free_gb(&self, path: &Path) -> Option<u64>;
    /// Size of the volume and bytes available to the daemon.
    fn disk_space(&self, _path: &Path) -> Option<ignite_os::system::DiskSpace> {
        None
    }
    /// Where the program called `name` is found on the search path.
    fn tool_path(&self, name: &str) -> Option<PathBuf>;
    fn path_exists(&self, path: &Path) -> bool;
}

/// The running machine.
pub struct NativeHost;

pub(crate) fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl Host for NativeHost {
    /// The short hostname in lower case, the way `ignite.toml` names machines: macOS reports
    /// the computer name's case, such as `JARVIS`.
    ///
    /// Windows's `hostname` takes no `-s`, and the computer name is in `COMPUTERNAME` in upper
    /// case, such as `CYGNUS`.
    fn hostname(&self) -> String {
        if cfg!(windows) {
            return std::env::var("COMPUTERNAME")
                .unwrap_or_default()
                .to_lowercase();
        }
        command_output("hostname", &["-s"])
            .unwrap_or_default()
            .to_lowercase()
    }

    fn cores(&self) -> u32 {
        ignite_os::Native.cores()
    }

    fn physical_cores(&self) -> u32 {
        ignite_os::Native.physical_cores()
    }

    fn memory_mb(&self) -> u64 {
        #[cfg(windows)]
        let bytes = ignite_os::system::total_memory_bytes().ok();
        #[cfg(not(windows))]
        let bytes = command_output("sysctl", &["-n", "hw.memsize"])
            .and_then(|text| text.parse::<u64>().ok());
        bytes.map_or(0, |bytes| bytes / (1024 * 1024))
    }

    fn disk_free_gb(&self, path: &Path) -> Option<u64> {
        ignite_os::system::disk_free_bytes(path)
            .ok()
            .map(|bytes| bytes / ignite_core::capacity::BYTES_PER_GIB)
    }

    fn disk_space(&self, path: &Path) -> Option<ignite_os::system::DiskSpace> {
        ignite_os::system::disk_space(path).ok()
    }

    fn tool_path(&self, name: &str) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH")?).collect();
        // A login service starts with a short `PATH`, so also look where the usual installers put
        // tools.
        if let Some(home) = ignite_core::config::home_dir(|name| std::env::var_os(name)) {
            dirs.push(home.join(".cargo").join("bin"));
        }
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        // A Windows program's file ends in `.exe`, which its name on a command line leaves out.
        let file = if cfg!(windows) {
            format!("{name}.exe")
        } else {
            name.to_owned()
        };
        dirs.into_iter()
            .map(|dir| dir.join(&file))
            .find(|p| p.is_file())
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// Lines of aligned `label  value` text.
fn columns(rows: &[(String, String)]) -> String {
    let width = rows.iter().map(|(label, _)| label.len()).max().unwrap_or(0);
    let mut text = String::new();
    for (label, value) in rows {
        let _ = writeln!(text, "{label:<width$}  {value}");
    }
    text
}

/// Cuts text to [`MAX_LINES`] lines, saying how many were left out.
fn cut(text: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= MAX_LINES {
        return lines.join("\n");
    }
    let mut kept = lines[..MAX_LINES - 1].join("\n");
    let _ = write!(kept, "\n… {} more lines", lines.len() - (MAX_LINES - 1));
    kept
}

/// Which `status` flags show a section.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    Always,
    /// Only with `--admission`.
    Admission,
    /// Only with `--exploration`.
    Exploration,
}

/// One part of `ignite status`, supplied by the module that owns the state: the scheduler, the
/// trees, the pool and idle work.
pub trait StatusSection: Send + Sync {
    fn when(&self) -> When {
        When::Always
    }
    fn title(&self) -> &str;
    fn lines(&self) -> Vec<String>;
}

/// Cores and memory, from the scheduler.
pub struct CoresSection(pub Arc<dyn Fn() -> sched::Status + Send + Sync>);

impl StatusSection for CoresSection {
    fn title(&self) -> &'static str {
        "cores"
    }

    fn lines(&self) -> Vec<String> {
        let s = (self.0)();
        let memory = s
            .free_memory_mb
            .map_or_else(String::new, |mb| format!(", {mb} MB memory free"));
        let swap = s
            .swap_used_mb
            .map_or_else(String::new, |mb| format!(", {mb} MB swap"));
        let pressed = if s.memory_pressed {
            "; memory short, admitting nothing"
        } else {
            ""
        };
        let (kept, held) = (s.tip_build, s.tip_build_used);
        let tip = (kept > 0).then(|| format!(", {kept} kept for tip builds ({held} in use)"));
        let tip = tip.unwrap_or_default();
        let mut lines = vec![format!(
            "{} of {} granted to Ignite (using {}, {} counted), {} busy outside it, \
             at most {} per command{tip}{memory}{swap}{pressed}",
            s.granted, s.cores, s.used, s.committed, s.outside, s.max_per_command
        )];
        lines.extend(s.priority_grants.iter().map(|(ticket, count)| {
            let commands = if *count == 1 { "command" } else { "commands" };
            format!(
                "{count} {commands} for {ticket} granted cores with priority because the ticket holds master back"
            )
        }));
        lines
    }
}

/// The cores admitted and the requests waiting for them, from the scheduler.
pub struct AdmissionSection(pub Arc<dyn Fn() -> sched::Status + Send + Sync>);

impl StatusSection for AdmissionSection {
    fn when(&self) -> When {
        When::Admission
    }

    fn title(&self) -> &'static str {
        "admission"
    }

    fn lines(&self) -> Vec<String> {
        let s = (self.0)();
        let mut lines: Vec<String> = s
            .classes
            .iter()
            .map(|c| {
                format!(
                    "{:<5} {} running on {} cores, {} waiting",
                    c.class.as_str(),
                    c.running,
                    c.cores,
                    c.waiting
                )
            })
            .collect();
        lines.extend(s.resources.iter().map(|r| {
            format!(
                "{}: {} of {} held, {} waiting",
                r.name, r.held, r.capacity, r.waiting
            )
        }));
        lines
    }
}

/// `ignite status`: prints the machine's state, assembled from the sections the daemon's modules
/// supply.
pub struct StatusVerb {
    pub sections: Vec<Box<dyn StatusSection>>,
}

impl StatusVerb {
    /// The verb with `sections`, after the commands running on this machine
    /// ([`RunningSection`]).
    pub fn new(mut sections: Vec<Box<dyn StatusSection>>) -> Self {
        sections.insert(0, Box::new(RunningSection(crate::steps::global(), None)));
        Self { sections }
    }

    /// The verb, with each running verb shown with the commit of the executable it runs under,
    /// as `executable_of` reads it for a handle.
    #[must_use]
    pub fn running_under(mut self, executable_of: ExecutableOf) -> Self {
        self.sections[0] = Box::new(RunningSection(crate::steps::global(), Some(executable_of)));
        self
    }

    /// The text and the same data as JSON, for the flags in `args`.
    pub fn render(&self, machine: &str, version: Option<&Commit>, args: &[String]) -> Reply {
        let has = |flag: &str| args.iter().any(|a| a == flag);
        let (admission, exploration) = (has("--admission"), has("--exploration"));
        let version = version.map_or_else(|| "unknown".to_owned(), ToString::to_string);
        let mut rows = vec![("machine".to_owned(), format!("{machine}, ignite {version}"))];
        let mut data = serde_json::Map::new();
        data.insert("machine".into(), json!(machine));
        data.insert("version".into(), json!(version));
        for section in &self.sections {
            let shown = match section.when() {
                When::Always => true,
                When::Admission => admission,
                When::Exploration => exploration,
            };
            if !shown {
                continue;
            }
            let lines = section.lines();
            data.insert(section.title().to_owned(), json!(lines));
            for (i, line) in lines.iter().enumerate() {
                let label = if i == 0 { section.title() } else { "" };
                rows.push((label.to_owned(), line.clone()));
            }
        }
        Reply {
            exit: Exit::Success,
            text: cut(columns(&rows).trim_end()),
            json: Some(serde_json::Value::Object(data)),
            handle: None,
        }
    }
}

impl Verb for StatusVerb {
    fn name(&self) -> &'static str {
        "status"
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let version = running_version();
        self.render(ctx.machine.as_str(), version.as_ref(), &req.args)
    }
}

/// The version recorded by [`remember_running_version`] when the daemon started.
static RUNNING: std::sync::OnceLock<Option<Commit>> = std::sync::OnceLock::new();

/// Records the commit the daemon was started from. Called once at startup, because the
/// launch path, `bin/current`, is a link that a prepared update repoints while the old
/// executable still runs; reading it later reports the next version, not this one.
pub fn remember_running_version(version: Option<Commit>) {
    let _ = RUNNING.set(version);
}

/// The commit the running executable was stored under.
fn running_version() -> Option<Commit> {
    if let Some(version) = RUNNING.get() {
        return version.clone();
    }
    let exe = ignite_os::paths::canonicalize(std::env::current_exe().ok()?).ok()?;
    ExecutableStore::version_of(&exe)
}

/// One line of `doctor`'s report.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Finding {
    pub ok: bool,
    /// Something worth attention that does not stop the machine's work; such a finding is
    /// `ok` and has a fix.
    pub warning: bool,
    pub label: String,
    pub text: String,
    /// What to do, for a problem.
    pub fix: Option<String>,
}

impl Finding {
    fn ok(label: &str, text: impl Into<String>) -> Self {
        Self {
            ok: true,
            warning: false,
            label: label.to_owned(),
            text: text.into(),
            fix: None,
        }
    }

    pub(crate) fn warning(label: &str, text: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            ok: true,
            warning: true,
            label: label.to_owned(),
            text: text.into(),
            fix: Some(fix.into()),
        }
    }

    fn problem(label: &str, text: impl Into<String>, fix: impl Into<String>) -> Self {
        Self {
            ok: false,
            warning: false,
            label: label.to_owned(),
            text: text.into(),
            fix: Some(fix.into()),
        }
    }

    /// The line `doctor` prints.
    pub fn line(&self) -> String {
        let level = if self.warning {
            "WARNING"
        } else if self.ok {
            "ok"
        } else {
            "PROBLEM"
        };
        match &self.fix {
            None => format!("{level:<8} {:<9} {}", self.label, self.text),
            Some(fix) => format!("{level:<8} {:<9} {} To fix: {fix}", self.label, self.text),
        }
    }
}

/// What `doctor` looks at.
pub struct DoctorInput<'a> {
    pub config: &'a Config,
    pub repo: &'a Path,
    pub host: &'a dyn Host,
    pub trees: &'a dyn TreeSource,
    pub hub: &'a dyn ignite_core::traits::Hub,
    /// Sends the write probe that tells the hub's listeners apart.
    pub probe: &'a dyn listener::WriteProbe,
    /// The check manifests, or the reason they cannot be read.
    pub manifest: std::result::Result<Manifest, String>,
    pub version: Option<Commit>,
    pub fleet: Option<ignite_core::fleet::FleetNotice>,
    /// The recent clone-aware measurement of the managed build directories, if available.
    pub used_tree_bytes: Option<u64>,
    /// The part of the measured usage held by leased ticket trees.
    pub leased_tree_bytes: Option<u64>,
}

/// The first word of each command that a family the machine can run, or a product such a family
/// needs, runs. A product that only other families need is never built here, so its tools are
/// not asked for: a machine that runs no browser family needs no `npm`.
fn needed_tools(manifest: &Manifest, runnable: &BTreeSet<String>) -> BTreeSet<String> {
    const SHELL_WORDS: &[&str] = &["cd", "export", "set", "echo", "test", "["];
    let mut commands: Vec<&str> = Vec::new();
    let mut products: BTreeSet<&str> = BTreeSet::new();
    for (name, declared) in &manifest.families {
        if runnable.contains(name) {
            let family = &declared.family;
            commands.extend(family.run.as_deref());
            commands.extend(family.list.as_deref());
            products.extend(family.needs.iter().map(String::as_str));
            products.extend(family.list_needs.iter().flatten().map(String::as_str));
        }
    }
    commands.extend(
        products
            .into_iter()
            .filter_map(|name| manifest.products.get(name))
            .map(|p| p.build.as_str()),
    );
    commands
        .into_iter()
        .filter_map(|c| c.split_whitespace().next())
        .filter(|w| !SHELL_WORDS.contains(w))
        .filter(|w| {
            w.chars()
                .all(|c| c.is_alphanumeric() || matches!(c, '-' | '_'))
        })
        .map(ToOwned::to_owned)
        .collect()
}

fn can_run(machine: &Machine, name: &str, platforms: &Platforms, os: Option<Os>) -> bool {
    let listed = machine
        .families
        .as_ref()
        .is_none_or(|f| f.iter().any(|n| n == name));
    let on_os = match platforms {
        Platforms::Any(_) => true,
        Platforms::Each(list) => os.is_none_or(|os| list.contains(&os)),
    };
    listed && on_os
}

/// Runs every check and returns one finding per fact.
pub fn diagnose(input: &DoctorInput) -> Vec<Finding> {
    let mut findings = Vec::new();
    let hostname = input.host.hostname();
    findings.push(Finding::ok(
        "version",
        input.version.as_ref().map_or_else(
            || "ignite is not running from a stored build".to_owned(),
            |v| format!("ignite {v}"),
        ),
    ));

    let url = &input.config.hub.url;
    findings.push(match input.hub.feed(Seq(0), 0) {
        // A reachable hub may still be the read-only listener.
        Ok(_) => listener::finding(&input.config.hub, input.probe)
            .unwrap_or_else(|| Finding::ok("hub", format!("reachable at {url}."))),
        Err(error) => {
            let dns = ignite_core::tailnet::diagnose_url(
                url,
                ignite_core::tailnet::FailureKind::ConnectOrTls,
            );
            Finding::problem(
                "hub",
                match &dns {
                    Some(dns) => format!("cannot reach the hub at {url}: {error}. {dns}"),
                    None => format!("cannot reach the hub at {url}: {error}."),
                },
                "check that Tailscale is connected on this machine and that the daemon on the \
                 machine that runs the hub is up, then run `ignite doctor` again",
            )
        }
    });

    findings.extend(model_findings(input.config));
    findings.extend(cli_findings(input.config, input.host));

    let default = Machine::default();
    let machine = input.config.machine(&hostname).unwrap_or(&default);
    let os = machine
        .os
        .or_else(Os::current)
        .expect("Ignite runs on macOS or Windows");
    let facts = machine_facts(input.host, os);
    let derived = ignite_core::capacity::derive(&facts);
    findings.push(Finding::ok(
        "machine",
        format!(
            "{hostname}, {} physical cores, {} logical cores, {} MB memory.",
            facts.physical_cores, facts.logical_cores, facts.memory_mb
        ),
    ));

    trees::findings(input, derived.landing_trees, &mut findings);
    family_findings(input, machine, &mut findings);
    findings
}

/// Warns when a configured provider has no model tiers to offer the hub.
fn model_findings(config: &Config) -> impl Iterator<Item = Finding> + '_ {
    config
        .pool
        .cli
        .keys()
        .filter(|app| {
            config
                .pool
                .models
                .get(*app)
                .is_none_or(std::collections::BTreeMap::is_empty)
        })
        .map(|app| {
            Finding::warning(
                "models",
                format!("[pool.models.{app}] names no models, so {app} offers no tiers."),
                format!("add the tiers and CLI model IDs to [pool.models.{app}]"),
            )
        })
}

/// A warning for each app in CLI mode on this machine whose program is not on the `PATH` the
/// daemon runs with: every turn of the app would fail to start, and the app would go on a CLI
/// hold.
fn cli_findings(config: &Config, host: &dyn Host) -> Vec<Finding> {
    config
        .pool
        .cli
        .iter()
        .filter(|(_, program)| host.tool_path(&program.program).is_none())
        .map(|(app, program)| {
            Finding::warning(
                "cli",
                format!(
                    "{app} is in CLI mode on this machine, but its program `{}` is not on \
                     the daemon's PATH, so no turn can start.",
                    program.program
                ),
                format!(
                    "install `{}` or put its folder on the PATH of the daemon's service",
                    program.program
                ),
            )
        })
        .collect()
}

fn family_findings(input: &DoctorInput, machine: &Machine, findings: &mut Vec<Finding>) {
    let manifest = match &input.manifest {
        Ok(manifest) => manifest,
        Err(error) => {
            findings.push(Finding::problem(
                "families",
                format!("cannot read the check manifests: {error}."),
                format!(
                    "fix the file named, in {}",
                    input.config.repository.checks.display()
                ),
            ));
            return;
        }
    };
    let os = Os::current();
    let unknown: Vec<&String> = machine
        .families
        .iter()
        .flatten()
        .filter(|name| !manifest.families.contains_key(*name))
        .collect();
    if !unknown.is_empty() {
        let names: Vec<&str> = unknown.iter().map(|s| s.as_str()).collect();
        findings.push(Finding::problem(
            "families",
            format!(
                "ignite.toml lists families that no manifest declares: {}.",
                names.join(", ")
            ),
            "correct the machine's `families` in ignite.toml",
        ));
    }
    let runnable: BTreeSet<String> = manifest
        .families
        .iter()
        .filter(|(name, d)| can_run(machine, name, &d.family.platforms, os))
        .map(|(name, _)| name.clone())
        .collect();
    let names: Vec<&str> = runnable.iter().map(String::as_str).collect();
    findings.push(Finding::ok(
        "families",
        format!(
            "can run {} of {}: {}.",
            names.len(),
            manifest.families.len(),
            names.join(", ")
        ),
    ));

    let mut tools = needed_tools(manifest, &runnable);
    tools.extend(["git".to_owned(), "cargo".to_owned()]);
    let missing: Vec<&String> = tools
        .iter()
        .filter(|tool| input.host.tool_path(tool).is_none())
        .collect();
    findings.push(if missing.is_empty() {
        Finding::ok(
            "tools",
            format!(
                "found {}.",
                tools.iter().cloned().collect::<Vec<_>>().join(", ")
            ),
        )
    } else {
        let names: Vec<&str> = missing.iter().map(|s| s.as_str()).collect();
        Finding::problem(
            "tools",
            format!("not found on the search path: {}.", names.join(", ")),
            "install them, or remove the families that need them from this machine's `families`",
        )
    });
}

/// Reads the manifests under the repository's check directory.
fn read_manifest(repo: &Path, dir: &Path) -> std::result::Result<Manifest, String> {
    let dir = repo.join(dir);
    let mut files = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text =
                std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
            files.push((path, text));
        }
    }
    files.sort();
    Manifest::from_files(&files).map_err(|e| e.to_string())
}

/// `ignite doctor`: checks the machine's health and says what to fix.
#[derive(Default)]
pub struct DoctorVerb {
    /// The daemon's background loops; `None` where there are none, as in a test.
    pub loops: Option<LoopHealth>,
    /// The machine's ticket mirror (see [`crate::mirror`]); `None` where there is none.
    pub mirror: Option<PathBuf>,
    /// The emergency pause this machine enforces; `None` where there is none.
    pub emergency: Option<Arc<crate::emergency::Emergencies>>,
    /// The daemon's live tree configuration and last fleet notice.
    pub managed_trees: Option<crate::trees::Trees>,
}

impl Verb for DoctorVerb {
    fn name(&self) -> &'static str {
        "doctor"
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, _req: &Request) -> Reply {
        let trees = GitTrees {
            repo: ctx.repo.clone(),
            dir: ctx.repo.join(&ctx.config.repository.trees),
        };
        let (used_tree_bytes, leased_tree_bytes) = self
            .managed_trees
            .as_ref()
            .map_or((None, None), crate::trees::used_bytes);
        let input = DoctorInput {
            config: &ctx.config,
            repo: &ctx.repo,
            host: &NativeHost,
            trees: &trees,
            hub: ctx.hub.as_ref(),
            probe: &listener::HttpProbe,
            manifest: read_manifest(&ctx.repo, &ctx.config.repository.checks),
            version: running_version(),
            fleet: self
                .managed_trees
                .as_ref()
                .and_then(crate::trees::Trees::fleet_notice),
            used_tree_bytes,
            leased_tree_bytes,
        };
        let mut findings = diagnose(&input);
        if let Some(health) = &self.loops {
            findings.extend(loops::findings(health));
        }
        if let Some(path) = &self.mirror {
            findings.push(mirror::finding(path, ctx.hub.as_ref(), ctx.clock.now()));
        }
        findings.extend(self.emergency.as_ref().and_then(|e| e.finding()));
        findings.extend(scanning::finding());
        report(&findings)
    }
}

/// The reply for a list of findings: the exit code says whether any is a problem.
pub fn report(findings: &[Finding]) -> Reply {
    let problems = findings.iter().filter(|f| !f.ok).count();
    let warnings = findings.iter().filter(|f| f.warning).count();
    let mut text = findings
        .iter()
        .map(Finding::line)
        .collect::<Vec<_>>()
        .join("\n");
    let _ = write!(
        text,
        "\n{}",
        match (problems, warnings) {
            (0, 0) => "healthy".to_owned(),
            (0, 1) => "healthy, 1 warning".to_owned(),
            (0, _) => format!("healthy, {warnings} warnings"),
            _ => format!("{problems} problems"),
        }
    );
    let unreachable = findings.iter().any(|f| !f.ok && f.label == "hub");
    Reply {
        exit: match (problems, unreachable) {
            (0, _) => Exit::Success,
            (_, true) => Exit::Unreachable,
            _ => Exit::CheckFailed,
        },
        text,
        json: Some(json!({"findings": findings.iter().map(|f| json!({
            "ok": f.ok, "warning": f.warning, "label": f.label, "text": f.text, "fix": f.fix,
        })).collect::<Vec<_>>()})),
        handle: None,
    }
}

#[cfg(test)]
mod tests;
