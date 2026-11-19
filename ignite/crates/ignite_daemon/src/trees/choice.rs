//! Which build directories a ticket will want, from the files it names.
//!
//! A tree keeps each build product's output directories between tickets (`TreeConfig::keep`).
//! A ticket that names a file under the directory holding one of them, such as a file of a Cargo
//! workspace for that workspace's `target`, will build there. A tree that lacks that directory
//! builds it from nothing: a ticket on the second Cargo workspace was given the tree nearest the
//! tip, which had never built that workspace, and its first build compiled 77 crates for ten
//! minutes while six other free trees held a build of it.
//!
//! What a tree keeps and prebuilds comes from the manifests' products, and changes when the daemon
//! reads changed manifests ([`TreeConfig::reload`]).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use ignite_core::manifest::{InputPattern, Manifest};

use super::TreeConfig;

impl TreeConfig {
    /// What a reset keeps and which products are prebuilt, from the products of `manifest`.
    pub(super) fn products(manifest: &Manifest) -> (Vec<String>, Vec<String>) {
        let keep: BTreeSet<String> = manifest
            .products
            .values()
            .flat_map(|product| product.outputs.iter())
            .map(|dir| dir.trim_matches('/').to_owned())
            .collect();
        let prebuild = manifest
            .products
            .iter()
            .filter(|(_, product)| {
                product.inputs.iter().any(|input| {
                    matches!(input, InputPattern::Crate(_) | InputPattern::CrateClosure)
                })
            })
            .map(|(name, _)| name.clone())
            .collect();
        (keep.into_iter().collect(), prebuild)
    }

    /// Takes what a reset keeps and which products are prebuilt from `manifest`, for every clone.
    pub fn reload(&self, manifest: &Manifest) {
        let (keep, prebuild) = Self::products(manifest);
        self.keep.set(keep);
        self.prebuild.set(prebuild);
    }
}

/// The directories of `keep` that `files` lie beside: those whose parent directory holds one of
/// the files and which no file lies inside. A directory at the root of the checkout is beside
/// every file.
pub(super) fn wanted<'a>(keep: &'a [String], files: &[String]) -> Vec<&'a str> {
    keep.iter()
        .map(String::as_str)
        .filter(|dir| {
            let parent = dir.rsplit_once('/').map_or("", |(parent, _)| parent);
            files
                .iter()
                .any(|file| within(file, parent) && !within(file, dir))
        })
        .collect()
}

/// How many of `wanted` are missing or empty in the tree at `root`.
pub(super) fn missing(root: &Path, wanted: &[&str]) -> usize {
    wanted
        .iter()
        .filter(|dir| {
            fs::read_dir(root.join(dir)).map_or(true, |mut entries| entries.next().is_none())
        })
        .count()
}

pub(super) fn within(path: &str, dir: &str) -> bool {
    dir.is_empty()
        || path
            .strip_prefix(dir)
            .is_some_and(|rest| rest.starts_with('/'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keep() -> Vec<String> {
        [
            "rust/target",
            "rust/second/target",
            "rust/target/lint",
            "ui/node_modules",
        ]
        .map(String::from)
        .to_vec()
    }

    #[test]
    fn a_file_of_the_second_workspace_wants_its_target_and_the_outer_one() {
        let files = ["rust/second/crates/scene/src/lib.rs".to_owned()];
        assert_eq!(
            wanted(&keep(), &files),
            ["rust/target", "rust/second/target"]
        );
    }

    #[test]
    fn files_elsewhere_want_only_the_directories_beside_them() {
        let files = ["ui/src/app.ts".to_owned(), "docs/notes.md".to_owned()];
        assert_eq!(wanted(&keep(), &files), ["ui/node_modules"]);
        assert!(wanted(&keep(), &[]).is_empty());
    }

    #[test]
    fn an_empty_directory_counts_as_missing() {
        let root = std::env::temp_dir().join(format!("ignite-choice-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("rust/target/debug")).unwrap();
        fs::create_dir_all(root.join("rust/second/target")).unwrap();
        let wanted = ["rust/target", "rust/second/target", "ui/node_modules"];
        assert_eq!(missing(&root, &wanted), 2);
        let _ = fs::remove_dir_all(&root);
    }
}
