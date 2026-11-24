use std::collections::BTreeMap;

use beryl_card_names::names::{ISLAND, MANA_CRYPT, TUNDRA};
use beryl_cli::Catalog;
use beryl_cli::script_check::moments;
use beryl_cli::specs::{self, View};
use beryl_scenario::{Action, P0, Scenario};

use crate::common::TEST;

const BROKEN: &str = "Script check with injected panic";
const CRYPT_SCENARIOS: &[Scenario] = &[
    Scenario::new(BROKEN)
        .hand(P0, &[MANA_CRYPT, ISLAND])
        .actions(&[Action::cast(P0, MANA_CRYPT)]),
    Scenario::new("Script check without a panic")
        .hand(P0, &[MANA_CRYPT, ISLAND])
        .actions(&[Action::cast(P0, MANA_CRYPT)]),
];
const TUNDRA_SCENARIOS: &[Scenario] = &[Scenario::new("Script check on another card")
    .hand(P0, &[TUNDRA, MANA_CRYPT])
    .actions(&[Action::PlayLand {
        player: P0,
        card: TUNDRA,
    }])];

static CATALOG: Catalog = Catalog {
    scenarios: || &[(MANA_CRYPT, CRYPT_SCENARIOS), (TUNDRA, TUNDRA_SCENARIOS)],
    ..TEST
};

/// A moment whose session panics fails its own checks, from each seat, with the panic; the
/// card's other moments and another card's pass.
#[test]
fn a_failing_moment_fails_only_its_own_checks() {
    let fresh = specs::fresh(&CATALOG, &[MANA_CRYPT, TUNDRA]);
    let chosen: Vec<_> = fresh.iter().collect();
    let mut checks = BTreeMap::new();
    specs::FAULTS.lock().unwrap().push(BROKEN);
    moments(&chosen, &[], &mut checks);
    specs::FAULTS.lock().unwrap().clear();

    let broken = "mana-crypt/script-check-with-injected-panic";
    for view in View::ALL {
        let seat = view.name();
        for moment in [
            broken,
            "mana-crypt/script-check-without-a-panic",
            "tundra/script-check-on-another-card",
        ] {
            let id = format!("{moment}/{seat}");
            assert!(checks.contains_key(&id), "missing {id}");
        }
    }
    for (id, check) in checks {
        if id.starts_with(broken) {
            let why = check.failure.as_deref().unwrap_or_default();
            assert!(why.contains("a fault injected into"), "{id}: {why:?}");
        } else {
            assert!(check.failure.is_none(), "{id} failed: {:?}", check.failure);
        }
    }
}
