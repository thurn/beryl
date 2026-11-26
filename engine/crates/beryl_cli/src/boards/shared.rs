//! The verb suite's boards in a game with one shared library and graveyard
//! (`Format::SHARED_DECK`): each plays what its per-player board in `verbs` plays, with both
//! players' libraries made one, so that the client's draws, mills, looks, reveals and cards put
//! back into a library start or end at the shared piles, from both seats.

use beryl_card_names::names::{ELITE_SPELLBINDER, MEMORY_LAPSE, PLAINS};
use beryl_engine::TurnStep;
use beryl_scenario::{Action, P0, P1, Scenario};

use super::shorthand::{ISLAND_, LANDS, PLAINS_};
use super::verbs::{
    DRAWS, LOOK, PILES, PILES_GRAVEYARD, REVEAL, REVEALED, SCRIED, SCRY, piles_table, reveal_table,
    scry_table,
};

/// The shared library of shared-verbs-piles, top first: the first seat's draws, then the
/// opponent's.
const PILES_LIBRARY: &[&str] = &concat::<{ DRAWS.len() + LANDS.len() }>(DRAWS, LANDS);

/// The shared library of shared-verbs-scry and shared-verbs-look, top first.
const SCRY_LIBRARY: &[&str] = &concat::<{ SCRIED.len() + LANDS.len() }>(SCRIED, LANDS);

/// The shared library of shared-verbs-reveal, top first.
const REVEAL_LIBRARY: &[&str] = &concat::<{ REVEALED.len() + LANDS.len() }>(REVEALED, LANDS);

/// `top` followed by `under`, as one list of `N` names.
const fn concat<const N: usize>(top: &[&'static str], under: &[&'static str]) -> [&'static str; N] {
    assert!(
        top.len() + under.len() == N,
        "the lists' lengths add up to N"
    );
    let mut all = [""; N];
    let mut i = 0;
    while i < top.len() {
        all[i] = top[i];
        i += 1;
    }
    while i < N {
        all[i] = under[i - top.len()];
        i += 1;
    }
    all
}

/// Boards for the verb suite in a shared-deck game.
pub(super) const SHARED: &[Scenario] = &[
    // verbs-piles: draws for the turn and by spells, a mill, cards put back on the library and
    // a permanent put into its owner's library, all from and to the one library.
    piles_table("shared-verbs-piles")
        .shared_library(PILES_LIBRARY)
        .shared_graveyard(PILES_GRAVEYARD)
        .actions(PILES),
    // verbs-scry: the first seat scries two of the shared library and puts one on the bottom.
    scry_table("shared-verbs-scry")
        .shared_library(SCRY_LIBRARY)
        .actions(SCRY),
    // verbs-look: the same scry left open while the first seat looks.
    scry_table("shared-verbs-look")
        .shared_library(SCRY_LIBRARY)
        .actions(LOOK),
    // verbs-reveal: Trinket Mage finds Mana Crypt in the shared library and reveals it, and the
    // opponent's Oust puts the face-down Snapcaster Mage into the shared library.
    reveal_table("shared-verbs-reveal")
        .shared_library(REVEAL_LIBRARY)
        .actions(REVEAL),
    // A spell put on top of the shared library: in the opponent's turn the first seat counters
    // their Elite Spellbinder with Memory Lapse, which puts it on top of the one library, and the
    // first seat draws it in the turn after.
    Scenario::new("shared-verbs-lapse")
        .at(2, P1, TurnStep::PrecombatMain)
        .permanents(P0, &[ISLAND_, ISLAND_])
        .hand(P0, &[MEMORY_LAPSE])
        .permanents(P1, &[PLAINS_, PLAINS_, PLAINS_])
        .hand(P1, &[ELITE_SPELLBINDER, PLAINS])
        .shared_library(LANDS)
        .actions(&[
            Action::cast(P1, ELITE_SPELLBINDER),
            Action::cast(P0, MEMORY_LAPSE),
        ]),
];
