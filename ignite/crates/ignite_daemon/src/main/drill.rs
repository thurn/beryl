//! Drills of the daemon's restarts, for "Manual QA" in `docs/ignite.md` and the end-to-end
//! restart test.
//!
//! - `ignite internal restart` sends the hidden `internal-restart` verb ([`RestartVerb`]), which
//!   makes the running daemon restart into its own executable through the same steps as a
//!   restart into a new release (see [`SelfUpdate::restart_now`]). Before it replies, the daemon
//!   stops taking requests, so the next request a client sends is answered by the new daemon.
//!   With the argument [`PROBE`] the verb only answers with the daemon's process ID, which is
//!   how the client tells when the new daemon answers.
//! - The file [`FAULT_START`] in the Ignite home makes `ignite daemon` exit with code 1 right
//!   after it has recorded its start, as a build that crashes at start would, so the start-loop
//!   rollback to `bin/last-good` can be drilled without building a broken release. The file
//!   names the commit of the build that is to fail, so the last-good build it rolls back to
//!   starts normally; an empty file makes every build fail. When the file is absent, checking
//!   for it costs one failed open.

use std::path::PathBuf;
use std::sync::{Arc, Weak};

use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_store::local::trees;

use super::selfupdate::SelfUpdate;
use crate::server::{Context, Server, Verb};
use crate::update::Home;

/// The verb `ignite internal restart` sends.
pub const RESTART_VERB: &str = "internal-restart";

/// The argument of [`RESTART_VERB`] that asks only for the daemon's process ID.
pub const PROBE: &str = "probe";

/// The file in the Ignite home that makes a build exit at start.
pub const FAULT_START: &str = "fault-start";

/// The contents of [`FAULT_START`] in `home`, trimmed, or `None` when it is absent.
pub fn fault_named(home: &Home) -> Option<String> {
    std::fs::read_to_string(home.root.join(FAULT_START))
        .ok()
        .map(|text| text.trim().to_owned())
}

/// The exit code for a starting daemon whose build [`FAULT_START`] in `home` names, or `None`
/// when the file is absent or names another build. `version` gives the running build's commit;
/// it is asked only when the file exists.
pub fn fault_exit(home: &Home, version: impl FnOnce() -> String) -> Option<i32> {
    let named = fault_named(home)?;
    let version = version();
    if !faults(&named, &version) {
        return None;
    }
    eprintln!(
        "ignite: {} names this build ({version}), so it exits as a build that crashes at start \
         would",
        home.root.join(FAULT_START).display()
    );
    Some(1)
}

/// Whether a fault file naming `named` makes the build of `version` fail: an empty name names
/// every build, and either may be an abbreviation of the other.
pub fn faults(named: &str, version: &str) -> bool {
    named.is_empty() || version.starts_with(named) || named.starts_with(version)
}

/// The hidden verb behind `ignite internal restart`.
pub struct RestartVerb {
    pub update: Arc<SelfUpdate>,
    pub server: Weak<Server>,
    /// The daemon's own executable, which it restarts into.
    pub executable: PathBuf,
    /// The daemon's local database, which says which trees are leased.
    pub database: PathBuf,
}

impl RestartVerb {
    /// The refusal for a caller that holds a build tree or asks from inside one: a session at
    /// work does not restart the daemon under everyone else. The drill is run from a shell.
    fn refuse_a_tree_holder(&self, ctx: &Context) -> Option<Reply> {
        let holds = |session: &str| -> Result<bool, String> {
            let conn = ignite_store::local::open(&self.database).map_err(|e| e.to_string())?;
            let all = trees::all(&conn).map_err(|e| e.to_string())?;
            Ok(all.iter().any(|tree| {
                tree.state == trees::State::Leased.as_str()
                    && tree.session.as_deref() == Some(session)
            }))
        };
        let held = match (&ctx.tree, &ctx.session) {
            (Some(tree), _) => Some(format!("is asked from inside {tree}")),
            (None, Some(session)) => match holds(session.as_str()) {
                Ok(true) => Some(format!("is asked by {session}, which holds a tree")),
                Ok(false) => None,
                Err(error) => {
                    let text = format!("failed   cannot read which trees are leased: {error}");
                    return Some(Reply::text(Exit::Usage, text));
                }
            },
            (None, None) => None,
        }?;
        Some(Reply::text(
            Exit::Conflict,
            format!(
                "conflict the restart {held}; run `ignite internal restart` from a shell \
                 outside every build tree, in no session that holds one"
            ),
        ))
    }
}

impl Verb for RestartVerb {
    fn name(&self) -> &str {
        RESTART_VERB
    }

    /// It acts on no tree; [`RestartVerb::refuse_a_tree_holder`] refuses tree holders itself.
    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    /// It gets no handle and is not counted among the verbs a restart waits for, or the restart
    /// it starts would wait for it.
    fn blocks_update(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let pid = std::process::id();
        if req.args.first().map(String::as_str) == Some(PROBE) {
            return Reply {
                json: Some(serde_json::json!({ "pid": pid })),
                ..Reply::text(Exit::Success, format!("daemon   {pid} answers"))
            };
        }
        if let Some(refusal) = self.refuse_a_tree_holder(ctx) {
            return refusal;
        }
        let Some(server) = self.server.upgrade() else {
            return Reply::text(Exit::Usage, "failed   the daemon is shutting down");
        };
        server.refuse_new_requests();
        let (update, executable) = (self.update.clone(), self.executable.clone());
        let spawned = std::thread::Builder::new()
            .name("ignite-drill-restart".to_owned())
            .spawn(move || {
                let steps = server.steps().clone();
                update.restart_now(&executable, &|| steps.critical_now());
            });
        if let Err(error) = spawned {
            if let Some(server) = self.server.upgrade() {
                crate::server::Drain::undrain(server.as_ref());
            }
            return Reply::text(
                Exit::Usage,
                format!("failed   cannot start the restart: {error}"),
            );
        }
        Reply {
            json: Some(serde_json::json!({
                "pid": pid,
                "restarting_into": self.executable,
            })),
            ..Reply::text(
                Exit::Success,
                format!(
                    "restart  the daemon {pid} is restarting into {}",
                    self.executable.display()
                ),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::faults;

    #[test]
    fn a_fault_file_names_the_build_that_fails() {
        assert!(faults("", "unknown"));
        assert!(faults("a1b2c3d", "a1b2c3d4e5f6"));
        assert!(faults("a1b2c3d4e5f6", "a1b2c3d"));
        assert!(!faults("e4f5a6b", "a1b2c3d4e5f6"));
    }
}
