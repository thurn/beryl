//! Repairing a Cargo target directory whose incremental build of one crate is damaged.
//!
//! rustc keeps each crate's incremental work in `<profile>/incremental/<crate>-<hash>/`, with a
//! copy or hard link of each codegen unit's object file in `<profile>/deps`. When a compile
//! is ended partway, for example by a daemon restart, or when objects from two compiles get
//! mixed, the next compile can reuse a damaged unit: on 2026-11-18 the test target `session`
//! failed to link in one landing tree with "Undefined symbols for architecture arm64" for its
//! own `.llvm.<n>` symbols, in 1-4 s on every retry, while the same commit built everywhere else.
//! Deleting that crate's incremental folder and its `deps` files fixed it.
//!
//! Two things here do that by themselves:
//!
//! - After a build fails, [`compile_failure`] reads which crates Cargo could not compile and
//!   whether the failure looks like damage to the tree's build rather than to the source (a link
//!   error, a compiler crash, unreadable metadata). The caller clears those crates
//!   ([`clear_crates`]) and builds once more before it reports the failure.
//! - A link error can come from a damaged library rather than from the crate being linked. On
//!   2026-11-19 the binary `record` failed to link in a landing tree for symbols that the library
//!   of one of its dependencies referenced and no longer held, five times in twelve hours: each
//!   repair cleared `record`, which was not damaged, and left the library.
//!   So [`compile_failure`] also names each crate whose library the linker reports an object
//!   of (`lib<crate>-<hash>.rlib[<n>](…)` on macOS, `lib<crate>-<hash>.rlib(…)` elsewhere).
//! - A build that was ended partway marks its target directory ([`mark_suspect`]). The next build
//!   there first clears each unit (one build of a crate, `<crate>-<hash>`) that a compile left
//!   unfinished ([`clear_if_suspect`]): rustc names a session folder `s-…-working` until the
//!   compile finishes.

use std::fs;
use std::path::{Path, PathBuf};

use ignite_core::protocol::Usage;
use ignite_core::traits::{BuildResult, RunResult};

use super::rejoin::BuildKey;
use super::{BUILD_KEY_ENV, BuildManager, Plan, lock, read_units};
use crate::supervise::{ResultFile, SpecFile};

/// The file in a target directory that says a build there was ended partway.
pub const SUSPECT_FILE: &str = ".ignite-suspect";

/// Text in a failed build's output that says the tree's build products are damaged, not the
/// source: the same inputs built elsewhere, or would after the crate's cache is cleared.
const DAMAGE: &[&str] = &[
    // The linkers of macOS, Linux and Windows, and Cargo's own line around them.
    "error: linking with `",
    "Undefined symbols for architecture",
    "undefined reference to `",
    "unresolved external symbol",
    // rustc crashed, or could not read its own outputs or incremental session.
    "internal compiler error",
    "found invalid metadata files",
    "incremental compilation",
    "failed to load dep-graph",
];

/// What a failed Cargo build said about the crates it could not compile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompileFailure {
    /// The crate names, as rustc names their incremental folders and `deps` files.
    pub crates: Vec<String>,
    /// Whether the output says the build products are damaged (see [`DAMAGE`]).
    pub damaged: bool,
}

/// The crates a failed Cargo build's output says could not compile, or `None` when it names
/// none, as when the build failed before compiling.
///
/// Cargo ends each crate's errors with ``error: could not compile `<package>` (<target>)``,
/// where the target is `lib`, `lib test`, `bin "<name>"`, `test "<name>"` and so on. A target
/// with a name compiles to a crate of that name; a library to one named after the package.
/// Build scripts are left out: rustc keeps no incremental work for them.
pub fn compile_failure(output: &str) -> Option<CompileFailure> {
    let mut crates: Vec<String> = Vec::new();
    for line in output.lines() {
        let Some((_, rest)) = line.split_once("could not compile `") else {
            continue;
        };
        let Some((package, rest)) = rest.split_once('`') else {
            continue;
        };
        if let Some(name) = crate_of(package, rest)
            && !crates.contains(&name)
        {
            crates.push(name);
        }
    }
    if crates.is_empty() {
        return None;
    }
    let damaged = DAMAGE.iter().any(|text| output.contains(text));
    if damaged {
        for name in linked_crates(output) {
            if !crates.contains(&name) {
                crates.push(name);
            }
        }
    }
    Some(CompileFailure { crates, damaged })
}

