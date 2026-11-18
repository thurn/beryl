//! Check manifests: the check families and build products a repository declares.
//!
//! Each file in the manifest directory (`repository.checks` in `ignite.toml`) holds
//! `[family.<name>]` and `[product.<name>]` tables. A family is a group of checks that one
//! command runs; a build product is a named build that families need, with the command that
//! makes it, what it reads and the directories it writes. The file's stem is the family's group.
//!
//! This module parses text the caller has read; it does no I/O. Errors name the file and the
//! key at fault.

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
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    pub kind: Kind,
    pub adapter: Adapter,
    pub platforms: Platforms,
    /// Build products that must be current before the checks run.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Shared resources the checks hold while they run.
    #[serde(default)]
    pub resources: Vec<String>,
    /// Prints the family's checks, each with any inputs of its own.
    pub list: Option<String>,
    /// Runs the checks named in `{checks}`.
    pub run: Option<String>,
    pub inputs: Vec<InputPattern>,
    /// Blocks every landing its inputs touch.
    #[serde(default)]
    pub always: bool,
}

/// A `[product.<name>]` table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Product {
    /// The build command; `{packages}` stands for the packages the caller needs.
    pub build: String,
    /// The directory the command runs in, relative to the checkout's root.
    pub dir: Option<String>,
    pub inputs: Vec<InputPattern>,
    /// The directories the command writes, which a build tree keeps between tickets.
    pub outputs: Vec<String>,
}

/// An `[[rebuild]]` entry of `ignite.toml`: an edit whose rebuild cost is measured.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RebuildEdit {
    pub name: String,
    /// What the edit does, in words; the idle work that makes it reads this.
    pub edit: String,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestFile {
    #[serde(default)]
    family: BTreeMap<String, Family>,
    #[serde(default)]
    product: BTreeMap<String, Product>,
}

impl Manifest {
    /// Parses every manifest file, given as (path, text), and checks them against each other.
    pub fn from_files(files: &[(PathBuf, String)]) -> Result<Manifest, ManifestError> {
        let mut manifest = Manifest::default();
        let mut origin: BTreeMap<String, &Path> = BTreeMap::new();
        for (path, text) in files {
            let error = |key: String, message: String| ManifestError {
                file: path.clone(),
                key,
                message,
            };
            let parsed: ManifestFile = toml::from_str(text)
                .map_err(|e| error(String::new(), e.to_string().trim_end().to_owned()))?;
            let group = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_owned();
            for (name, family) in parsed.family {
                let key = format!("family.{name}");
                check_family(&family).map_err(|(field, message)| {
                    error(format!("{key}.{field}"), message.to_owned())
                })?;
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
            for (index, product) in declared.family.needs.iter().enumerate() {
                if !manifest.products.contains_key(product) {
                    return Err(ManifestError {
                        file: origin[&format!("family.{name}")].to_path_buf(),
                        key: format!("family.{name}.needs[{index}]"),
                        message: format!("no product named {product:?} is declared"),
                    });
                }
            }
        }
        Ok(manifest)
    }
}

fn check_family(family: &Family) -> Result<(), (&'static str, &'static str)> {
    if let Platforms::Each(list) = &family.platforms
        && list.is_empty()
    {
        return Err(("platforms", "names no operating system"));
    }
    if family.inputs.is_empty() {
        return Err(("inputs", "must name what the checks read"));
    }
    if family.adapter == Adapter::Jsonl {
        if family.list.is_none() {
            return Err(("list", "is required for the jsonl adapter"));
        }
        if family.run.is_none() {
            return Err(("run", "is required for the jsonl adapter"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const RUST: &str = r#"
[family.rust]
kind = "regression"
adapter = "nextest"
platforms = "any"
needs = ["rust-tests"]
inputs = ["crate-closure", "rust/.config/nextest.toml"]

[product.rust-tests]
build = "cargo nextest run --no-run {packages}"
dir = "rust"
inputs = ["crate-closure"]
outputs = ["rust/target"]
"#;

    const SHOTS: &str = r#"
[family.shots]
kind = "regression"
adapter = "jsonl"
platforms = ["macos"]
needs = ["cli", "ui-deps"]
resources = ["browser-pages"]
list = "node ui/scripts/capture.mjs --list"
run = "node ui/scripts/capture.mjs {checks}"
inputs = ["ui/src/**", "ui/shots.json", "crate:game_cli"]

[product.cli]
build = "cargo build -p game_cli"
inputs = ["crate:game_cli"]
outputs = ["rust/target"]

[product.ui-deps]
build = "npm ci"
dir = "ui"
inputs = ["ui/package-lock.json"]
outputs = ["ui/node_modules"]
"#;

    fn files(list: &[(&str, &str)]) -> Vec<(PathBuf, String)> {
        list.iter()
            .map(|(p, t)| (PathBuf::from(p), (*t).to_owned()))
            .collect()
    }

    #[test]
    fn parses_the_document_examples() {
        let manifest = Manifest::from_files(&files(&[
            ("checks/rust.toml", RUST),
            ("checks/ui.toml", SHOTS),
        ]))
        .unwrap();
        let rust = &manifest.families["rust"];
        assert_eq!(rust.group, "rust");
        assert!(rust.family.platforms.is_any());
        assert_eq!(rust.family.inputs[0], InputPattern::CrateClosure);
        let shots = &manifest.families["shots"].family;
        assert_eq!(shots.platforms, Platforms::Each(vec![Os::Macos]));
        assert_eq!(shots.inputs[2], InputPattern::Crate("game_cli".to_owned()));
        assert_eq!(manifest.products["rust-tests"].outputs, ["rust/target"]);
    }

    #[test]
    fn a_missing_product_names_the_file_and_key() {
        let error = Manifest::from_files(&files(&[(
            "checks/rust.toml",
            &RUST[..RUST.find("[product").unwrap()],
        )]))
        .unwrap_err();
        assert_eq!(error.key, "family.rust.needs[0]");
        assert_eq!(error.file, PathBuf::from("checks/rust.toml"));
    }

    #[test]
    fn a_duplicate_family_names_both_files() {
        let error =
            Manifest::from_files(&files(&[("checks/a.toml", RUST), ("checks/b.toml", RUST)]))
                .unwrap_err();
        assert_eq!(error.file, PathBuf::from("checks/b.toml"));
        assert_eq!(error.key, "family.rust");
        assert!(error.message.contains("checks/a.toml"));
    }

    #[test]
    fn field_errors_name_the_key() {
        let text = SHOTS.replace("list = \"node ui/scripts/capture.mjs --list\"\n", "");
        let error = Manifest::from_files(&files(&[("checks/ui.toml", &text)])).unwrap_err();
        assert_eq!(
            error.to_string(),
            "checks/ui.toml: family.shots.list: is required for the jsonl adapter"
        );
        let text = RUST.replace("platforms = \"any\"", "platforms = \"anywhere\"");
        let error = Manifest::from_files(&files(&[("checks/rust.toml", &text)])).unwrap_err();
        assert!(error.message.contains("platforms"), "{error}");
        let text = RUST.replace("kind = ", "knid = ");
        assert!(Manifest::from_files(&files(&[("checks/rust.toml", &text)])).is_err());
    }

    #[test]
    fn input_patterns_round_trip() {
        for text in ["crate-closure", "crate:core", "ui/src/**"] {
            let pattern = InputPattern::from(text.to_owned());
            assert_eq!(String::from(pattern), text);
        }
    }
}
