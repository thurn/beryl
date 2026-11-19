//! Which changes to Ignite's source change what the running daemon does.
//!
//! A release is built whenever a file under Ignite's source directory changed, because the
//! command line client and the hub come from the same executable. The daemon itself restarts
//! only when a changed file is part of the code it runs. These are not:
//!
//! - the `ignite_cli` crate, the client and the entry point. A client starts from `bin/current`,
//!   which the release points at the new build without a restart. The protocol the client
//!   speaks lives in `ignite_core`, which the daemon runs, so a change to it still restarts.
//! - test code: a file named `tests.rs` or `<name>_tests.rs`, or one under a `tests` directory.
//!   Every such file is compiled only into tests.
//! - Markdown, which nothing compiles.
//!
//! Everything else counts, including `ignite_hub`: the daemon links it, serves the hub itself on
//! a machine whose hub has no login service of its own, and reads ticket bodies with its
//! functions. So does `Cargo.toml` or `Cargo.lock` and any file a crate embeds.

use ignite_core::ids::Commit;
use ignite_core::traits::{Git, Result};

/// The crate of the command line client, under Ignite's source directory.
const CLIENT_CRATE: &str = "crates/ignite_cli/";

/// Whether `path`, relative to the repository, is part of the code the daemon runs when Ignite's
/// source lives in `source`.
pub fn affects_daemon(source: &str, path: &str) -> bool {
    let prefix = format!("{}/", source.trim_end_matches('/'));
    let Some(inside) = path.strip_prefix(&prefix) else {
        return false;
    };
    if inside.starts_with(CLIENT_CRATE) {
        return false;
    }
    let mut parts: Vec<&str> = inside.split('/').collect();
    let name = parts.pop().unwrap_or_default();
    let test_file = name == "tests.rs" || name.ends_with("_tests.rs");
    let in_tests = parts.contains(&"tests");
    let markdown = std::path::Path::new(name)
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"));
    !(test_file || in_tests || markdown)
}

/// The files changed between `from` and `to` that are part of the code the daemon runs, sorted.
/// Empty when a daemon built from `from` would run the same code as one built from `to`.
pub fn daemon_changes(
    git: &dyn Git,
    source: &str,
    from: &Commit,
    to: &Commit,
) -> Result<Vec<String>> {
    let mut changed: Vec<String> = git
        .changed(from, to)?
        .into_iter()
        .filter(|path| affects_daemon(source, path))
        .collect();
    changed.sort();
    Ok(changed)
}

/// `changes` for a message: the first three, then how many more.
pub fn summary(changes: &[String]) -> String {
    let shown = changes
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match changes.len() {
        0..=3 => shown,
        n => format!("{shown} and {} more", n - 3),
    }
}

#[cfg(test)]
#[path = "scope_tests.rs"]
mod tests;
