//! Covered families: a family whose `covered_by` names another family is not run twice in tip
//! verification (see "How long a round takes" in `docs/ignite.md`).
//!
//! A check of a covered family is left out of a tip's set when the covering family lists a
//! check of the same name (the part after the family) on the same platform that reads at least
//! what it reads. The covering check is in the set already, so its pass is what moves `master`,
//! and a failure is reported once, for the covering check. A covered check with no such
//! counterpart stays in the set and runs. The landing gate does not use `covered_by`.

use std::collections::{BTreeMap, HashMap, HashSet};

use ignite_core::config::Os;
use ignite_core::ids::CheckName;

use super::TipCheck;

/// `checks` without the checks that a check of their covering family stands for.
pub(super) fn drop_covered(
    checks: Vec<TipCheck>,
    covered_by: &BTreeMap<String, String>,
    regressions: &HashSet<CheckName>,
) -> Vec<TipCheck> {
    if covered_by.is_empty() {
        return checks;
    }
    // The inputs of each check of a covering family, by family, name and platform.
    let mut covering: HashMap<(&str, &str, Option<Os>), HashSet<&str>> = HashMap::new();
    for check in &checks {
        if covered_by.values().any(|family| *family == check.family) {
            covering.insert(
                (check.family.as_str(), short(check), check.key.platform),
                check.inputs.iter().map(String::as_str).collect(),
            );
        }
    }
    let stood_for: Vec<bool> = checks
        .iter()
        .map(|check| {
            // A regression needs a pass for the check that filed it.
            !regressions.contains(&check.check)
                && covered_by.get(&check.family).is_some_and(|family| {
                    covering
                        .get(&(family.as_str(), short(check), check.key.platform))
                        .is_some_and(|inputs| {
                            check.inputs.iter().all(|i| inputs.contains(i.as_str()))
                        })
                })
        })
        .collect();
    checks
        .into_iter()
        .zip(stood_for)
        .filter(|(_, stood_for)| !stood_for)
        .map(|(check, _)| check)
        .collect()
}

/// A check's name after its family.
fn short(check: &TipCheck) -> &str {
    check
        .check
        .as_str()
        .strip_prefix(check.family.as_str())
        .and_then(|rest| rest.strip_prefix(':'))
        .unwrap_or(check.check.as_str())
}
