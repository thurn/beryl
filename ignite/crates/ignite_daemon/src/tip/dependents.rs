//! Making sure an automatic revert still builds, and finding the later landings that have to
//! be undone with it (see "A broken build is reverted" in `docs/ignite.md`).
//!
//! A revert undoes one landing on a tip that holds every landing since. A later landing can
//! use a function the reverted landing added: then the revert lets the failing check pass, but
//! the tree no longer compiles, and `green` and every tip round stop until someone reverts the
//! later landing by hand. So before a revert is pushed, the round runs the build checks whose
//! inputs the revert changes at the revert commit.
//!
//! When one of them fails, the round looks for the later landings that depend on the reverted
//! one, undoes them in the same commit, and builds again. A later landing depends on it when
//! its added lines use a word the reverted landing introduced (a word in its added lines that
//! no text file held before it), or when it removed a line the reverted landing added. The
//! search for words reads git, not the language, so it names no tool of the repository. A
//! revert that still does not build is not pushed: the regression is reported, naming the
//! landings involved.

use std::collections::{BTreeSet, HashSet};

use ignite_core::ids::{CheckName, Commit};
use ignite_core::protocol::Outcome;
use ignite_core::traits::LineChanges;

use super::{TipLanding, TipVerifier, touches};
use crate::server::Context;

/// The shortest word that counts as something a landing introduced. Shorter words are mostly
/// loop variables and abbreviations that many unrelated lines share.
const MIN_WORD: usize = 3;

/// The shortest line, without its indentation, that counts when a later landing removes a line
/// the reverted landing added. Shorter lines (`}`, `None,`) are shared by unrelated code.
const MIN_LINE: usize = 12;

/// Whether the build checks pass at a revert.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum Built {
    /// Every build check whose inputs the revert changes passes there, or none does.
    Yes,
    /// These build checks fail at the revert.
    Fails(Vec<CheckName>),
    /// These build checks gave no verdict at the revert, so it is not known to build.
    Unknown(Vec<CheckName>),
}

/// The words of `line`: runs of ASCII letters, digits and underscores that start with a letter
/// or an underscore, at least [`MIN_WORD`] long.
fn words(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|word| word.len() >= MIN_WORD && !word.starts_with(|c: char| c.is_ascii_digit()))
}

/// The distinct words of `lines`, sorted.
fn all_words(lines: &[String]) -> Vec<String> {
    let set: BTreeSet<&str> = lines.iter().flat_map(|line| words(line)).collect();
    set.into_iter().map(str::to_owned).collect()
}

/// Whether `later` uses one of the `introduced` words in a line it added, or removed one of the
/// `added` lines (trimmed, at least [`MIN_LINE`] long).
fn depends_on(later: &LineChanges, introduced: &BTreeSet<String>, added: &HashSet<&str>) -> bool {
    later
        .added
        .iter()
        .flat_map(|line| words(line))
        .any(|word| introduced.contains(word))
        || later.removed.iter().any(|line| added.contains(line.trim()))
}

impl TipVerifier {
    /// Runs the round's build checks whose inputs `paths` touch at `revert`, all but `failing`,
    /// which has passed there already. A cached result counts.
    pub(super) fn builds_at(
        &self,
        ctx: &Context,
        revert: &Commit,
        paths: &[String],
        failing: &CheckName,
    ) -> Built {
        let names: Vec<CheckName> = self
            .round_builds
            .iter()
            .filter(|check| check.check != *failing && touches(paths, &check.inputs))
            .map(|check| check.check.clone())
            .collect();
        if names.is_empty() {
            return Built::Yes;
        }
        let runs = self.work.run(ctx, revert, &names, false, false);
        let outcome = |name: &CheckName| runs.iter().find(|r| r.check == *name).map(|r| r.outcome);
        let mut fails = Vec::new();
        let mut unknown = Vec::new();
        for name in names {
            match outcome(&name) {
                Some(Outcome::Pass | Outcome::Flaky) => {}
                Some(Outcome::Fail) => fails.push(name),
                Some(Outcome::Incomplete) | None => unknown.push(name),
            }
        }
        if !unknown.is_empty() {
            Built::Unknown(unknown)
        } else if !fails.is_empty() {
            Built::Fails(fails)
        } else {
            Built::Yes
        }
    }

    /// The landings after `landing` up to `base`, oldest first, that use a word `landing`
    /// introduced or removed a line it added (see the module's documentation).
    pub(super) fn dependents(
        &self,
        ctx: &Context,
        landing: &TipLanding,
        base: &Commit,
    ) -> Result<Vec<TipLanding>, String> {
        let (Some(first), Some(last)) = (landing.commits.first(), landing.commits.last()) else {
            return Ok(Vec::new());
        };
        if last == base {
            return Ok(Vec::new());
        }
        let changes = ctx
            .git
            .line_changes(&ctx.repo, &landing.commits)
            .map_err(|e| format!("the lines it changed could not be read: {e}"))?;
        let candidates = all_words(&changes.added);
        let before = ctx
            .git
            .words_before(&ctx.repo, first, &candidates)
            .map_err(|e| format!("the words before it could not be read: {e}"))?;
        let introduced: BTreeSet<String> = candidates
            .into_iter()
            .filter(|word| !before.contains(word))
            .collect();
        let added: HashSet<&str> = changes
            .added
            .iter()
            .map(|line| line.trim())
            .filter(|line| line.len() >= MIN_LINE)
            .collect();
        let later = self
            .work
            .landings(ctx, last, base)
            .map_err(|e| format!("the later landings could not be read: {e}"))?;
        let mut dependents = Vec::new();
        for other in later {
            if other.commits.is_empty() || other.commits.iter().any(|c| landing.commits.contains(c))
            {
                continue;
            }
            let changes = ctx
                .git
                .line_changes(&ctx.repo, &other.commits)
                .map_err(|e| format!("the lines a later landing changed could not be read: {e}"))?;
            if depends_on(&changes, &introduced, &added) {
                dependents.push(other);
            }
        }
        Ok(dependents)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|l| (*l).to_owned()).collect()
    }

    #[test]
    fn words_skip_short_runs_and_numbers() {
        let found: Vec<&str> = words("let x = nonland(3, 42abc) + a_b;").collect();
        assert_eq!(found, vec!["let", "nonland", "a_b"]);
    }

    #[test]
    fn a_later_landing_depends_by_a_word_or_a_removed_line() {
        let introduced: BTreeSet<String> = ["nonland".to_owned()].into();
        let added: HashSet<&str> = ["pub fn nonland() -> Filter {"].into();
        let calls = LineChanges {
            added: lines(&["    .filter(nonland())"]),
            removed: Vec::new(),
        };
        let edits = LineChanges {
            added: lines(&["pub fn nonland_card() -> Filter {"]),
            removed: lines(&["  pub fn nonland() -> Filter {"]),
        };
        let unrelated = LineChanges {
            added: lines(&["    .filter(land())"]),
            removed: lines(&["}"]),
        };
        assert!(depends_on(&calls, &introduced, &added));
        assert!(depends_on(&edits, &introduced, &added));
        assert!(!depends_on(&unrelated, &introduced, &added));
    }
}
