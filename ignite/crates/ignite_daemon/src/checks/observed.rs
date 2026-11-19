//! What a Cargo build was observed to read: the repository files each workspace package's
//! compile units name in Cargo's dep-info files, and the paths its build script watches (see
//! "Observed inputs" in `docs/ignite.md`).
//!
//! A `nextest` family's checks hash `crate-closure`, the directories of the package holding a
//! test and of the workspace packages it depends on. A test also reads, at compile time, every
//! file an `include_str!` or `include_bytes!` names and every file its build scripts read. The
//! compiler lists the first kind in the dep-info file it writes beside each unit
//! (`<target>/<profile>/deps/<unit>.d`), and a build script names the second with
//! `cargo:rerun-if-changed=` lines in its output (`<target>/<profile>/build/<package>-<hash>/output`).
//! Ignite reads both after a fresh listing, when `cargo nextest list` has just compiled every
//! test binary it lists, so the dep-info of every unit the tests use is current.
//!
//! For each package the read list holds the tracked repository paths its units name outside
//! the package's own directory, which `crate-closure` already covers. A path is a file, or a
//! directory when a build script watches one. Dep-info files that an earlier build with other
//! features left in the target directory are read too, which can only add paths. A path that
//! is not tracked, such as a generated file under the target directory, is left out: git holds
//! no object ID for it, and it is a product of the build, not an input.
//!
//! The read lists ride in the family's kept listing (see `listings`), so one machine's
//! observation serves every machine whose tree has the same listing hash.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::fs;
use std::path::Path;

use ignite_core::inputs::CrateGraph;

/// The repository paths each workspace package's compile units read outside its own directory,
/// by package name, each list sorted.
pub(crate) type Reads = BTreeMap<String, Vec<String>>;

/// The two prefixes a build script's output gives a watched path, the second from Cargo 1.77.
const RERUN: [&str; 2] = ["cargo:rerun-if-changed=", "cargo::rerun-if-changed="];

/// The read lists of every package in `graph`, from the target directory `target` (relative to
/// the checkout at `checkout`). A target directory that does not exist reads nothing: Cargo
/// writes where the family's product says only when the manifest is right, and the cache
/// audits are what catch a manifest that is not. `None` only when the checkout's path is not
/// text.
pub(crate) fn observe(
    checkout: &Path,
    target: &str,
    graph: &CrateGraph,
    tracked: &[String],
) -> Option<Reads> {
    let profiles = fs::read_dir(checkout.join(target))
        .into_iter()
        .flatten()
        .filter_map(Result::ok);
    let mut dep_info: Vec<String> = Vec::new();
    let mut build_outputs: Vec<(String, String, Option<String>)> = Vec::new();
    for profile in profiles {
        let profile = profile.path();
        if let Ok(units) = fs::read_dir(profile.join("deps")) {
            for unit in units.filter_map(Result::ok) {
                let path = unit.path();
                if path.extension().is_some_and(|e| e == "d")
                    && let Ok(text) = fs::read_to_string(&path)
                {
                    dep_info.push(text);
                }
            }
        }
        if let Ok(scripts) = fs::read_dir(profile.join("build")) {
            for script in scripts.filter_map(Result::ok) {
                let dir = script.path();
                let Ok(output) = fs::read_to_string(dir.join("output")) else {
                    continue;
                };
                let name = script.file_name().to_string_lossy().into_owned();
                let root_output = fs::read_to_string(dir.join("root-output")).ok();
                build_outputs.push((name, output, root_output));
            }
        }
    }
    let checkout = checkout.to_str()?;
    // Cargo runs the compiler in the workspace's root and names the workspace's own files
    // relative to it.
    let workspace = graph
        .packages
        .iter()
        .map(|p| p.workspace.as_str())
        .filter(|workspace| within(target, workspace))
        .max_by_key(|workspace| workspace.len())
        .unwrap_or("");
    Some(reads(
        &Observed {
            checkout,
            target,
            workspace,
            graph,
            tracked,
        },
        &dep_info,
        &build_outputs,
    ))
}

/// What [`reads`] works from, apart from the files' texts.
pub(crate) struct Observed<'a> {
    /// The checkout's absolute root, as the operating system writes it.
    pub checkout: &'a str,
    /// The target directory, relative to the checkout's root.
    pub target: &'a str,
    /// The root of the workspace the target directory belongs to, relative to the checkout's
    /// root, which the compiler's relative paths start from.
    pub workspace: &'a str,
    pub graph: &'a CrateGraph,
    pub tracked: &'a [String],
}

