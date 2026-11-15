//! Text changes no oracle can check, played as scenarios here instead of in a card's file.
//! `card diverge` records an oracle that is wrong only when the other oracle passes, and
//! `XMage` has no Mind Bend and no Crystal Spray, so where Forge is wrong too nothing can be
//! recorded.
//!
//! A text change on a permanent spell goes on with the permanent the spell becomes
//! ([CR 400.7a]): "Effects from spells, activated abilities, and triggered abilities that change
//! the characteristics or controller of a permanent spell on the stack continue to apply to the
//! permanent that spell becomes." Crystal Spray's ruling says it of the card: "If you change the
//! text of a spell which is to become a permanent, the permanent will retain the text change
//! until the effect wears off." Forge drops the change as the spell resolves.
//!
//! A text change of a land type word changes it wherever the text uses it as a land type
//! ([CR 612.2]): "A text-changing effect changes only those words that are used in the correct
//! way (for example, a Magic color word being used as a color word, a land type word used as a
//! land type, or a creature type word used as a creature type)." Forge changes Dandân's "Island"
//! in its sacrifice trigger but not in its attack restriction.
//!
//! A copy exception's color is a color word of the copier's text ([CR 707.9b]): "Some copy
//! effects modify a characteristic as part of the copying process." Forge makes Fork's copy red
//! after Fork's "red" is changed to "blue".
//!
//! [CR 400.7a]: https://yawgatog.com/resources/magic-rules/#R4007a
//! [CR 612.2]: https://yawgatog.com/resources/magic-rules/#R6122
//! [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b

use beryl_card::{Colors, Subtype, TextWord, TurnStep, Zone};
use beryl_scenario::Scenario;
use beryl_scenario_types::prelude::{ME, P0, P1, Resolve, THEM, advance_to};

use crate::{CARDS, names};

const ISLAND: TextWord = TextWord::LandType(Subtype::Island);
const SWAMP: TextWord = TextWord::LandType(Subtype::Swamp);

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
        ME.chooses_word(ISLAND),
        Resolve,
        ME.pt(names::WILD_NACATL, 2, 2),
        advance_to(2, TurnStep::Upkeep),
        ME.pt(names::WILD_NACATL, 1, 1),
    ]);

#[test]
fn a_spells_text_change_goes_on_with_its_permanent() {
    beryl_scenario::run(&CARDS, &SPELL).unwrap_or_else(|e| panic!("{e}"));
}

/// Mind Bend makes Dandân's "Island" read "Swamp": P0's Swamp keeps Dandân, and it attacks
/// P1, who controls a Swamp and no Island.
const MIND_BEND_ON_DANDAN: Scenario =
    Scenario::new("Mind Bend lets Dandân attack a player who controls a Swamp")
        .interaction(&[names::DANDAN, names::MIND_BEND])
        .battlefield(P0, &[names::DANDAN, names::ISLAND, names::SWAMP])
        .battlefield(P1, &[names::SWAMP])
        .hand(P0, &[names::MIND_BEND])
        .actions(&[
            ME.casts(names::MIND_BEND),
            ME.targets(names::DANDAN),
            Resolve,
            ME.chooses_word(ISLAND),
            ME.chooses_word(SWAMP),
            ME.controls(names::DANDAN, 1),
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::DANDAN),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(16),
        ]);

/// Crystal Spray makes the same change until end of turn.
const CRYSTAL_SPRAY_ON_DANDAN: Scenario =
    Scenario::new("Crystal Spray lets Dandân attack a player who controls a Swamp")
        .interaction(&[names::DANDAN, names::CRYSTAL_SPRAY])
        .battlefield(
            P0,
            &[
                names::DANDAN,
                names::ISLAND,
                names::ISLAND,
                names::ISLAND,
                names::SWAMP,
            ],
        )
        .battlefield(P1, &[names::SWAMP])
        .hand(P0, &[names::CRYSTAL_SPRAY])
        .library(P0, &[names::ISLAND, names::ISLAND])
        .actions(&[
            ME.casts(names::CRYSTAL_SPRAY),
            ME.targets(names::DANDAN),
            Resolve,
            ME.chooses_word(ISLAND),
            ME.chooses_word(SWAMP),
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::DANDAN),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(16),
        ]);

#[test]
fn a_text_change_moves_dandans_attack_restriction_to_the_new_land_type() {
    for scenario in [&MIND_BEND_ON_DANDAN, &CRYSTAL_SPRAY_ON_DANDAN] {
        beryl_scenario::run(&CARDS, scenario).unwrap_or_else(|e| panic!("{e}"));
    }
}

/// P1's Lightning Bolt aims at P0. P0 Forks it, then Crystal Spray changes Fork's "red" to
/// "blue" before Fork resolves, so Fork's copy, which aims at P1, is blue and the Bolt red.
const CRYSTAL_SPRAY_ON_FORK: Scenario = Scenario::new("Crystal Spray makes Fork's copy blue")
    .interaction(&[names::CRYSTAL_SPRAY, names::FORK, names::LIGHTNING_BOLT])
    .battlefield(
        P0,
        &[
            names::MOUNTAIN,
            names::MOUNTAIN,
            names::ISLAND,
            names::ISLAND,
            names::ISLAND,
        ],
    )
    .hand(P0, &[names::FORK, names::CRYSTAL_SPRAY])
    .library(P0, &[names::ISLAND, names::ISLAND, names::ISLAND])
    .battlefield(P1, &[names::MOUNTAIN])
    .hand(P1, &[names::LIGHTNING_BOLT])
    .at(2, P1, TurnStep::PrecombatMain)
    .actions(&[
        THEM.casts(names::LIGHTNING_BOLT),
        THEM.targets_player(ME),
        ME.casts(names::FORK),
        ME.casts(names::CRYSTAL_SPRAY),
        ME.targets(names::FORK),
        Resolve,
        ME.chooses_word(TextWord::Color(Colors::RED)),
        ME.chooses_word(TextWord::Color(Colors::BLUE)),
        Resolve,
        ME.targets_player(THEM),
        ME.colored(Zone::Stack, names::LIGHTNING_BOLT, Colors::BLUE),
        THEM.colored(Zone::Stack, names::LIGHTNING_BOLT, Colors::RED),
        Resolve,
        THEM.life(17),
    ]);

#[test]
fn a_text_change_on_fork_changes_its_copys_color() {
    beryl_scenario::run(&CARDS, &CRYSTAL_SPRAY_ON_FORK).unwrap_or_else(|e| panic!("{e}"));
}
