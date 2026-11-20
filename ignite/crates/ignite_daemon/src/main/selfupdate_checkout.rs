//! Bringing the main checkout up to a release before the daemon restarts into it.
//!
//! The daemon reads `ignite.toml` from the main checkout only when it starts. The `ignite-main`
//! loop fast-forwards that checkout every 30 seconds, but a release whose build is stored
//! restarts the daemon within seconds of the release ref moving. On 2026-11-20 a commit pushed
//! to the integration branch and the release ref at once changed both `ignite.toml`'s
//! `[repository]` section and the daemon's code. Every daemon restarted into the release about
//! two seconds before its checkout was fast-forwarded, so each kept the old `[repository]`
//! names until a later release restarted it again.
//!
//! So before a restart into a release goes ahead, [`SelfUpdate::checkout_holds`] fast-forwards
//! the main checkout itself and asks whether the commit the checkout has checked out contains
//! the release. While it does not (the remote has not shown the release yet, or the checkout has
//! local edits and is not fast-forwarded), the restart waits, like any other held restart, at
//! most the restart window's `cap_seconds` from the first pending release (see
//! [`crate::main::restart_window`]). After the cap the daemon restarts anyway, on whatever
//! `ignite.toml` the checkout has, rather than stay on an old build indefinitely.

use std::path::PathBuf;

use ignite_core::ids::Commit;
use ignite_core::traits::Result;

use super::SelfUpdate;
use crate::trees::Trees;

/// The main checkout, as the self-update sees it before a restart.
pub trait MainCheckout: Send + Sync {
    /// Fast-forwards the checkout's integration branches to the remote's and answers whether the
    /// commit it has checked out now contains `release`, so that its `ignite.toml` is the one
    /// that came with the release.
    fn bring_up_to(&self, release: &Commit) -> Result<bool>;
}

/// The real main checkout: fast-forwarded by [`Trees::fast_forward_main`], as the `ignite-main`
/// loop does, and asked about with `git merge-base --is-ancestor`.
pub struct RepositoryCheckout {
    pub trees: Trees,
    /// The main checkout's directory.
    pub repo: PathBuf,
}

impl MainCheckout for RepositoryCheckout {
    fn bring_up_to(&self, release: &Commit) -> Result<bool> {
        self.trees.fast_forward_main()?;
        let head = ignite_git::refs::head(&self.repo)?;
        ignite_git::refs::is_ancestor(&self.repo, release, &head)
    }
}

/// Why a restart into a release waits while the main checkout lacks it.
pub const CHECKOUT_LACKS_RELEASE: &str =
    "the main checkout does not hold the release yet, so its `ignite.toml` would be the old one";

impl SelfUpdate {
    /// Whether the main checkout holds `release` after a fast-forward (see the module
    /// documentation). A checkout that cannot be fast-forwarded or asked counts as not holding
    /// it, so the restart waits up to the window's cap.
    pub(super) fn checkout_holds(&self, release: &Commit) -> bool {
        match self.checkout.bring_up_to(release) {
            Ok(holds) => holds,
            Err(error) => {
                eprintln!(
                    "ignite: cannot bring the main checkout up to release {release}: {error}"
                );
                false
            }
        }
    }
}

/// The end of a restart's reason in `starts.jsonl` that says how long it took: `waited` seconds
/// since the first pending release appeared, `held` of them after a release was ready (see
/// [`super::Decision::Go`]). On 2026-11-19 restarts read "after waiting 185 s", well past the
/// 30 s cap, though the window had held them for no time at all: the release had been building.
/// The build's time is now named apart.
pub(super) fn waited_for(waited: f64, held: f64) -> String {
    let building = waited - held;
    let ready = if building >= 1.0 {
        format!(", ready {building:.0} s after the release appeared")
    } else {
        String::new()
    };
    let waiting = if held > 0.0 {
        format!(", after waiting {held:.0} s for landings, newer releases and the main checkout")
    } else {
        String::new()
    };
    format!("{ready}{waiting}")
}
