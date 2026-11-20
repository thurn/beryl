//! The commit a new ticket tree starts at.
//!
//! With `repository.green` set, new work starts at `green`, the newest tip whose build checks
//! passed, because a landing can reach `staging` and fail to compile there until a tip round
//! reverts it. Two cases start elsewhere:
//!
//! - A ticket whose dependency landed after `green` last moved must still get that
//!   dependency's code, and must not wait for `green` to catch up. It starts at the newest
//!   landed commit of its dependencies that `green` does not hold.
//! - A regression ticket starts at the `staging` tip, because its failure is at the tip and a
//!   build that does not compile is not in `green`.
//!
//! Without `repository.green`, or while that branch does not exist yet, the start is the
//! `staging` tip, as it was before `green` existed.

use ignite_core::ids::{Commit, TicketId};
use ignite_core::protocol::tickets::{EdgeKind, TicketDetail};
use ignite_core::traits::Result;

use super::TicketDesk;
use crate::server::Context;

/// How a start commit was chosen, which the brief's `tree` line says.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum How {
    /// The tip of this branch (`green` or `staging`).
    Branch(String),
    /// The commit where this dependency landed, which `green` does not hold yet.
    Landed(TicketId),
}

impl How {
    /// The brief's text after `at`, for a tree based at `commit`.
    pub fn phrase(&self, commit: &Commit) -> String {
        let short = &commit.as_str()[..commit.as_str().len().min(7)];
        match self {
            How::Branch(branch) => format!("{branch} {short}"),
            How::Landed(ticket) => format!("{short}, where {ticket} landed"),
        }
    }
}

/// The commit a tree starts at, how it was chosen, and the note the brief prints when a
/// dependency's commit is what was chosen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Base {
    pub commit: Commit,
    pub how: How,
    pub note: Option<String>,
}

/// The tip of `green` when `repository.green` is set and the branch exists, otherwise the tip of
/// `staging`. `Landing::sync` rebases onto it.
pub fn green_or_tip(ctx: &Context) -> Result<(Commit, String)> {
    let repository = &ctx.config.repository;
    if let Some(green) = &repository.green
        && let Ok(commit) = ctx.git.tip(green)
    {
        return Ok((commit, green.clone()));
    }
    let commit = ctx.git.tip(&repository.staging)?;
    Ok((commit, repository.staging.clone()))
}

/// Where the tree of `detail`'s ticket starts; `None` for an ad hoc ticket, which has no
/// dependencies.
pub fn choose(ctx: &Context, desk: &dyn TicketDesk, detail: Option<&TicketDetail>) -> Result<Base> {
    let staging = &ctx.config.repository.staging;
    let at_tip = || -> Result<Base> {
        Ok(Base {
            commit: ctx.git.tip(staging)?,
            how: How::Branch(staging.clone()),
            note: None,
        })
    };
    if ctx.config.repository.green.is_none() || detail.is_some_and(is_regression) {
        return at_tip();
    }
    let (green, branch) = green_or_tip(ctx)?;
    if &branch == staging {
        return at_tip();
    }
    let Some(detail) = detail else {
        return Ok(Base {
            commit: green,
            how: How::Branch(branch),
            note: None,
        });
    };
    match newest_missing(ctx, desk, detail, &green)? {
        Some((commit, ticket)) => Ok(Base {
            note: Some(format!(
                "green does not hold {} yet; this tree starts at that commit, which no tip \
                 round has checked",
                &commit.as_str()[..commit.as_str().len().min(7)]
            )),
            commit,
            how: How::Landed(ticket),
        }),
        None => Ok(Base {
            commit: green,
            how: How::Branch(branch),
            note: None,
        }),
    }
}

fn is_regression(detail: &TicketDetail) -> bool {
    detail.ticket.labels.iter().any(|l| l == "regression")
}

/// The newest landed commit of a dependency of the ticket that `green` does not hold. The
/// history of `staging` is linear, so the newest commit holds the others.
fn newest_missing(
    ctx: &Context,
    desk: &dyn TicketDesk,
    detail: &TicketDetail,
    green: &Commit,
) -> Result<Option<(Commit, TicketId)>> {
    let mut newest: Option<(Commit, TicketId)> = None;
    for edge in &detail.edges {
        if edge.src != detail.ticket.id || edge.kind != EdgeKind::Blocks {
            continue;
        }
        // A dependency that cannot be read does not hold up the start: the tree then starts at
        // `green` without its code, as it would for a dependency that closed without landing.
        let Ok(dependency) = desk.detail(&edge.dst) else {
            continue;
        };
        let Some(commit) = dependency
            .ticket
            .landed
            .as_ref()
            .and_then(|work| work.commits.last())
        else {
            continue;
        };
        if ctx.git.is_ancestor(commit, green)? {
            continue;
        }
        let newer = match &newest {
            Some((held, _)) => ctx.git.is_ancestor(held, commit)?,
            None => true,
        };
        if newer {
            newest = Some((commit.clone(), edge.dst.clone()));
        }
    }
    Ok(newest)
}