/// The read lists from the texts of the dep-info files and of the build scripts' outputs, each
/// output with its directory's name (`<package>-<hash>`) and the text of its `root-output` file.
pub(crate) fn reads(
    observed: &Observed<'_>,
    dep_info: &[String],
    build_outputs: &[(String, String, Option<String>)],
) -> Reads {
    let known = known_paths(observed.tracked);
    let checkout = slashes(observed.checkout);
    let mut found: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut add = |package: &ignite_core::inputs::Package, path: String| {
        if known.contains(path.as_str())
            && !within(&path, &package.dir)
            && !within(&path, observed.target)
        {
            found.entry(package.name.clone()).or_default().insert(path);
        }
    };
    for text in dep_info {
        let Some((output, deps)) = rule(text) else {
            continue;
        };
        let root = root_of(&output, observed.target).unwrap_or_else(|| checkout.clone());
        let relative: Vec<String> = deps
            .iter()
            .filter_map(|dep| {
                if is_absolute(dep) {
                    relative_to(dep, &root).or_else(|| relative_to(dep, &checkout))
                } else {
                    normalize(&format!("{}/{dep}", observed.workspace))
                }
            })
            .collect();
        // The first dependency is the unit's crate root, which says whose unit it is.
        let Some(package) = relative.first().and_then(|r| holder(r, observed.graph)) else {
            continue;
        };
        for path in relative {
            add(package, path);
        }
    }
    for (dir_name, output, root_output) in build_outputs {
        let Some(package) = script_package(dir_name, observed) else {
            continue;
        };
        let root = root_output
            .as_deref()
            .and_then(|text| root_of(text.trim(), observed.target))
            .unwrap_or_else(|| checkout.clone());
        for line in output.lines() {
            let Some(watched) = RERUN.iter().find_map(|prefix| line.strip_prefix(prefix)) else {
                continue;
            };
            let watched = slashes(watched.trim());
            let path = if is_absolute(&watched) {
                relative_to(&watched, &root).or_else(|| relative_to(&watched, &checkout))
            } else {
                normalize(&format!("{}/{watched}", package.dir))
            };
            if let Some(path) = path {
                add(package, path);
            }
        }
    }
    found
        .into_iter()
        .map(|(package, paths)| (package, paths.into_iter().collect()))
        .collect()
}

/// The paths of `reads` for `packages`, sorted and without repeats.
pub(crate) fn paths_for<'a>(
    reads: &'a Reads,
    packages: impl IntoIterator<Item = &'a String>,
) -> Vec<String> {
    let mut paths: BTreeSet<&String> = BTreeSet::new();
    for package in packages {
        paths.extend(reads.get(package).into_iter().flatten());
    }
    paths.into_iter().cloned().collect()
}

/// `reads` with only the paths that name a tracked file or a directory holding one.
pub(crate) fn tracked_only(reads: &Reads, tracked: &[String]) -> Reads {
    let known = known_paths(tracked);
    reads
        .iter()
        .map(|(package, paths)| {
            let kept = paths
                .iter()
                .filter(|p| known.contains(p.as_str()))
                .cloned()
                .collect();
            (package.clone(), kept)
        })
        .collect()
}

/// Every tracked file and every directory that holds one.
pub(crate) fn known_paths(tracked: &[String]) -> HashSet<&str> {
    let mut known: HashSet<&str> = HashSet::new();
    for file in tracked {
        let mut rest = file.as_str();
        known.insert(rest);
        while let Some((dir, _)) = rest.rsplit_once('/') {
            known.insert(dir);
            rest = dir;
        }
    }
    known
}

/// Every path of `reads`, sorted and without repeats.
pub(crate) fn every_path(reads: &Reads) -> Vec<String> {
    paths_for(reads, reads.keys())
}

/// The first rule of a dep-info file: its output and its dependencies, with the escapes of
/// spaces undone. Comment lines (`# env-dep:`) and the empty rules for each dependency that
/// follow the first rule are not read.
fn rule(text: &str) -> Option<(String, Vec<String>)> {
    let line = text
        .lines()
        .find(|line| !line.trim().is_empty() && !line.starts_with('#'))?;
    let mut words = split_words(line).into_iter();
    let output = words.next()?;
    let output = output.strip_suffix(':')?;
    Some((slashes(output), words.map(|w| slashes(&w)).collect()))
}

/// A makefile line's words: split at spaces, where `\ ` is a space within a word.
fn split_words(line: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                word.push(' ');
                chars.next();
            }
            ' ' | '\t' => {
                if !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
            }
            _ => word.push(c),
        }
    }
    if !word.is_empty() {
        words.push(word);
    }
    words
}

