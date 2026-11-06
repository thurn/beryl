//! The `beryl_cli` binary under test, found at run time.
//!
//! Never `env!("CARGO_BIN_EXE_beryl_cli")`: that absolute path is fixed when the test is
//! compiled. Cargo fingerprints workspace members by workspace-relative path, so a
//! `rust/target` cloned into another worktree (`tools/src/beryl_tools/warm.py`) keeps test
//! binaries it counts as fresh, which would spawn the old worktree's binary or none at all.

use std::path::PathBuf;

/// `beryl_cli` in this test's profile directory, one up from the test's own `deps/`.
pub fn beryl_cli() -> PathBuf {
    let me = std::env::current_exe().expect("the test binary");
    let bin = me
        .parent()
        .and_then(|deps| deps.parent())
        .expect("target/<profile>/deps")
        .join("beryl_cli");
    assert!(bin.is_file(), "no beryl_cli beside {}", me.display());
    bin
}
