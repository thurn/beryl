//! A text change on a permanent spell goes on with the permanent the spell becomes
//! ([CR 400.7a](https://yawgatog.com/resources/magic-rules/#R4007a)): "Effects from spells,
//! activated abilities, and triggered abilities that change the characteristics or controller
//! of a permanent spell on the stack continue to apply to the permanent that spell becomes."
//! Crystal Spray's ruling says it of the card: "If you change the text of a spell which is to
//! become a permanent, the permanent will retain the text change until the effect wears off."
//!
//! This is no scenario of Crystal Spray's because no oracle can check it: `XMage` has no Crystal
//! Spray, and Forge drops the change as the spell resolves, so the divergence cannot be
//! recorded (`card diverge` needs the other oracle to pass).

use beryl_card::{Subtype, TextWord, TurnStep};
use beryl_scenario::Scenario;
use beryl_scenario_types::prelude::{ME, P0, Resolve, advance_to};

use crate::{CARDS, names};

/// P0 casts Wild Nacatl, and in response Crystal Spray makes the Nacatl spell's "Mountain"
/// read "Island". As a permanent it keeps the change, so P0's Islands give it +1/+1, until the
/// cleanup step ends the change. It entered this turn, so P0 has no attack to declare.
const SPELL: Scenario = Scenario::new("a spell's text change goes on with its permanent")
    .interaction(&[names::CRYSTAL_SPRAY, names::WILD_NACATL])
    .battlefield(
        P0,
        &[names::FOREST, names::ISLAND, names::ISLAND, names::ISLAND],
    )
    .hand(P0, &[names::WILD_NACATL, names::CRYSTAL_SPRAY])
    .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
    .actions(&[
        ME.casts(names::WILD_NACATL),
        ME.casts(names::CRYSTAL_SPRAY),
        ME.targets(names::WILD_NACATL),
        Resolve,
        ME.chooses_word(TextWord::LandType(Subtype::Mountain)),
        ME.chooses_word(TextWord::LandType(Subtype::Island)),
        Resolve,
        ME.pt(names::WILD_NACATL, 2, 2),
        advance_to(2, TurnStep::Upkeep),
        ME.pt(names::WILD_NACATL, 1, 1),
    ]);

#[test]
fn a_spells_text_change_goes_on_with_its_permanent() {
    beryl_scenario::run(&CARDS, &SPELL).unwrap_or_else(|e| panic!("{e}"));
}
