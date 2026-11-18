//! Input resolution: from a family's `inputs` patterns to the concrete paths a check reads, and
//! from those paths to the git object IDs that its input hash covers.
//!
//! An `inputs` entry is one of three things (see [`InputPattern`]):
//!
//! - A path pattern, relative to the checkout's root. A literal path names a tracked file or a
//!   directory that holds tracked files. A pattern ending in `/**` names that directory, so its
//!   one tree ID covers every file below it, files added later included. Any other pattern is a
//!   glob over the tracked files, where `*` and `?` match within one path segment and `**`
//!   matches any number of segments.
//! - `crate:<package>`: the directory of that workspace package and of every workspace package
//!   it depends on, transitively, development and build dependencies included.
//! - `crate-closure`: the same, starting from the package that holds the check.
//!
//! A family that reads a Cargo package also reads its workspace's manifest (which holds the
//! shared dependency versions, lints and profiles), its lock file and the nearest toolchain
//! file, so all three are added whenever a crate pattern is resolved. A family that reads
//! only paths has no use for a Cargo lock file, so none is added for it.
//!
//! The crate graph comes from `cargo metadata --format-version 1 --no-deps`, passed in as text,
//! and the tracked files from the caller: this module does no I/O.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::Path;

use serde::Deserialize;

use crate::ids::Oid;
use crate::manifest::{InputPattern, Manifest};
use crate::traits::{Git, Result as GitResult};

/// The lock file name Cargo writes at a workspace's root.
const LOCK_FILE: &str = "Cargo.lock";

/// The manifest at a workspace's root.
const WORKSPACE_MANIFEST: &str = "Cargo.toml";

/// The toolchain file names rustup looks for, in the order it prefers them.
const TOOLCHAIN_FILES: &[&str] = &["rust-toolchain.toml", "rust-toolchain"];

/// One workspace package, as `cargo metadata` reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Package {
    pub name: String,
    /// The package's directory, relative to the checkout's root.
    pub dir: String,
    /// Workspace packages it depends on, development dependencies included.
    pub deps: Vec<String>,
    /// The directory of the workspace that holds it, relative to the checkout's root.
    pub workspace: String,
}

/// The workspace packages of every Cargo workspace a family reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CrateGraph {
    pub packages: Vec<Package>,
}

/// A pattern that names nothing that exists.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InputError {
    pub pattern: String,
    pub message: String,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pattern, self.message)
    }
}

impl std::error::Error for InputError {}

fn input_error(pattern: impl Into<String>, message: impl Into<String>) -> InputError {
    InputError {
        pattern: pattern.into(),
        message: message.into(),
    }
}

#[derive(Deserialize)]
struct Metadata {
    packages: Vec<MetadataPackage>,
    workspace_root: String,
}

#[derive(Deserialize)]
struct MetadataPackage {
    name: String,
    manifest_path: String,
    dependencies: Vec<MetadataDependency>,
}

#[derive(Deserialize)]
struct MetadataDependency {
    name: String,
    /// Set only for a path dependency, which is how a workspace package depends on another.
    path: Option<String>,
}

impl CrateGraph {
    /// Adds the packages of one workspace from the JSON of
    /// `cargo metadata --format-version 1 --no-deps`. `checkout` is the checkout's absolute
    /// root, which the paths in the JSON are made relative to.
    pub fn add_workspace(&mut self, metadata: &str, checkout: &str) -> Result<(), InputError> {
        let parsed: Metadata = serde_json::from_str(metadata)
            .map_err(|e| input_error("cargo metadata", e.to_string()))?;
        let workspace = relative(&parsed.workspace_root, checkout)?;
        let names: BTreeSet<&str> = parsed.packages.iter().map(|p| p.name.as_str()).collect();
        for package in &parsed.packages {
            let manifest = relative(&package.manifest_path, checkout)?;
            let dir = manifest
                .rsplit_once('/')
                .map_or(String::new(), |(dir, _)| dir.to_owned());
            let mut deps: Vec<String> = package
                .dependencies
                .iter()
                .filter(|d| d.path.is_some() && names.contains(d.name.as_str()))
                .map(|d| d.name.clone())
                .collect();
            deps.sort();
            deps.dedup();
            self.packages.push(Package {
                name: package.name.clone(),
                dir,
                deps,
                workspace: workspace.clone(),
            });
        }
        Ok(())
    }

    fn package(&self, name: &str) -> Option<&Package> {
        self.packages.iter().find(|p| p.name == name)
    }
}