/// The crates whose libraries a linker's error names an object of, in the order they first
/// appear.
///
/// The linkers name the member of a static library that holds a symbol or a reference as the
/// library's file followed by the member: `libfoo-<hash>.rlib[15](foo-<hash>.cgu.rcgu.o)` (the
/// macOS linker) or `…/libfoo-<hash>.rlib(foo-<hash>.cgu.rcgu.o)` (GNU ld, lld, MSVC). The
/// linker's command line, which Cargo prints too, lists every library without a member, so a
/// library is named only when a `[` or `(` follows `.rlib`.
fn linked_crates(output: &str) -> Vec<String> {
    let mut crates: Vec<String> = Vec::new();
    for line in output.lines() {
        let mut rest = line;
        while let Some(at) = rest.find(".rlib") {
            let (before, after) = rest.split_at(at);
            rest = &after[".rlib".len()..];
            if !(rest.starts_with('[') || rest.starts_with('(')) {
                continue;
            }
            let file = before
                .rsplit(|c: char| {
                    c.is_whitespace() || matches!(c, '/' | '\\' | '"' | '(' | '{' | ',')
                })
                .next()
                .unwrap_or(before);
            let Some((krate, hash)) = file.strip_prefix("lib").and_then(|f| f.rsplit_once('-'))
            else {
                continue;
            };
            if hash.len() == 16 && hashed(hash) && !crates.iter().any(|c| c == krate) {
                crates.push(krate.to_owned());
            }
        }
    }
    crates
}

/// The crate a `could not compile` line names, from its package and the text after it.
fn crate_of(package: &str, rest: &str) -> Option<String> {
    let target = rest
        .trim_start()
        .strip_prefix('(')
        .and_then(|t| t.split_once(')'))
        .map_or("", |(target, _)| target);
    if target.starts_with("build script") {
        return None;
    }
    let named = target.split('"').nth(1).filter(|name| !name.is_empty());
    Some(named.unwrap_or(package).replace('-', "_"))
}

/// The profile directories of a target directory: those that hold `deps` or `incremental`,
/// one level down (`<target>/debug`) or two for a cross build (`<target>/<triple>/debug`).
fn profile_dirs(target: &Path) -> Vec<PathBuf> {
    let is_profile = |dir: &Path| dir.join("deps").is_dir() || dir.join("incremental").is_dir();
    let mut found = Vec::new();
    for child in subdirectories(target) {
        if is_profile(&child) {
            found.push(child);
        } else {
            found.extend(subdirectories(&child).into_iter().filter(|d| is_profile(d)));
        }
    }
    found
}

fn subdirectories(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort();
    dirs
}

