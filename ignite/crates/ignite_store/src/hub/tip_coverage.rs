//! Durable platform verification coverage. The daemon compares commits with git, and the hub
//! only writes the replacement if no other round has changed that platform meanwhile.

use std::collections::BTreeMap;

use ignite_core::config::Os;
use ignite_core::ids::Commit;
use ignite_core::protocol::tickets::FeedChange;
use ignite_core::time::Timestamp;
use ignite_core::tipround::{
    PlatformVerification, TipBudgetPlan, TipCoverage, TipCoverageUpdate, TipSelection,
    TipSelectionUpdate,
};
use rusqlite::{Connection, TransactionBehavior};

use super::{feed, fleet_settings, meta, tips};
use crate::{Result, StoreError};

const KEY: &str = "tip_coverage";

fn platforms(conn: &Connection) -> Result<BTreeMap<Os, PlatformVerification>> {
    Ok(meta::get(conn, KEY)?.unwrap_or_default())
}

pub fn get(conn: &Connection) -> Result<TipCoverage> {
    let settings = fleet_settings::settings(conn)?;
    Ok(TipCoverage {
        platforms: platforms(conn)?,
        off_machines: fleet_settings::off_machines(conn, &settings)?,
    })
}

pub fn update(conn: &mut Connection, update: &TipCoverageUpdate, now: Timestamp) -> Result<bool> {
    let transaction = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    let mut platforms = platforms(&transaction)?;
    if platforms.get(&update.platform) != update.expected.as_ref() {
        return Ok(false);
    }
    if update.replacement.deferred.is_some() && !update.checks_deferred {
        let settings = fleet_settings::settings(&transaction)?;
        let off = fleet_settings::unavailable_platforms(&transaction, &settings, now)?;
        if !off.contains(&update.platform)
            && !fleet_settings::checks_only_platforms(&transaction, &settings, now)?
                .contains(&update.platform)
            && !fleet_settings::waited_platforms(&transaction)?.contains(&update.platform)
        {
            return Err(StoreError::Rejected(format!(
                "{} can verify tips again",
                update.platform
            )));
        }
    }
    platforms.insert(update.platform, update.replacement.clone());
    meta::put(&transaction, KEY, &platforms)?;
    feed::record(
        &transaction,
        &FeedChange {
            seq: feed::next_seq(&transaction)?,
            kind: "attention".to_owned(),
            ticket: None,
            name: Some(update.platform.to_string()),
        },
    )?;
    transaction.commit()?;
    Ok(true)
}

/// The budget plan for one platform at this candidate. An older candidate's plan cannot
/// exempt a check even if its key did not change.
pub fn budget(conn: &Connection, commit: &Commit, platform: Os) -> Result<Option<TipBudgetPlan>> {
    let plan: Option<TipBudgetPlan> = meta::get(conn, &format!("tip_budget.{platform}"))?;
    Ok(plan.filter(|plan| plan.commit == *commit))
}

/// One slot per platform, so publishing a plan costs no storage once its candidate moves on.
pub fn put_budget(conn: &mut Connection, plan: &TipBudgetPlan) -> Result<bool> {
    if plan
        .after
        .iter()
        .any(|key| key.platform != Some(plan.platform))
    {
        return Err(StoreError::Invalid(
            "tip budget contains a key for another platform".to_owned(),
        ));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if tips::candidate(&tx)?.is_some_and(|candidate| candidate.commit != plan.commit) {
        return Ok(false);
    }
    meta::put(&tx, &format!("tip_budget.{}", plan.platform), plan)?;
    tx.commit()?;
    Ok(true)
}

/// One slot holds the candidate's budget decisions, keyed by exact result keys.
const SELECTION_KEY: &str = "tip_selection";

pub fn selection(conn: &Connection, commit: &Commit) -> Result<Option<TipSelection>> {
    let selection: Option<TipSelection> = meta::get(conn, SELECTION_KEY)?;
    Ok(selection.filter(|selection| selection.commit == *commit))
}

pub fn update_selection(conn: &mut Connection, update: &TipSelectionUpdate) -> Result<bool> {
    if update.portable.as_ref().is_some_and(|portable| {
        !portable.billions.is_finite()
            || portable.billions < 0.0
            || portable.after.iter().any(|key| key.platform.is_some())
    }) || update
        .native_after
        .iter()
        .any(|key| key.platform.is_none() || key.platform != update.platform)
    {
        return Err(StoreError::Invalid(
            "tip selection has an invalid cost or a result key in the wrong partition".to_owned(),
        ));
    }
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    if tips::candidate(&tx)?.is_some_and(|candidate| candidate.commit != update.commit) {
        return Ok(false);
    }
    let Some(selection) = update.apply(selection(&tx, &update.commit)?) else {
        return Ok(false);
    };
    meta::put(&tx, SELECTION_KEY, &selection)?;
    tx.commit()?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use ignite_core::time::Timestamp;
    use ignite_core::tipround::{PortableTipBudget, TipCandidate, TipCandidateUpdate};

    use super::*;

    #[test]
    fn a_racing_verifier_can_add_its_platform_but_cannot_replace_the_selection() {
        let mut conn = super::super::open_in_memory().unwrap();
        let commit = Commit::new("aaaa").unwrap();
        let mut proposal = TipSelectionUpdate {
            commit: commit.clone(),
            portable: Some(PortableTipBudget {
                after: Vec::new(),
                billions: 10.0,
            }),
            platform: Some(Os::Macos),
            native_after: Vec::new(),
        };
        assert!(update_selection(&mut conn, &proposal).unwrap());
        proposal.platform = Some(Os::Windows);
        proposal.portable.as_mut().unwrap().billions = 20.0;
        assert!(!update_selection(&mut conn, &proposal).unwrap());
        proposal.portable = None;
        assert!(update_selection(&mut conn, &proposal).unwrap());
        assert!(!update_selection(&mut conn, &proposal).unwrap());

        let fixed = selection(&conn, &commit).unwrap().unwrap();
        assert_eq!(
            fixed.native.keys().copied().collect::<Vec<_>>(),
            [Os::Macos, Os::Windows]
        );
        assert!((fixed.portable.billions - 10.0).abs() < f64::EPSILON);
        let newer = Commit::new("bbbb").unwrap();
        assert!(
            tips::update_candidate(
                &mut conn,
                &TipCandidateUpdate {
                    expected: None,
                    replacement: TipCandidate {
                        commit: newer.clone(),
                        retry_on_new_tip: false
                    },
                },
                Timestamp::EPOCH
            )
            .unwrap()
        );
        assert!(!update_selection(&mut conn, &proposal).unwrap());
        assert!(selection(&conn, &newer).unwrap().is_none());
    }
}
