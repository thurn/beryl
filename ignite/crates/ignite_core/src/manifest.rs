//! Check manifests: the check families and build products a repository declares.
//!
//! Each file in `repository.checks` declares families (groups of checks run by one command)
//! and products (builds those checks need). The file's stem is the family's group.
//! This module parses text and reports errors with the file and key.

use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::Os;

/// Whether a family's checks verify commits or search for new bugs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    /// Gives the same answer for the same tree; a failure blames a commit and may block landing.
    Regression,
    /// Searches at random; a failure is a newly found bug and never blocks anything.
    Exploration,
}

/// How a family's results are read.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Adapter {
    /// cargo-nextest's own machine-readable output.
    Nextest,
    /// One JSON line per check, written to the file named by `IGNITE_RESULTS`.
    Jsonl,
}

/// Where a check must pass.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Platforms {
    /// `"any"`: one pass on any operating system is enough.
    Any(AnyPlatform),
    /// A pass is needed on each operating system named.
    Each(Vec<Os>),
}

/// The literal string `any`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AnyPlatform {
    Any,
}

impl Platforms {
    /// Whether the family runs on this operating system.
    pub fn supports(&self, os: Os) -> bool {
        match self {
            Self::Any(_) => true,
            Self::Each(systems) => systems.contains(&os),
        }
    }

    /// Whether one pass on any operating system is enough.
    pub fn is_any(&self) -> bool {
        matches!(self, Platforms::Any(_))
    }
}

/// One entry of an `inputs` list.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(from = "String", into = "String")]
pub enum InputPattern {
    /// A file, a directory or a glob, relative to the checkout's root.
    Path(String),
    /// `crate:<package>`: the package with every workspace package it depends on.
    Crate(String),
    /// `crate-closure`: the same, computed per test from the package that holds it.
    CrateClosure,
}

impl From<String> for InputPattern {
    fn from(text: String) -> Self {
        if text == "crate-closure" {
            InputPattern::CrateClosure
        } else if let Some(package) = text.strip_prefix("crate:") {
            InputPattern::Crate(package.to_owned())
        } else {
            InputPattern::Path(text)
        }
    }
}

impl From<InputPattern> for String {
    fn from(pattern: InputPattern) -> String {
        match pattern {
            InputPattern::Path(path) => path,
            InputPattern::Crate(package) => format!("crate:{package}"),
            InputPattern::CrateClosure => "crate-closure".to_owned(),
        }
    }
}