/// The names of the entries in `dir`.
fn names(dir: &Path) -> Vec<String> {
    fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// Whether `rest` starts with a 16-digit hexadecimal Cargo metadata hash.
fn hashed(rest: &str) -> bool {
    rest.len() >= 16 && rest.as_bytes()[..16].iter().all(u8::is_ascii_hexdigit)
}

/// Whether `name`, in a `deps` folder, is a codegen unit object of crate `krate` named without
/// a unit hash, `<crate>.<unit>….rcgu.o`, which belongs to no one unit.
fn is_loose_object(name: &str, krate: &str) -> bool {
    name.strip_prefix(krate).is_some_and(|r| r.starts_with('.')) && name.ends_with(".rcgu.o")
}

/// The folder of per-unit fingerprints in a profile directory. Cargo names each unit's folder
/// there `<crate>-<hash>`, as it names the unit's files in `deps`.
const FINGERPRINTS: &str = ".fingerprint";

/// Deletes `path`, a file or a folder, and adds it to `deleted` when it was there.
fn remove(path: &Path, deleted: &mut Vec<PathBuf>) {
    let removed = if fs::symlink_metadata(path).is_ok_and(|m| m.is_dir()) {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    if removed.is_ok() {
        deleted.push(path.to_owned());
    }
}

/// Deletes one unit, `<crate>-<hash>`, of a profile directory: its fingerprint folder first, and
/// its `deps` files only once that is gone. Returns what it deleted.
///
/// The order is the one `sweep_superseded` in `trees/disk.rs` keeps. Stopped partway, or with a
/// fingerprint folder that cannot be deleted, it leaves either the whole unit or files that no
/// fingerprint names, which Cargo never reads. The other order could leave a fingerprint whose
/// outputs are gone, which Cargo reports as `FailedToReadMetadata`.
fn clear_unit(profile: &Path, unit: &str) -> Vec<PathBuf> {
    let mut deleted = Vec::new();
    let fingerprint = profile.join(FINGERPRINTS).join(unit);
    remove(&fingerprint, &mut deleted);
    if fs::symlink_metadata(&fingerprint).is_ok() {
        return deleted;
    }
    let deps = profile.join("deps");
    for name in names(&deps) {
        let stem = name.split('.').next().unwrap_or(&name);
        if stem == unit || stem.strip_prefix("lib") == Some(unit) {
            remove(&deps.join(&name), &mut deleted);
        }
    }
    deleted
}

/// The units of crate `krate` in a profile's `deps` folder, as `<crate>-<hash>`.
fn units_of(profile: &Path, krate: &str) -> Vec<String> {
    let mut units: Vec<String> = Vec::new();
    for name in names(&profile.join("deps")) {
        let stem = name.split('.').next().unwrap_or(&name);
        let unit = stem
            .strip_prefix("lib")
            .filter(|s| s.starts_with(krate))
            .unwrap_or(stem);
        let is_unit = unit
            .strip_prefix(krate)
            .and_then(|r| r.strip_prefix('-'))
            .is_some_and(|hash| hash.len() == 16 && hashed(hash));
        if is_unit && !units.iter().any(|u| u == unit) {
            units.push(unit.to_owned());
        }
    }
    units.sort();
    units
}

/// Deletes the units, incremental folders and other `deps` files of each of `crates` in every
/// profile of the target directory `target`, and returns what it deleted. Cargo then compiles
/// each from scratch: a unit without a fingerprint is out of date.
///
/// This clears every build of each crate, because a failed build's output names crates, not
/// units ([`compile_failure`]).
pub fn clear_crates(target: &Path, crates: &[String]) -> Vec<PathBuf> {
    let mut deleted = Vec::new();
    for profile in profile_dirs(target) {
        for krate in crates {
            for unit in units_of(&profile, krate) {
                deleted.extend(clear_unit(&profile, &unit));
            }
        }
        // What is left is no unit's: codegen unit objects named without a hash, and the
        // incremental folders, which rustc rebuilds from nothing when they are missing.
        let deps = profile.join("deps");
        for name in names(&deps) {
            if crates.iter().any(|c| is_loose_object(&name, c)) {
                remove(&deps.join(&name), &mut deleted);
            }
        }
        let incremental = profile.join("incremental");
        for name in names(&incremental) {
            if let Some((krate, _)) = name.rsplit_once('-')
                && crates.iter().any(|c| c == krate)
            {
                remove(&incremental.join(&name), &mut deleted);
            }
        }
    }
    deleted
}

/// Whether an incremental folder holds a session that a compile left unfinished: rustc names a
/// session folder `s-…-working` until the compile finishes.
fn unfinished(folder: &Path) -> bool {
    names(folder)
        .iter()
        .any(|session| session.starts_with("s-") && session.ends_with("-working"))
}

/// The unit, `<crate>-<hash>`, whose incremental work is in `folder`, an incremental folder of
/// `profile`, or `None` when no `deps` object shares a codegen unit with it.
///
/// The incremental folder's hash is not the unit's: rustc derives it from the crate's stable ID.
/// But each session keeps its codegen units as `<cgu>.o`, and the unit's objects in `deps` are
/// `<crate>-<hash>.<cgu>.<suffix>.rcgu.o`, and the codegen unit names of different units of one
/// crate differ. rustc starts a session by linking in the files of the one before, so a session
/// left unfinished still holds the units of the last compile that finished.
fn unit_of_incremental(profile: &Path, folder: &Path) -> Option<String> {
    let crate_name = folder.file_name()?.to_str()?.rsplit_once('-')?.0.to_owned();
    let mut codegen_units: Vec<String> = Vec::new();
    for session in subdirectories(folder) {
        for name in names(&session) {
            if let Some(cgu) = name.strip_suffix(".o") {
                codegen_units.push(cgu.to_owned());
            }
        }
    }
    names(&profile.join("deps")).into_iter().find_map(|name| {
        let object = name.strip_suffix(".rcgu.o")?;
        let (unit, rest) = object.split_once('.')?;
        let cgu = rest.split('.').next()?;
        let hash = unit.strip_prefix(&crate_name)?.strip_prefix('-')?;
        (hashed(hash) && codegen_units.iter().any(|c| c == cgu)).then(|| unit.to_owned())
    })
}

/// Marks `target` as holding a build that was ended partway, when it is a directory.
pub fn mark_suspect(target: &Path) {
    if target.is_dir() {
        let _ = fs::write(
            target.join(SUSPECT_FILE),
            "a build here was ended partway\n",
        );
    }
}

/// If `target` is marked as holding a build that was ended partway, clears each unit whose
/// incremental session a compile left unfinished ([`unfinished`]) and removes the mark. Returns
/// the units cleared, as `<crate>-<hash>`, or the incremental folder's name for one whose unit
/// is not known.
///
/// Only the unit whose compile was cut off can be damaged; the other builds of its crate, such
/// as its test build or a build with other features, are left alone. A unit is cleared
/// fingerprint first ([`clear_unit`]), and its incremental folder last. When no `deps` object
/// names the unit, its compile was the first and wrote no codegen unit, so only the incremental
/// folder goes. Should anything damaged survive this, the build fails with a link error and
/// [`BuildManager::run`] clears that crate whole and builds once more.
pub fn clear_if_suspect(target: &Path) -> Vec<String> {
    let mark = target.join(SUSPECT_FILE);
    if !mark.is_file() {
        return Vec::new();
    }
    let mut cleared: Vec<String> = Vec::new();
    for profile in profile_dirs(target) {
        let incremental = profile.join("incremental");
        for name in names(&incremental) {
            let folder = incremental.join(&name);
            if !unfinished(&folder) {
                continue;
            }
            match unit_of_incremental(&profile, &folder) {
                Some(unit) => {
                    clear_unit(&profile, &unit);
                    cleared.push(unit);
                }
                None => cleared.push(name),
            }
            let _ = fs::remove_dir_all(&folder);
        }
    }
    if !cleared.is_empty() {
        eprintln!(
            "ignite: cleared {} in {}: a build there was ended partway",
            cleared.join(", "),
            target.display()
        );
    }
    let _ = fs::remove_file(mark);
    cleared
}

/// Marks the target directories of a command that was ended partway, when it may have been
/// compiling: a build manager's build (its `IGNITE_BUILD_KEY` names its output directories in
/// `tree`), or any other command that runs `cargo`, whose target directory is `target` in its
/// working directory.
pub fn mark_ended(spec: &SpecFile, tree: &Path) {
    let key = spec
        .env
        .iter()
        .find(|(name, _)| name == BUILD_KEY_ENV)
        .and_then(|(_, value)| serde_json::from_str::<BuildKey>(value).ok());
    if let Some(key) = key {
        for output in &key.outputs {
            mark_suspect(&tree.join(output));
        }
    } else if spec.args.iter().any(|arg| arg == "cargo") {
        mark_suspect(&spec.cwd.join("target"));
    }
}

/// Whether a command was ended before it finished. A wrapper can catch the stop signal and
/// return a numeric exit code, so check the supervisor's record for failed commands.
pub fn ended_partway(run: &RunResult) -> bool {
    run.exit.is_none()
        || run.hung
        || (run.exit != Some(0) && ResultFile::ended_from_outside(&run.output))
}

/// The exit codes a shell or a wrapper such as `uv run` returns when a signal ended its child:
/// 128 plus `SIGHUP`, `SIGINT`, `SIGKILL` or `SIGTERM`. Cargo fails with 101 of its own accord.
const SIGNAL_EXIT_CODES: [i32; 4] = [129, 130, 137, 143];

/// Whether a failed build's exit code says that a signal ended it, though the run has no
/// `ended` mark: a tree reset ends every process inside the tree with a signal of its own, not
/// through [`crate::supervise::Run::end`]. Such a failure is not remembered, since the same
/// inputs may well build. It does not count as [`ended_partway`], which would clear the
/// target's unfinished units: the code alone is not proof enough for that.
pub fn reports_signal(run: &RunResult) -> bool {
    run.exit
        .is_some_and(|code| SIGNAL_EXIT_CODES.contains(&code))
}

/// Reports whether `run` ended partway and, if so, marks each target where it may have compiled
/// so the next build there clears what it left unfinished.
pub fn mark_if_ended(run: &RunResult, targets: &[PathBuf]) -> bool {
    let ended = ended_partway(run);
    if ended {
        for target in targets {
            mark_suspect(target);
        }
    }
    ended
}

/// What a command's Cargo build said when it failed before the command did anything else: it
/// exited with a code, `reported` is false (it reported no result), and its output names crates
/// that could not compile.
pub fn run_failure(reported: bool, run: &RunResult) -> Option<CompileFailure> {
    if reported || run.hung || run.exit.is_none_or(|code| code == 0) {
        return None;
    }
    compile_failure(&fs::read_to_string(&run.output).unwrap_or_default())
}

/// The sum of two commands' usage, for a build run twice.
fn add_usage(a: &Usage, b: &Usage) -> Usage {
    Usage {
        queued_s: a.queued_s + b.queued_s,
        wall_s: a.wall_s + b.wall_s,
        cpu_s: a.cpu_s + b.cpu_s,
        instructions: match (a.instructions, b.instructions) {
            (Some(a), Some(b)) => Some(a + b),
            (one, other) => one.or(other),
        },
        peak_mb: a.peak_mb.max(b.peak_mb),
    }
}

impl BuildManager {
    /// Runs the build command, first clearing what a build ended partway left, and once more
    /// after clearing the crates that failed when the failure looks like damage to this tree's
    /// build (see the module documentation). The flag is as for [`BuildManager::run_once`].
    pub(super) fn run(&self, plan: &Plan) -> (BuildResult, bool) {
        let targets: Vec<PathBuf> = plan
            .product
            .outputs
            .iter()
            .map(|output| plan.path.join(output))
            .collect();
        for target in &targets {
            clear_if_suspect(target);
        }
        let (first, units_file) = match self.run_once(plan) {
            Ok(ran) => ran,
            Err(failed) => return (failed, false),
        };
        if mark_if_ended(&first, &targets) {
            return Self::outcome(first, &units_file, true);
        }
        let repair = run_failure(false, &first)
            .filter(|failure| failure.damaged || self.held_elsewhere(plan));
        let Some(failure) = repair else {
            return Self::outcome(first, &units_file, false);
        };
        let first_units = read_units(&units_file);
        let _ = fs::remove_file(&units_file);
        for target in &targets {
            clear_crates(target, &failure.crates);
        }
        eprintln!(
            "ignite: {} failed to build in {}; clearing {} as a damaged target directory and building once more",
            plan.request.product,
            plan.request.tree,
            failure.crates.join(", ")
        );
        let activity = crate::steps::current();
        let cancelled = || {
            activity
                .as_ref()
                .is_some_and(|activity| activity.cancelled())
        };
        let _retry_slot = match self.prepare_space(plan, &cancelled) {
            Ok(slot) => slot,
            Err(failed) => return (failed, false),
        };
        let (second, units_file) = match self.run_once(plan) {
            Ok(ran) => ran,
            Err(failed) => return (failed, false),
        };
        let ended = mark_if_ended(&second, &targets);
        match Self::outcome(second, &units_file, ended) {
            (BuildResult::Built { usage, units }, remember) => {
                let mut all = first_units;
                all.extend(units);
                let usage = add_usage(&first.usage, &usage);
                (BuildResult::Built { usage, units: all }, remember)
            }
            (BuildResult::Failed { output, usage }, remember) => {
                let usage = add_usage(&first.usage, &usage);
                (BuildResult::Failed { output, usage }, remember)
            }
            other => other,
        }
    }

    /// Whether another tree holds the same product and packages built from the same inputs, so
    /// a failure here comes from this tree's build products and not from the source.
    fn held_elsewhere(&self, plan: &Plan) -> bool {
        lock(&self.state).held.iter().any(|(request, hash)| {
            request.tree != plan.request.tree
                && request.product == plan.request.product
                && request.packages == plan.request.packages
                && *hash == plan.hash
        })
    }
}

#[cfg(test)]
#[path = "repair_tests.rs"]
mod tests;