/// The checkout root a path under the target directory was written from: the part before
/// `/<target>/`. A build tree whose target directory was copied from another tree holds
/// dep-info files that name the other tree's root.
fn root_of(path: &str, target: &str) -> Option<String> {
    let path = slashes(path);
    let marker = format!("/{}/", target.trim_matches('/'));
    path.find(&marker).map(|at| path[..at].to_owned())
}

/// `path` relative to `root`, with `.` and `..` resolved, or `None` outside it.
fn relative_to(path: &str, root: &str) -> Option<String> {
    let rest = path.strip_prefix(root)?.strip_prefix('/')?;
    normalize(rest)
}

/// A relative path with `.` and `..` segments resolved; `None` when it climbs above its start.
fn normalize(path: &str) -> Option<String> {
    let mut parts: Vec<&str> = Vec::new();
    for segment in path.split('/') {
        match segment {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            other => parts.push(other),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

/// The innermost package whose directory holds `path`.
fn holder<'a>(path: &str, graph: &'a CrateGraph) -> Option<&'a ignite_core::inputs::Package> {
    graph
        .packages
        .iter()
        .filter(|p| !p.dir.is_empty() && within(path, &p.dir))
        .max_by_key(|p| p.dir.len())
}

/// The package a build script's directory (`<package>-<hash>`) belongs to, among the packages
/// of the workspace that holds the target directory.
fn script_package<'a>(
    dir_name: &str,
    observed: &Observed<'a>,
) -> Option<&'a ignite_core::inputs::Package> {
    let (name, hash) = dir_name.rsplit_once('-')?;
    if hash.is_empty() || !hash.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    observed
        .graph
        .packages
        .iter()
        .filter(|p| p.name == name && within(observed.target, &p.workspace))
        .max_by_key(|p| p.workspace.len())
}

fn slashes(path: &str) -> String {
    path.replace('\\', "/")
}

/// Whether a path with forward slashes is absolute, on Unix or on Windows (`C:/`).
fn is_absolute(path: &str) -> bool {
    path.starts_with('/') || path.as_bytes().get(1..3) == Some(b":/")
}