/// `path` relative to `root`.
fn relative(path: &str, root: &str) -> Result<String, InputError> {
    let root = root.trim_end_matches('/');
    let rest = path
        .strip_prefix(root)
        .filter(|rest| rest.is_empty() || rest.starts_with('/'))
        .ok_or_else(|| input_error(path, format!("is not inside {root}")))?;
    Ok(rest.trim_start_matches('/').to_owned())
}

/// The sorted, deduplicated paths (files and directories) that `patterns` name. `test_package`
/// is the package holding the check, which `crate-closure` starts from; `tracked` lists every
/// file git tracks in the checkout. A path inside a directory that is also named is dropped,
/// because the directory's tree ID covers it.
pub fn resolve(
    patterns: &[InputPattern],
    test_package: Option<&str>,
    graph: &CrateGraph,
    tracked: &[String],
) -> Result<Vec<String>, InputError> {
    let mut paths = BTreeSet::new();
    let mut crates = BTreeSet::new();
    for pattern in patterns {
        match pattern {
            InputPattern::Path(text) => paths.extend(match_path(text, tracked)?),
            InputPattern::Crate(package) => crates.extend(closure(package, graph)?),
            InputPattern::CrateClosure => {
                let package = test_package.ok_or_else(|| {
                    input_error("crate-closure", "the check belongs to no package")
                })?;
                crates.extend(closure(package, graph)?);
            }
        }
    }
    let mut workspaces = BTreeSet::new();
    for name in &crates {
        let package = graph
            .package(name)
            .ok_or_else(|| input_error(name.clone(), "is not a workspace package"))?;
        paths.insert(package.dir.clone());
        workspaces.insert(package.workspace.as_str());
    }
    for workspace in workspaces {
        for name in [LOCK_FILE, WORKSPACE_MANIFEST] {
            let file = join(workspace, name);
            if tracked.contains(&file) {
                paths.insert(file);
            }
        }
        if let Some(toolchain) = toolchain_file(workspace, tracked) {
            paths.insert(toolchain);
        }
    }
    Ok(outermost(paths))
}

/// The package and every workspace package it depends on, transitively, sorted.
pub fn closure(package: &str, graph: &CrateGraph) -> Result<Vec<String>, InputError> {
    let mut seen = BTreeSet::new();
    let mut stack = vec![package.to_owned()];
    while let Some(name) = stack.pop() {
        if seen.contains(&name) {
            continue;
        }
        let found = graph
            .package(&name)
            .ok_or_else(|| input_error(format!("crate:{name}"), "names no workspace package"))?;
        stack.extend(found.deps.iter().cloned());
        seen.insert(name);
    }
    Ok(seen.into_iter().collect())
}

/// Each of `paths` with its git object ID in the working tree `tree`, sorted by path.
pub fn object_ids(git: &dyn Git, tree: &Path, paths: &[String]) -> GitResult<Vec<(String, Oid)>> {
    let mut ids = git.object_ids(tree, paths)?;
    ids.sort();
    Ok(ids)
}

/// What scores and admission group changes by. The workspace package that holds a path, or
/// outside every package the path's second-level directory (or its first-level directory, or
/// the file itself, for a path that is not that deep).
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Area {
    Package(String),
    Directory(String),
}

impl fmt::Display for Area {
    /// A package prints as its name and a directory with a trailing `/`, so the two never meet.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Area::Package(name) => f.write_str(name),
            Area::Directory(dir) => write!(f, "{dir}/"),
        }
    }
}

/// The area that holds `path`, relative to the checkout's root.
pub fn area(path: &str, graph: &CrateGraph) -> Area {
    let package = graph
        .packages
        .iter()
        .filter(|p| !p.dir.is_empty() && is_within(path, &p.dir))
        .max_by_key(|p| p.dir.len());
    if let Some(package) = package {
        return Area::Package(package.name.clone());
    }
    Area::Directory(directory_area(path))
}

/// The area name of a path outside every package: its first two directories, or its first
/// directory when it is directly inside one, or the file's own name when it is at the top.
/// Admission and scores both group by this, so they agree on every path.
pub fn directory_area(path: &str) -> String {
    let segments: Vec<&str> = path.split('/').collect();
    let depth = if segments.len() > 2 { 2 } else { 1 };
    segments[..depth].join("/")
}

/// One problem `lint` found: the manifest key and the pattern at fault.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LintFinding {
    /// Such as `family.rust.inputs[1]`.
    pub key: String,
    pub error: InputError,
}

impl fmt::Display for LintFinding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.key, self.error)
    }
}

