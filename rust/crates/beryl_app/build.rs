//! Tauri's build step, and the commit the app is built from (`BERYL_COMMIT`), which a bug
//! report records: a bundled app runs from no checkout, so it cannot ask git when it runs.

use std::path::Path;
use std::process::Command;

fn main() {
    // macOS's 8 MiB main-thread stack on Windows, whose linker defaults to 1 MiB.
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/STACK:8388608");
    }
    commit();
    tauri_build::build();
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