/// A `[family.<name>]` table.
// Each flag is a separate key a manifest sets on its own; no two of them form a state.
#[allow(clippy::struct_excessive_bools)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Family {
    pub kind: Kind,
    pub adapter: Adapter,
    pub platforms: Platforms,
    /// Platforms that verify this family after master moves.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub defer_platforms: Vec<Os>,
    /// A JSONL regression check's digest identifies the current calibration. Results are
    /// scoped to the registered verifier, and each required OS supplies its own listing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub machine_relative: bool,
    /// Build products that must be current before the checks run.
    #[serde(default)]
    pub needs: Vec<String>,
    /// The products that must be current before `list` runs, when that is fewer than `needs`
    /// (a `list` that reads no build output). Without it, `list` waits for all of `needs`.
    #[serde(default)]
    pub list_needs: Option<Vec<String>>,
    /// Shared resources the checks hold while they run.
    #[serde(default)]
    pub resources: Vec<String>,
    /// Let `list` run without these resources when it only reads the checkout or build outputs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub list_without_resources: bool,
    /// The `run` command rereads the file `IGNITE_CORES_FILE` names while it runs and starts
    /// more threads as its grant grows, so the scheduler gives it cores as they free up.
    #[serde(default)]
    pub grows: bool,
    /// The cores a `run` command asks for, when fewer than the machine's usual share would be
    /// used: a family whose work runs elsewhere, such as in a virtual machine, and whose own
    /// process only waits for it.
    #[serde(default)]
    pub cores: Option<u32>,
    /// Prints the family's checks, each with any inputs of its own.
    pub list: Option<String>,
    /// Runs the checks named in `{checks}`.
    pub run: Option<String>,
    /// An exploration family's source of new checks: prints, in the format of `list`, checks
    /// that this command has never printed before, such as scenarios from unplayed seeds.
    pub next: Option<String>,
    pub inputs: Vec<InputPattern>,
    /// Repository paths or globs that require this family for ordinary ticket dispatch.
    /// When omitted, dispatch uses the path patterns from `inputs`, `package_inputs`, and
    /// `digest_inputs`. An empty list opts out of path-based dispatch for this family.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatch_paths: Option<Vec<String>>,
    /// Extra inputs of one package's checks, by package name: what that package's tests read
    /// beyond `inputs`, such as the card files a code generator's tests walk. Only the checks
    /// that package holds hash them, so the family's other checks keep their cached results
    /// when these files change.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub package_inputs: BTreeMap<String, Vec<InputPattern>>,
    /// Files beyond the family and product inputs that fully determine an ordinary listing's
    /// digests. Declaring them lets Ignite cache the listing and use it to scope the gate.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub digest_inputs: Vec<InputPattern>,
    /// Blocks every landing its inputs touch.
    #[serde(default)]
    pub always: bool,
    /// Path patterns that narrow `always`. On changes that match none of the patterns, the
    /// gate scores these checks like any others.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub always_when: Vec<String>,
    /// The family's checks fail when the code does not compile or type-check. A tip round runs
    /// these build checks first, moves the green branch once they pass, and may revert the
    /// landing that made one of them fail.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub builds: bool,
    /// What the family's `list` prints cannot depend on the comments in a Rust file, as with
    /// test names or what a compiled program does. Its listing hash and the hash of its
    /// listing's sources then cover each `.rs` file by its code without comments, so a change
    /// that edits only comments keeps its kept listing and builds nothing to list. The checks'
    /// own hashes still cover every file whole.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub listing_ignores_comments: bool,
    /// The family's `run` command checks the whole family or workspace regardless of how many
    /// `{checks}` are named (such as `cargo clippy --workspace`), so splitting its checks into
    /// multiple tip batches repeats the entire command once per batch without saving work.
    /// Tip verification runs all of the family's checks in one claimable batch (`0`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub single_batch: bool,
}

impl Family {
    /// Whether the family is in the always set for a change to `changed`: `always`, narrowed by
    /// `always_when` when that names any paths.
    pub fn always_for(&self, changed: &[String]) -> bool {
        self.always
            && (self.always_when.is_empty()
                || changed.iter().any(|path| {
                    self.always_when
                        .iter()
                        .any(|pattern| crate::inputs::path_matches(pattern, path))
                }))
    }

    /// The family's extra inputs for the checks of `package`, from `package_inputs`; none for a
    /// check that belongs to no package.
    pub fn package_patterns(&self, package: Option<&str>) -> &[InputPattern] {
        package
            .and_then(|name| self.package_inputs.get(name))
            .map_or(&[], Vec::as_slice)
    }
}

/// A `[product.<name>]` table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Product {
    /// The build command; `{packages}` stands for the packages the caller needs.
    pub build: String,
    /// The directory the command runs in, relative to the checkout's root.
    pub dir: Option<String>,
    pub inputs: Vec<InputPattern>,
    /// The directories the command writes, which a build tree keeps between tickets.
    pub outputs: Vec<String>,
    /// A product with only path inputs that still writes compiled build outputs.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub compiles: bool,
    /// The build command rereads the file `IGNITE_CORES_FILE` names while it runs and starts
    /// more threads as its grant grows, as `grows` on a family.
    #[serde(default)]
    pub grows: bool,
    /// Workspace packages a build of the whole workspace leaves out: with no packages named,
    /// `{packages}` stands for `--workspace --exclude <package>` for each. Products and families
    /// that select the same set of packages build with the same Cargo features, so they reuse
    /// each other's compiled crates instead of rebuilding them.
    #[serde(default)]
    pub exclude: Vec<String>,
}

impl Product {
    pub fn uses_build_slot(&self) -> bool {
        !self.outputs.is_empty()
            && (self.compiles
                || self.inputs.iter().any(|input| {
                    matches!(input, InputPattern::Crate(_) | InputPattern::CrateClosure)
                }))
    }
}

