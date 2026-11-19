//! The landing's side of building a release before it lands: an express landing names the
//! commit it is about to check in [`CANDIDATE_REF`], and the daemon builds that commit's
//! release while the landing runs its express commands (see
//! `crate::main::selfupdate::SelfUpdate::poll_candidate`).
//!
//! On 2026-11-19 two Ignite-only fixes each waited 45 to 47 s after their landing moved the
//! release branch for the daemon to restart into it, 36 s of which was the release build. The
//! landing's express commands had just spent 31 to 35 s checking the same commit. An express
//! landing pushes the commit it checked, so when it lands, the release branch moves to that
//! commit and the build is already stored: only the selftest is left.
//!
//! The ref is a hint. A landing that fails, or loses the race for the tip and rebases again,
//! leaves a stored build nobody restarts into, and the next landing points the ref elsewhere.

use std::path::Path;

use ignite_core::ids::Commit;

use crate::server::Context;

/// The local ref an express landing points at the commit it is about to check. Every worktree
/// of the repository shares it, so the daemon reads it from the main checkout.
pub const CANDIDATE_REF: &str = "refs/ignite/release-candidate";

/// Points [`CANDIDATE_REF`] at `commit`, the rebased commit in the ticket tree `tree`, when the
/// landing is an express landing (`express`). A failure to write the ref only means the
/// release is built after it lands, as before, so it is ignored.
pub fn offer(ctx: &Context, tree: &Path, commit: &Commit, express: bool) {
    if express {
        let _ = ctx.git.set_local_ref(tree, CANDIDATE_REF, commit);
    }
}
