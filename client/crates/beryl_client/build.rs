//! Compiles `tests/c/header.c`, the C program that tests `include/beryl_client.h`, and links it into
//! the test targets only, so that neither the phone libraries nor the desktop binary contain
//! it. A phone build compiles nothing: its tests do not run.
//!
//! It also sets `BERYL_COMMIT`, the commit the binary is built from, which a bug report records:
//! a bundled app runs from no checkout, so it cannot ask git when it runs. The variable is set
//! for this package only, so a new commit rebuilds `beryl_client` but not the scene or inspect
//! crates it depends on.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    commit();
    embed_effects();
    println!("cargo:rerun-if-changed=tests/c");
    println!("cargo:rerun-if-changed=include/beryl_client.h");
    let target = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target == "ios" || target == "android" {
        return;
    }
    let objects = cc::Build::new()
        .file("tests/c/header.c")
        .include("include")
        .warnings(true)
        .extra_warnings(true)
        .warnings_into_errors(true)
        .cargo_metadata(false)
        .compile_intermediates();
    for object in objects {
        println!("cargo:rustc-link-arg-tests={}", object.display());
    }
}

fn git(args: &[&str]) -> Option<String> {
    Command::new("git")
        .args(args)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// Sets `BERYL_COMMIT` to `HEAD`'s hash, and reruns once `HEAD` moves: a commit, a checkout or
/// a fast-forward of the branch it is on.
fn commit() {
    let head = git(&["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".to_owned());
    println!("cargo:rustc-env=BERYL_COMMIT={head}");
    let mut watched = vec![
        "HEAD".to_owned(),
        "logs/HEAD".to_owned(),
        "packed-refs".to_owned(),
    ];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(branch);
    }
    for name in watched {
        // A missing file would rerun this every build.
        if let Some(path) = git(&["rev-parse", "--path-format=absolute", "--git-path", &name])
            && Path::new(&path).exists()
        {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}

/// Lists the files to embed in the standalone app, including every newly added definition.
fn embed_effects() {
    const DIRECTORY: &str = "../../../data/client/effects";
    println!("cargo:rerun-if-changed={DIRECTORY}");
    let mut paths: Vec<_> = std::fs::read_dir(DIRECTORY)
        .expect("effect definitions")
        .map(|entry| entry.expect("effect file").path())
        .filter(|path| {
            path.extension()
                .is_some_and(|extension| extension == "json")
        })
        .collect();
    paths.sort_unstable();
    let mut source = String::from("[\n");
    for path in paths {
        let name = path
            .file_stem()
            .and_then(|name| name.to_str())
            .expect("UTF-8 effect name");
        let file = format!("/{DIRECTORY}/{name}.json");
        writeln!(
            source,
            "({name:?}, include_str!(concat!(env!(\"CARGO_MANIFEST_DIR\"), {file:?}))),"
        )
        .expect("write embedded effect");
    }
    source.push_str("]\n");
    let output = PathBuf::from(std::env::var_os("OUT_DIR").expect("Cargo output"));
    std::fs::write(output.join("effects.rs"), source).expect("write effect file list");
}
