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
use ignite_core::traits::Result;
use serde_json::json;

use crate::loop_health::LoopHealth;
use crate::sched;
use crate::server::{Context, Verb};
use crate::update::ExecutableStore;

mod loops;
mod trees;

pub use loops::LoopsSection;

/// The most lines `status` and `doctor` print.
const MAX_LINES: usize = 40;

/// Facts about the machine that need the operating system.
pub trait Host {
    fn hostname(&self) -> String;
    fn cores(&self) -> u32;
    fn memory_mb(&self) -> u64;
    /// Gigabytes free on the disk that holds `path`.
    fn disk_free_gb(&self, path: &Path) -> Option<u64>;
    /// Where the program called `name` is found on the search path.
    fn tool_path(&self, name: &str) -> Option<PathBuf>;
    fn path_exists(&self, path: &Path) -> bool;
}

/// The running machine.
pub struct NativeHost;

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

impl Host for NativeHost {
    /// The short hostname in lower case, the way `ignite.toml` names machines: macOS reports
    /// the computer name's case, such as `JARVIS`.
    fn hostname(&self) -> String {
        command_output("hostname", &["-s"])
            .unwrap_or_default()
            .to_lowercase()
    }

    fn cores(&self) -> u32 {
        std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(1))
    }

    fn memory_mb(&self) -> u64 {
        command_output("sysctl", &["-n", "hw.memsize"])
            .and_then(|text| text.parse::<u64>().ok())
            .map_or(0, |bytes| bytes / (1024 * 1024))
    }

    fn disk_free_gb(&self, path: &Path) -> Option<u64> {
        let text = command_output("df", &["-Pk", &path.to_string_lossy()])?;
        let kilobytes: u64 = text
            .lines()
            .nth(1)?
            .split_whitespace()
            .nth(3)?
            .parse()
            .ok()?;
        Some(kilobytes / (1024 * 1024))
    }

    fn tool_path(&self, name: &str) -> Option<PathBuf> {
        let mut dirs: Vec<PathBuf> = std::env::split_paths(&std::env::var_os("PATH")?).collect();
        // A login service starts with a short `PATH`, so also look where the usual installers put
        // tools.
        if let Some(home) = std::env::var_os("HOME") {
            dirs.push(PathBuf::from(home).join(".cargo").join("bin"));
        }
        dirs.extend(["/opt/homebrew/bin", "/usr/local/bin"].map(PathBuf::from));
        dirs.into_iter()
            .map(|dir| dir.join(name))
            .find(|p| p.is_file())
    }

    fn path_exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// One build tree, as the daemon knows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TreeInfo {
    pub name: String,
    pub path: PathBuf,
    pub head: Option<Commit>,
}

/// The build trees that exist.
pub trait TreeSource: Send + Sync {
    fn trees(&self) -> Result<Vec<TreeInfo>>;

    /// The names of the directories directly under the trees folder, hidden ones left out,
    /// whether or not git knows them as trees.
    fn directories(&self) -> Vec<String> {
        Vec::new()
    }
}

/// The trees git has registered under the repository's tree directory. A tree whose directory
/// was deleted is still registered until `git worktree prune`, which is how `doctor` finds it.
pub struct GitTrees {
    pub repo: PathBuf,
    pub dir: PathBuf,
}

impl TreeSource for GitTrees {
    fn trees(&self) -> Result<Vec<TreeInfo>> {
        Ok(ignite_git::worktree::list(&self.repo)?
            .into_iter()
            .filter(|tree| tree.path.starts_with(&self.dir))
            .map(|tree| TreeInfo {
                name: tree
                    .path
                    .file_name()
                    .map_or_else(String::new, |n| n.to_string_lossy().into_owned()),
                path: tree.path,
                head: tree.head,
            })
            .collect())
    }

    fn directories(&self) -> Vec<String> {
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return Vec::new();
        };
        entries
            .flatten()
            .filter(|entry| entry.file_type().is_ok_and(|t| t.is_dir()))
            .filter_map(|entry| entry.file_name().into_string().ok())
            .filter(|name| !name.starts_with('.'))
            .collect()
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
        vec![format!(
            "{} of {} granted to Ignite, {} busy outside it, at most {} per command{memory}",
            s.granted, s.cores, s.outside, s.max_per_command
        )]
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

/// The names of the trees a machine declares: `tree-01`… for its ticket trees and `landing-1`…
/// for its landing trees.
pub fn configured_tree_names(ticket_trees: u32, landing_trees: u32) -> Vec<String> {
    (1..=ticket_trees)
        .map(|n| format!("tree-{n:02}"))
        .chain((1..=landing_trees).map(|n| format!("landing-{n}")))
        .collect()
}

/// How many of the declared trees exist, which are gone, and which directories under the trees
/// folder are not declared.
pub struct TreesSection {
    pub source: Arc<dyn TreeSource>,
    /// The declared trees' names ([`configured_tree_names`]); only these are counted.
    pub configured: Vec<String>,
    pub host: Arc<dyn Host + Send + Sync>,
}

impl StatusSection for TreesSection {
    fn title(&self) -> &'static str {
        "trees"
    }

