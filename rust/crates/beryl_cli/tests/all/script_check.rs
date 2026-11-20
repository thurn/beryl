use std::collections::BTreeMap;

use beryl_cli::script_check::moments;

use crate::common::TEST;

/// A moment whose session panics fails its own checks, from each seat, with the panic; the
/// card's other moments and another card's pass.
#[test]
fn a_failing_moment_fails_only_its_own_checks() {
    use beryl_card_names::names::{MANA_DRAIN, REPRIEVE};
    let scenarios = |card| {
        beryl_cards::scenarios()
            .iter()
            .find(|(name, _)| *name == card)
            .unwrap()
            .1
    };
    // A moment's name is its scenario's slug with the card's left off.
    let card = beryl_cli::specs::slug(MANA_DRAIN);
    let [broken, whole] = [0, 1].map(|i| {
        let slug = beryl_cli::specs::slug(scenarios(MANA_DRAIN)[i].name);
        slug.strip_prefix(&card)
            .unwrap()
            .trim_start_matches('-')
            .to_owned()
    });
    beryl_cli::specs::FAULTS
        .lock()
        .unwrap()
        .push(scenarios(MANA_DRAIN)[0].name);
    let fresh = beryl_cli::specs::fresh(&TEST, &[MANA_DRAIN, REPRIEVE]);
    let chosen: Vec<&beryl_cli::specs::Spec> = fresh.iter().collect();
    let mut checks = BTreeMap::new();
    moments(&chosen, &[], &mut checks);
    beryl_cli::specs::FAULTS.lock().unwrap().clear();
    let under = |prefix: &str| {
        let prefix = format!("{card}/{prefix}");
        checks
            .iter()
            .filter(move |(id, _)| id.starts_with(&prefix))
            .collect::<Vec<_>>()
    };
    for seat in ["yours", "theirs"] {
        let failed: Vec<_> = under(&broken)
            .into_iter()
            .filter(|(id, _)| id.ends_with(seat))
            .collect();
        assert!(!failed.is_empty(), "no {seat} check of {broken}");
        for (id, check) in failed {
            let why = check.failure.as_deref().unwrap_or_default();
            assert!(why.contains("a fault injected into"), "{id}: {why:?}");
        }
    }
    let failed = format!("{card}/{broken}");
    let passed: Vec<&String> = checks
        .iter()
        .filter(|(id, _)| !id.starts_with(&failed))
        .map(|(id, check)| {
            assert!(check.failure.is_none(), "{id} failed");
            id
        })
        .collect();
    assert!(passed.iter().any(|id| id.contains(&whole)), "{passed:?}");
    assert!(passed.iter().any(|id| id.starts_with("reprieve/")));
}