/// An `[[rebuild]]` entry of `ignite.toml`: an edit whose rebuild cost is measured.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RebuildEdit {
    pub name: String,
    /// What the edit does, in words, for people reading the counts.
    pub edit: String,
    /// The file the edit changes, relative to the checkout's root.
    pub file: String,
    /// The text the edit appends to `file`, such as a no-op item. The edit is undone by writing
    /// the file's old contents back.
    pub append: String,
    /// The build products whose rebuild is counted.
    pub products: Vec<String>,
}

/// A family with the group (file stem) that declared it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeclaredFamily {
    pub group: String,
    pub family: Family,
}

/// Every family and product from the manifest directory.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Manifest {
    pub families: BTreeMap<String, DeclaredFamily>,
    pub products: BTreeMap<String, Product>,
    /// Each family's `covered_by`, if set. Tip verification runs the covering family's checks,
    /// which detect every fault the covered family's checks detect.
    pub covered_by: BTreeMap<String, String>,
}

/// A manifest that cannot be used.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ManifestError {
    pub file: PathBuf,
    pub key: String,
    pub message: String,
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.key.is_empty() {
            write!(f, "{}: {}", self.file.display(), self.message)
        } else {
            write!(f, "{}: {}: {}", self.file.display(), self.key, self.message)
        }
    }
}

impl std::error::Error for ManifestError {}

/// The `covered_by` field of each family table, read in a second pass over the same text.
#[derive(Deserialize)]
struct CoverFile {
    #[serde(default)]
    family: BTreeMap<String, CoverTable>,
}

#[derive(Deserialize)]
struct CoverTable {
    covered_by: Option<String>,
}

/// The field name of [`Manifest::covered_by`] in a family table.
const COVERED_BY: &str = "covered_by";

#[derive(Deserialize)]
struct ManifestFile {
    #[serde(default)]
    family: BTreeMap<String, Family>,
    #[serde(default)]
    product: BTreeMap<String, Product>,
}

impl Manifest {
    /// Parses every manifest file, given as (path, text), and checks them against each other.
    pub fn from_files(files: &[(PathBuf, String)]) -> Result<Manifest, ManifestError> {
        Self::from_files_with_unknown(files).map(|(manifest, _)| manifest)
    }

    /// Parses like [`Manifest::from_files`] and also returns each key no family or product
    /// declares, as `file: key`, so that the caller can warn about it. As in `ignite.toml`,
    /// an unknown key is not an error, so that a key added to a manifest never stops a
    /// daemon built before it.
    pub fn from_files_with_unknown(
        files: &[(PathBuf, String)],
    ) -> Result<(Manifest, Vec<String>), ManifestError> {
        let mut unknown = Vec::new();
        let mut manifest = Manifest::default();
        let mut origin: BTreeMap<String, &Path> = BTreeMap::new();
        for (path, text) in files {
            let error = |key: String, message: String| ManifestError {
                file: path.clone(),
                key,
                message,
            };
            let (parsed, ignored): (ManifestFile, _) =
                crate::config::parse_lenient(text).map_err(|e| error(String::new(), e))?;
            unknown.extend(
                ignored
                    .into_iter()
                    .filter(|key| !is_cover_key(key))
                    .map(|key| format!("{}: {key}", path.display())),
            );
            let covers: CoverFile =
                toml::from_str(text).map_err(|e| error(String::new(), e.to_string()))?;
            for (name, table) in covers.family {
                if let Some(covering) = table.covered_by {
                    manifest.covered_by.insert(name, covering);
                }
            }
            let group = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            for (name, family) in parsed.family {
                let key = format!("family.{name}");
                check_family(&family)
                    .map_err(|(field, message)| error(format!("{key}.{field}"), message))?;
                if let Some(first) = origin.insert(key.clone(), path) {
                    return Err(error(
                        key,
                        format!("already declared in {}", first.display()),
                    ));
                }
                manifest.families.insert(
                    name,
                    DeclaredFamily {
                        group: group.clone(),
                        family,
                    },
                );
            }
            for (name, product) in parsed.product {
                let key = format!("product.{name}");
                if product.build.trim().is_empty() {
                    return Err(error(
                        format!("{key}.build"),
                        "must not be empty".to_owned(),
                    ));
                }
                if product.outputs.is_empty() {
                    return Err(error(
                        format!("{key}.outputs"),
                        "names no directory".to_owned(),
                    ));
                }
                if let Some(first) = origin.insert(key.clone(), path) {
                    return Err(error(
                        key,
                        format!("already declared in {}", first.display()),
                    ));
                }
                manifest.products.insert(name, product);
            }
        }
        for (name, declared) in &manifest.families {
            let list_needs = declared.family.list_needs.iter().flatten();
            let named = declared
                .family
                .needs
                .iter()
                .map(|p| ("needs", p))
                .chain(list_needs.map(|p| ("list_needs", p)));
            for (index, (field, product)) in named.enumerate() {
                if !manifest.products.contains_key(product) {
                    return Err(ManifestError {
                        file: origin[&format!("family.{name}")].to_path_buf(),
                        key: format!("family.{name}.{field}[{index}]"),
                        message: format!("no product named {product:?} is declared"),
                    });
                }
            }
        }
        check_covers(&manifest).map_err(|(name, message)| ManifestError {
            file: origin[&format!("family.{name}")].to_path_buf(),
            key: format!("family.{name}.{COVERED_BY}"),
            message,
        })?;
        Ok((manifest, unknown))
    }
}