    fn lines(&self) -> Vec<String> {
        match self.source.trees() {
            Err(error) => vec![format!("cannot list trees: {error}")],
            Ok(trees) => {
                let declared: Vec<&TreeInfo> = trees
                    .iter()
                    .filter(|t| self.configured.contains(&t.name))
                    .collect();
                // Directories git does not know as trees are named too.
                let extra: BTreeSet<String> = trees
                    .iter()
                    .map(|t| t.name.clone())
                    .chain(self.source.directories())
                    .filter(|name| !self.configured.contains(name))
                    .collect();
                let missing: Vec<&TreeInfo> = declared
                    .iter()
                    .copied()
                    .filter(|t| !self.host.path_exists(&t.path))
                    .collect();
                let mut lines = vec![format!(
                    "{} of {} present",
                    declared.len() - missing.len(),
                    self.configured.len()
                )];
                lines.extend(
                    missing
                        .iter()
                        .map(|t| format!("{}: directory is missing", t.name)),
                );
                if !extra.is_empty() {
                    let names: Vec<&str> = extra.iter().map(String::as_str).collect();
                    lines.push(format!("not in ignite.toml: {}", names.join(", ")));
                }
                lines
            }
        }
    }
}

/// `ignite status`: prints the machine's state, assembled from the sections the daemon's modules
/// supply.
pub struct StatusVerb {
    pub sections: Vec<Box<dyn StatusSection>>,
}

impl StatusVerb {
    pub fn new(sections: Vec<Box<dyn StatusSection>>) -> Self {
        Self { sections }
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
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
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

    fn warning(label: &str, text: impl Into<String>, fix: impl Into<String>) -> Self {
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
    /// The check manifests, or the reason they cannot be read.
    pub manifest: std::result::Result<Manifest, String>,
    pub version: Option<Commit>,
}

/// The first word of each command that a family or product runs, when the machine can run it.
fn needed_tools(manifest: &Manifest, runnable: &BTreeSet<String>) -> BTreeSet<String> {
    const SHELL_WORDS: &[&str] = &["cd", "export", "set", "echo", "test", "["];
    let mut commands: Vec<&str> = manifest
        .products
        .values()
        .map(|p| p.build.as_str())
        .collect();
    for (name, declared) in &manifest.families {
        if runnable.contains(name) {
            commands.extend(declared.family.run.as_deref());
            commands.extend(declared.family.list.as_deref());
        }
    }
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
        Ok(_) => Finding::ok("hub", format!("reachable at {url}.")),
        Err(error) => Finding::problem(
            "hub",
            format!("cannot reach the hub at {url}: {error}."),
            "check that Tailscale is connected on this machine and that the daemon on the \
             machine that runs the hub is up, then run `ignite doctor` again",
        ),
    });

    let Some(machine) = input.config.machine(&hostname) else {
        findings.push(Finding::problem(
            "machine",
            format!("ignite.toml declares no [machine.{hostname}]."),
            format!("add a [machine.{hostname}] table with its trees and run `ignite install`"),
        ));
        return findings;
    };
    findings.push(Finding::ok(
        "machine",
        format!(
            "{hostname}, {} cores, {} MB memory.",
            input.host.cores(),
            input.host.memory_mb()
        ),
    ));

    trees::findings(input, machine, &mut findings);
    family_findings(input, machine, &mut findings);
    findings
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
        let input = DoctorInput {
            config: &ctx.config,
            repo: &ctx.repo,
            host: &NativeHost,
            trees: &trees,
            hub: ctx.hub.as_ref(),
            manifest: read_manifest(&ctx.repo, &ctx.config.repository.checks),
            version: running_version(),
        };
        let mut findings = diagnose(&input);
        if let Some(health) = &self.loops {
            findings.extend(loops::findings(health));
        }
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
        text: cut(&text),
        json: Some(json!({"findings": findings.iter().map(|f| json!({
            "ok": f.ok, "warning": f.warning, "label": f.label, "text": f.text, "fix": f.fix,
        })).collect::<Vec<_>>()})),
        handle: None,
    }
}

#[cfg(test)]
mod tests;