/// Whether `path` is `dir` or lies below it; every path lies in the checkout's root, `""`.
fn within(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path == dir
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ignite_core::inputs::Package;

    fn graph() -> CrateGraph {
        let package = |name: &str, dir: &str| Package {
            name: name.to_owned(),
            dir: dir.to_owned(),
            deps: Vec::new(),
            workspace: "rust".to_owned(),
        };
        CrateGraph {
            packages: vec![
                package("store", "rust/crates/store"),
                package("table", "rust/crates/table"),
                package("cards", "rust/crates/cards"),
            ],
        }
    }

    fn tracked() -> Vec<String> {
        [
            "rust/crates/store/src/lib.rs",
            "rust/crates/table/tests/commands.rs",
            "rust/crates/cards/build.rs",
            "rust/crates/cards/src/lib.rs",
            "data/store.tsv",
            "data/commands.json",
            "docs/decks/a.txt",
            "docs/my file.txt",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    fn observe_texts(dep_info: &[&str], outputs: &[(&str, &str, Option<&str>)]) -> Reads {
        let graph = graph();
        let tracked = tracked();
        let dep_info: Vec<String> = dep_info.iter().map(|t| (*t).to_owned()).collect();
        let outputs: Vec<(String, String, Option<String>)> = outputs
            .iter()
            .map(|(n, o, r)| ((*n).to_owned(), (*o).to_owned(), r.map(str::to_owned)))
            .collect();
        reads(
            &Observed {
                checkout: "/work/tree-1",
                target: "rust/target",
                workspace: "rust",
                graph: &graph,
                tracked: &tracked,
            },
            &dep_info,
            &outputs,
        )
    }

    #[test]
    fn an_included_file_outside_the_package_is_read_by_that_package() {
        let reads = observe_texts(
            &[concat!(
                "/work/tree-1/rust/target/debug/deps/store-1a2b.d: ",
                "/work/tree-1/rust/crates/store/src/lib.rs ",
                "/work/tree-1/rust/crates/store/src/../../../../data/store.tsv\n",
                "\n/work/tree-1/rust/crates/store/src/lib.rs:\n",
                "# env-dep:CARGO_PKG_NAME=store\n",
            )],
            &[],
        );
        assert_eq!(reads.len(), 1);
        assert_eq!(reads["store"], ["data/store.tsv"]);
    }

    #[test]
    fn a_target_copied_from_another_tree_names_that_trees_root() {
        let reads = observe_texts(
            &[concat!(
                "/work/tree-9/rust/target/debug/deps/commands-77.d: ",
                "/work/tree-9/rust/crates/table/tests/commands.rs ",
                "/work/tree-9/data/commands.json ",
                "/work/tree-9/docs/my\\ file.txt ",
                "/home/u/.cargo/registry/src/serde/lib.rs\n",
            )],
            &[],
        );
        assert_eq!(reads["table"], ["data/commands.json", "docs/my file.txt"]);
    }

    #[test]
    fn a_workspace_member_names_its_files_relative_to_the_workspace() {
        let reads = observe_texts(
            &[concat!(
                "/work/tree-7/rust/target/debug/deps/store-2e47.d: ",
                "crates/store/src/lib.rs crates/store/src/../../../../data/store.tsv\n",
            )],
            &[],
        );
        assert_eq!(reads["store"], ["data/store.tsv"]);
    }

    #[test]
    fn untracked_files_and_units_of_no_package_are_left_out() {
        let reads = observe_texts(
            &[
                concat!(
                    "/work/tree-1/rust/target/debug/deps/store-1.d: ",
                    "/work/tree-1/rust/crates/store/src/lib.rs ",
                    "/work/tree-1/rust/target/debug/build/store-2/out/table.rs ",
                    "/work/tree-1/data/untracked.json\n",
                ),
                concat!(
                    "/work/tree-1/rust/target/debug/deps/serde-3.d: ",
                    "/home/u/.cargo/registry/src/serde/lib.rs ",
                    "/work/tree-1/data/store.tsv\n",
                ),
            ],
            &[],
        );
        assert!(reads.is_empty(), "{reads:?}");
    }

    #[test]
    fn a_build_script_watches_files_and_directories_relative_to_its_package() {
        let reads = observe_texts(
            &[],
            &[
                (
                    "cards-0f3e",
                    concat!(
                        "cargo:rerun-if-changed=../../../data/store.tsv\n",
                        "cargo::rerun-if-changed=/work/tree-9/docs/decks\n",
                        "cargo:rerun-if-changed=build.rs\n",
                        "cargo:rustc-cfg=fast\n",
                    ),
                    Some("/work/tree-9/rust/target/debug/build/cards-0f3e/out\n"),
                ),
                ("unknown-0f3e", "cargo:rerun-if-changed=../data\n", None),
                ("cards-notahash", "cargo:rerun-if-changed=../data\n", None),
            ],
        );
        assert_eq!(reads.len(), 1);
        assert_eq!(reads["cards"], ["data/store.tsv", "docs/decks"]);
    }

    #[test]
    fn paths_for_some_packages_join_their_lists() {
        let mut reads = Reads::new();
        reads.insert("a".into(), vec!["x".into(), "y".into()]);
        reads.insert("b".into(), vec!["y".into(), "z".into()]);
        reads.insert("c".into(), vec!["w".into()]);
        let wanted = ["a".to_owned(), "b".to_owned(), "gone".to_owned()];
        assert_eq!(paths_for(&reads, &wanted), ["x", "y", "z"]);
        assert_eq!(every_path(&reads), ["w", "x", "y", "z"]);
    }

    #[test]
    fn observe_reads_the_files_under_each_profile() {
        let dir = std::env::temp_dir().join(format!("ignite-observed-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let deps = dir.join("rust/target/debug/deps");
        let build = dir.join("rust/target/debug/build/cards-0f3e");
        fs::create_dir_all(&deps).unwrap();
        fs::create_dir_all(&build).unwrap();
        let root = dir.to_str().unwrap();
        fs::write(
            deps.join("store-1.d"),
            format!("{root}/rust/target/debug/deps/store-1: {root}/rust/crates/store/src/lib.rs {root}/data/store.tsv\n"),
        )
        .unwrap();
        fs::write(deps.join("libstore-1.rlib"), "not dep-info").unwrap();
        fs::write(
            build.join("output"),
            "cargo:rerun-if-changed=../../../docs/decks\n",
        )
        .unwrap();
        let reads = observe(&dir, "rust/target", &graph(), &tracked()).unwrap();
        assert_eq!(reads["store"], ["data/store.tsv"]);
        assert_eq!(reads["cards"], ["docs/decks"]);
        assert_eq!(
            observe(&dir, "rust/missing", &graph(), &tracked()),
            Some(Reads::new())
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
