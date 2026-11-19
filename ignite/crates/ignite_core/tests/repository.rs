//! Checks over Ignite's own source tree and the manifests it ships with.

use std::fs;
use std::path::{Path, PathBuf};

use ignite_core::manifest::Manifest;

fn workspace() -> PathBuf {
    ignite_core::test_files::root().unwrap().join("ignite")
}

fn files_under(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files_under(&path, out);
        } else {
            out.push(path);
        }
    }
}

/// Ignite's code names nothing of the repository it serves: everything specific to it comes
/// from the configuration and the manifests. The name is assembled here so this file passes.
#[test]
fn no_source_file_names_the_repository() {
    let name = ["ber", "yl"].concat();
    let mut files = Vec::new();
    // Opening the folder through the helper makes its whole tree an input of this test.
    ignite_core::test_files::read_dir("ignite/crates").unwrap();
    files_under(&workspace().join("crates"), &mut files);
    let offenders: Vec<_> = files
        .iter()
        .filter(|path| {
            fs::read(path).is_ok_and(|bytes| {
                String::from_utf8_lossy(&bytes)
                    .to_lowercase()
                    .contains(&name)
            })
        })
        .collect();
    assert!(
        offenders.is_empty(),
        "these files name the repository: {offenders:?}"
    );
}

/// Every manifest in `ignite/checks/` parses, and together they are consistent.
#[test]
fn every_check_manifest_parses() {
    let mut files = Vec::new();
    for entry in ignite_core::test_files::read_dir("ignite/checks").unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|e| e == "toml") {
            let text = fs::read_to_string(&path).unwrap();
            files.push((path, text));
        }
    }
    if let Err(error) = Manifest::from_files(&files) {
        panic!("{error}");
    }
}