/// Every pattern in `manifest` that matches no tracked file, and every `crate:` that names no
/// workspace package, in the order of the manifest's keys.
pub fn lint(manifest: &Manifest, graph: &CrateGraph, tracked: &[String]) -> Vec<LintFinding> {
    let mut lists: BTreeMap<String, &[InputPattern]> = BTreeMap::new();
    for (name, declared) in &manifest.families {
        lists.insert(format!("family.{name}.inputs"), &declared.family.inputs);
    }
    for (name, product) in &manifest.products {
        lists.insert(format!("product.{name}.inputs"), &product.inputs);
    }
    let mut findings = Vec::new();
    for (key, patterns) in lists {
        for (index, pattern) in patterns.iter().enumerate() {
            let checked = match pattern {
                InputPattern::Path(text) => match_path(text, tracked).map(drop),
                InputPattern::Crate(package) => closure(package, graph).map(drop),
                InputPattern::CrateClosure => Ok(()),
            };
            if let Err(error) = checked {
                findings.push(LintFinding {
                    key: format!("{key}[{index}]"),
                    error,
                });
            }
        }
    }
    findings
}

/// The paths a path pattern names; an error if it names nothing tracked.
fn match_path(pattern: &str, tracked: &[String]) -> Result<Vec<String>, InputError> {
    let none = || input_error(pattern, "matches no tracked file");
    if let Some(dir) = pattern.strip_suffix("/**")
        && !has_wildcard(dir)
    {
        return if tracked.iter().any(|f| is_within(f, dir) && f != dir) {
            Ok(vec![dir.to_owned()])
        } else {
            Err(none())
        };
    }
    if !has_wildcard(pattern) {
        let path = pattern.trim_end_matches('/');
        return if tracked.iter().any(|f| is_within(f, path)) {
            Ok(vec![path.to_owned()])
        } else {
            Err(none())
        };
    }
    let segments: Vec<&str> = pattern.split('/').collect();
    let matched: Vec<String> = tracked
        .iter()
        .filter(|f| glob(&segments, &f.split('/').collect::<Vec<_>>()))
        .cloned()
        .collect();
    if matched.is_empty() {
        Err(none())
    } else {
        Ok(matched)
    }
}

fn has_wildcard(text: &str) -> bool {
    text.contains(['*', '?'])
}

/// Whether the path segments match the pattern segments, `**` standing for any number of them.
fn glob(pattern: &[&str], path: &[&str]) -> bool {
    match pattern.split_first() {
        None => path.is_empty(),
        Some((&"**", rest)) => (0..=path.len()).any(|skip| glob(rest, &path[skip..])),
        Some((first, rest)) => path
            .split_first()
            .is_some_and(|(segment, tail)| segment_matches(first, segment) && glob(rest, tail)),
    }
}

/// Whether one segment matches a pattern of literal characters, `*` and `?`.
fn segment_matches(pattern: &str, segment: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let segment: Vec<char> = segment.chars().collect();
    let (mut p, mut s) = (0, 0);
    // Where the last `*` was, and the segment position it has consumed up to.
    let mut star: Option<(usize, usize)> = None;
    while s < segment.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == segment[s]) {
            p += 1;
            s += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = Some((p, s));
            p += 1;
        } else if let Some((star_p, star_s)) = star {
            p = star_p + 1;
            s = star_s + 1;
            star = Some((star_p, star_s + 1));
        } else {
            return false;
        }
    }
    pattern[p..].iter().all(|&c| c == '*')
}

/// Whether `path` is `dir` or lies below it.
fn is_within(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_owned()
    } else {
        format!("{dir}/{name}")
    }
}

/// The toolchain file rustup would use for a workspace: the first one found in its directory
/// or the nearest directory above it.
fn toolchain_file(workspace: &str, tracked: &[String]) -> Option<String> {
    let mut dir = Some(workspace);
    while let Some(current) = dir {
        for name in TOOLCHAIN_FILES {
            let candidate = join(current, name);
            if tracked.contains(&candidate) {
                return Some(candidate);
            }
        }
        dir = match current.rsplit_once('/') {
            Some((parent, _)) => Some(parent),
            None if current.is_empty() => None,
            None => Some(""),
        };
    }
    None
}

/// The paths with every path inside another of them removed.
fn outermost(paths: BTreeSet<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    // Sorting puts `a` before `a/b`, but `a-b` sorts between them, so test against every kept
    // path rather than only the last.
    for path in paths {
        if !kept.iter().any(|dir| is_within(&path, dir)) {
            kept.push(path);
        }
    }
    kept
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