/// Whether `key`, a dotted path the family tables did not declare, is a family's `covered_by`,
/// which the second pass reads.
fn is_cover_key(key: &str) -> bool {
    key.strip_prefix("family.")
        .and_then(|rest| rest.rsplit_once('.'))
        .is_some_and(|(_, field)| field == COVERED_BY)
}

/// Every `covered_by` names a declared family, and no family covers itself, directly or
/// through a chain. Returns the family at fault and what is wrong.
fn check_covers(manifest: &Manifest) -> Result<(), (String, String)> {
    for (name, covering) in &manifest.covered_by {
        if !manifest.families.contains_key(covering) {
            return Err((
                name.clone(),
                format!("no family named {covering:?} is declared"),
            ));
        }
        let mut at = covering;
        for _ in 0..=manifest.covered_by.len() {
            if at == name {
                return Err((name.clone(), "the family covers itself".to_owned()));
            }
            match manifest.covered_by.get(at) {
                Some(next) => at = next,
                None => break,
            }
        }
    }
    Ok(())
}

/// The field at fault, after `family.<name>.`, and what is wrong with it.
fn check_family(family: &Family) -> Result<(), (String, String)> {
    let fail = |field: &str, message: &str| Err((field.to_owned(), message.to_owned()));
    if family.machine_relative
        && (family.adapter != Adapter::Jsonl
            || family.kind != Kind::Regression
            || family.platforms.is_any())
    {
        return fail(
            "machine_relative",
            "requires a JSONL regression family with named platforms",
        );
    }
    if let Platforms::Each(list) = &family.platforms
        && list.is_empty()
    {
        return fail("platforms", "names no operating system");
    }
    if !family.defer_platforms.is_empty()
        && !matches!(&family.platforms, Platforms::Each(platforms)
            if family.defer_platforms.iter().all(|os| platforms.contains(os)))
    {
        return fail("defer_platforms", "must be a subset of the named platforms");
    }
    if family.inputs.is_empty() {
        return fail("inputs", "must name what the checks read");
    }
    for (package, patterns) in &family.package_inputs {
        let field = format!("package_inputs.{package}");
        if patterns.is_empty() {
            return fail(&field, "must name what the package's checks read");
        }
        // `crate-closure` already starts from the package that holds each check, so it belongs
        // in `inputs`, where it covers every package.
        if patterns.contains(&InputPattern::CrateClosure) {
            return fail(&field, "crate-closure belongs in the family's inputs");
        }
    }
    if family.digest_inputs.contains(&InputPattern::CrateClosure) {
        return fail(
            "digest_inputs",
            "crate-closure belongs in the family's inputs",
        );
    }
    match (family.kind, &family.next) {
        (Kind::Exploration, None) => {
            return fail("next", "is required for an exploration family");
        }
        (Kind::Regression, Some(_)) => {
            return fail("next", "is only for an exploration family");
        }
        _ => {}
    }
    if family.adapter == Adapter::Jsonl {
        if family.list.is_none() {
            return fail("list", "is required for the jsonl adapter");
        }
        if family.run.is_none() {
            return fail("run", "is required for the jsonl adapter");
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
