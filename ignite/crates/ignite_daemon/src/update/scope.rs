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
//! - the repository's check manifests (`repository.checks` in `ignite.toml`), when that directory
//!   lies under Ignite's source. No executable compiles them: each verb process, including a tip
//!   round's, reads them from the main checkout when it starts, and the daemon reads them again
//!   whenever it fast-forwards the main checkout (see `crate::manifests`). A change to them
//!   is not a change to Ignite's source at all ([`ignite_change`]), so it builds no release.
//! - the files in [`CLIENT_ONLY`]: text compiled into the executable that only the client reads,
//!   such as the guide topics `ignite guide` prints.
//!
//! Everything else counts, including `ignite_hub`: the daemon links it, serves the hub itself on
//! a machine whose hub has no login service of its own, and reads ticket bodies with its
//! functions. So does `Cargo.toml` or `Cargo.lock` and any file a crate embeds.

use ignite_core::ids::Commit;
use ignite_core::traits::{Git, Result};

/// The crate of the command line client, under Ignite's source directory.
const CLIENT_CRATE: &str = "crates/ignite_cli/";

/// Files, relative to Ignite's source directory, that are compiled into the executable but read
/// only by the client, as (path prefix, file extension, why the daemon never reads them). A
/// changed file that starts with the prefix and has the extension does not restart the daemon:
/// the client runs `bin/current`, which the release points at its new build without a restart,
/// so the client prints the new text at once.
const CLIENT_ONLY: &[(&str, &str, &str)] = &[(
    "crates/ignite_core/src/guide/",
    "txt",
    // `ignite_core::guide` embeds each topic with `include_str!`, and only the client calls it
    // (`ignite guide`, answered in `ignite_cli` without asking the daemon). The daemon names
    // topics in its messages, such as a claim's brief, but never reads their text. The module's
    // `.rs` file is not on the list: it holds the topic names and summaries, which are code.
    "the guide topics, which only `ignite guide` in the client prints",
)];

/// Whether `inside`, relative to Ignite's source directory, is on [`CLIENT_ONLY`].
fn client_only(inside: &str) -> bool {
    CLIENT_ONLY.iter().any(|(prefix, extension, _)| {
        inside.strip_prefix(prefix).is_some_and(|rest| {
            std::path::Path::new(rest)
                .extension()
                .is_some_and(|found| found.eq_ignore_ascii_case(extension))
        })
    })
}

/// Where Ignite's source and the repository's check manifests live, as directories relative to
/// the repository.
#[derive(Clone, Copy, Debug)]
pub struct Source<'a> {
    /// Ignite's source directory, `crate::update::SOURCE`.
    pub source: &'a str,
    /// The check manifests' directory, `repository.checks` in `ignite.toml`. It may lie inside
    /// `source`, and then its files are still not Ignite's source.
    pub checks: &'a str,
}

impl<'a> Source<'a> {
    /// Ignite's source directory, [`crate::update::SOURCE`], with the manifests in `checks`.
    pub fn ignite(checks: &'a str) -> Source<'a> {
        Source {
            source: crate::update::SOURCE,
            checks,
        }
    }
}

/// The check manifests' directory from `config`, with `/` between its parts, as the paths Git
/// reports have.
pub fn checks_dir(config: &ignite_core::config::Config) -> String {
    config
        .repository
        .checks
        .to_string_lossy()
        .replace('\\', "/")
}

/// `path` relative to `dir`, when `path` lies under the directory `dir`.
fn inside<'p>(dir: &str, path: &'p str) -> Option<&'p str> {
    let dir = dir.trim_end_matches('/');
    if dir.is_empty() {
        return None;
    }
    path.strip_prefix(dir)?.strip_prefix('/')
}

/// Whether `path`, relative to the repository, is part of Ignite's source: under the source
/// directory and not under the check manifests' directory. A landing that changes such a file
/// judges the release (see `crate::land::release`).
pub fn ignite_change(at: Source<'_>, path: &str) -> bool {
    inside(at.source, path).is_some() && inside(at.checks, path).is_none()
}

/// Whether `path`, relative to the repository, is part of the code the daemon runs.
pub fn affects_daemon(at: Source<'_>, path: &str) -> bool {
    if !ignite_change(at, path) {
        return false;
    }
    let Some(inside) = inside(at.source, path) else {
        return false;
    };
    if inside.starts_with(CLIENT_CRATE) || client_only(inside) {
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
    at: Source<'_>,
    from: &Commit,
    to: &Commit,
) -> Result<Vec<String>> {
    let mut changed: Vec<String> = git
        .changed(from, to)?
        .into_iter()
        .filter(|path| affects_daemon(at, path))
        .collect();
    changed.sort();
    Ok(changed)
}

/// The daemon's landing code, relative to Ignite's source directory: the `land` module, which
/// `ignite land` runs in its verb process.
const LANDING_CODE: &[&str] = &[
    "crates/ignite_daemon/src/land.rs",
    "crates/ignite_daemon/src/land/",
];

/// Whether `path`, relative to the repository, is part of the code a landing runs. A landing whose verb process runs an older build than the daemon
/// goes on under that build unless the release changed such a file (see `server/outdated.rs`),
/// and only such a release waits for the landings that run before it restarts the daemon (see
/// `main/selfupdate.rs`).
pub fn affects_landing(at: Source<'_>, path: &str) -> bool {
    affects_daemon(at, path)
        && inside(at.source, path)
            .is_some_and(|inside| LANDING_CODE.iter().any(|code| inside.starts_with(code)))
}

/// The files changed between `from` and `to` that are part of the code a landing runs, sorted.
/// Empty when a landing started under `from` would run the same landing code under `to`.
pub fn landing_changes(
    git: &dyn Git,
    at: Source<'_>,
    from: &Commit,
    to: &Commit,
) -> Result<Vec<String>> {
    Ok(daemon_changes(git, at, from, to)?
        .into_iter()
        .filter(|path| affects_landing(at, path))
        .collect())
}

/// The daemon's tip-verification code, relative to Ignite's source directory: the `tip` module,
/// which `ignite tip-round` runs in its verb process.
const TIP_CODE: &[&str] = &[
    "crates/ignite_daemon/src/tip.rs",
    "crates/ignite_daemon/src/tip/",
];

/// Whether `path`, relative to the repository, is part of the code a tip round runs. A tip round
/// whose verb process runs an older build than the daemon goes on under that build unless the
/// release changed such a file (see `server/outdated.rs`).
pub fn affects_tip(at: Source<'_>, path: &str) -> bool {
    affects_daemon(at, path)
        && inside(at.source, path)
            .is_some_and(|inside| TIP_CODE.iter().any(|code| inside.starts_with(code)))
}

/// The files changed between `from` and `to` that are part of the code a tip round runs, sorted.
/// Empty when a tip round started under `from` would run the same tip code under `to`.
pub fn tip_changes(
    git: &dyn Git,
    at: Source<'_>,
    from: &Commit,
    to: &Commit,
) -> Result<Vec<String>> {
    Ok(daemon_changes(git, at, from, to)?
        .into_iter()
        .filter(|path| affects_tip(at, path))
        .collect())
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
