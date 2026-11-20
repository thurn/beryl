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
//!
//! A lease prebuilds the products beside the ticket's files first ([`TreeConfig::prebuild_order`]).
//! The products were built in name order, so on 2026-11-19 a ticket that changed only the second
//! workspace had its tree build three products of the first, two of them optimized, before its
//! own workspace's tests. Its landing, a few minutes later, ran its express commands for 165 s
//! on 2 cores; with that workspace's test build current, the same commands took 32 to 37 s
//! after a one-file edit under similar load.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

use ignite_core::manifest::{InputPattern, Manifest};

use super::TreeConfig;

/// What [`TreeConfig::products`] finds: the directories a reset keeps, the products prebuilt
/// and each prebuilt product's output directories.
type Products = (Vec<String>, Vec<String>, BTreeMap<String, Vec<String>>);

impl TreeConfig {
    /// What a reset keeps and which products are prebuilt, from the products of `manifest`.
    pub(super) fn products(manifest: &Manifest) -> Products {
        let keep: BTreeSet<String> = manifest
            .products
            .values()
            .flat_map(|product| product.outputs.iter())
            .map(|dir| dir.trim_matches('/').to_owned())
            .collect();
        let outputs: BTreeMap<String, Vec<String>> = manifest
            .products
            .iter()
            .filter(|(_, product)| {
                product.inputs.iter().any(|input| {
                    matches!(input, InputPattern::Crate(_) | InputPattern::CrateClosure)
                })
            })
            .map(|(name, product)| {
                let dirs = product.outputs.iter();
                (
                    name.clone(),
                    dirs.map(|d| d.trim_matches('/').to_owned()).collect(),
                )
            })
            .collect();
        let prebuild = outputs.keys().cloned().collect();
        (keep.into_iter().collect(), prebuild, outputs)
    }

    /// Takes what a reset keeps and which products are prebuilt from `manifest`, for every clone.
    pub fn reload(&self, manifest: &Manifest) {
        let (keep, prebuild, outputs) = Self::products(manifest);
        self.keep.set(keep);
        self.prebuild.set(prebuild);
        self.prebuild_outputs.set(outputs);
    }

    /// The prebuild products in the order a lease for a ticket naming `files` builds them: first
    /// those that write a directory beside the files ([`wanted`]), then the rest, each group in
    /// name order.
    pub fn prebuild_order(&self, files: &[String]) -> Vec<String> {
        let keep = self.keep.get();
        let wanted = wanted(&keep, files);
        let outputs = self.prebuild_outputs.get();
        let beside = |product: &String| {
            outputs
                .get(product)
                .is_some_and(|dirs| dirs.iter().any(|dir| wanted.contains(&dir.as_str())))
        };
        let (first, rest): (Vec<String>, Vec<String>) =
            self.prebuild.get().iter().cloned().partition(beside);
        first.into_iter().chain(rest).collect()
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
    fn a_lease_prebuilds_the_products_beside_the_tickets_files_first() {
        let config = config(&[
            ("cli", "rust/target"),
            ("fuzz", "rust/target"),
            ("tool-tests", "tool/target"),
        ]);
        let tool = ["tool/crates/daemon/src/lib.rs".to_owned()];
        assert_eq!(config.prebuild_order(&tool), ["tool-tests", "cli", "fuzz"]);
        let game = ["rust/crates/kernel/src/lib.rs".to_owned()];
        assert_eq!(config.prebuild_order(&game), ["cli", "fuzz", "tool-tests"]);
        assert_eq!(config.prebuild_order(&[]), ["cli", "fuzz", "tool-tests"]);
    }

    /// A tree configuration whose prebuild products write the directories given.
    fn config(products: &[(&str, &str)]) -> TreeConfig {
        let outputs: BTreeMap<String, Vec<String>> = products
            .iter()
            .map(|(name, dir)| ((*name).to_owned(), vec![(*dir).to_owned()]))
            .collect();
        let keep: BTreeSet<String> = products.iter().map(|(_, dir)| (*dir).to_owned()).collect();
        TreeConfig {
            repo: "repo".into(),
            dir: "repo/trees".into(),
            remote: "origin".to_owned(),
            branches: Vec::new(),
            ticket_trees: 1,
            landing_trees: 0,
            disk_limit: None,
            keep: keep.into_iter().collect::<Vec<_>>().into(),
            prebuild: outputs.keys().cloned().collect::<Vec<_>>().into(),
            prebuild_outputs: outputs.into(),
        }
    }

    #[test]
    fn an_empty_directory_counts_as_missing() {
        let root = ignite_core::test_files::temp_dir().join(format!("ignite-choice-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("rust/target/debug")).unwrap();
        fs::create_dir_all(root.join("rust/second/target")).unwrap();
        let wanted = ["rust/target", "rust/second/target", "ui/node_modules"];
        assert_eq!(missing(&root, &wanted), 2);
        let _ = fs::remove_dir_all(&root);
    }
}
