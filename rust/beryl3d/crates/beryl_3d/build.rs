//! Compiles `tests/c/header.c`, the C program that tests `include/beryl3d.h`, and links it into
//! the test targets only, so that neither the phone libraries nor the desktop binary contain
//! it. A phone build compiles nothing: its tests do not run.
//!
//! It also sets `BERYL_COMMIT`, the commit the binary is built from, which a bug report records:
//! a bundled app runs from no checkout, so it cannot ask git when it runs. The variable is set
//! for this package only, so a new commit rebuilds `beryl_3d` but not the scene or inspect
//! crates it depends on.

use std::path::Path;
use std::process::Command;

fn main() {
    commit();
    println!("cargo:rerun-if-changed=tests/c");
    println!("cargo:rerun-if-changed=include/beryl3d.h");
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
