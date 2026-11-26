//! Text changes no oracle can check, played as scenarios here instead of in a card's file.
//! `card diverge` records an oracle that is wrong only when the other oracle passes, and
//! `XMage` has no Mind Bend and no Crystal Spray, so where Forge is wrong too nothing can be
//! recorded. Magical Hack's attack and duration scenarios with Dandân are here with them, since
//! Forge's Dandân, below, attacks as printed.
//!
//! A text change on a permanent spell goes on with the permanent the spell becomes
//! ([CR 400.7a]): "Effects from spells, activated abilities, and triggered abilities that change
//! the characteristics or controller of a permanent spell on the stack continue to apply to the
//! permanent that spell becomes." Crystal Spray's ruling says it of the card: "If you change the
//! text of a spell which is to become a permanent, the permanent will retain the text change
//! until the effect wears off." Forge drops the change as the spell resolves.
//!
//! A text change can invalidate a target during resolution ([CR 608.2b]): “Other changes to the
//! game state may cause a target to no longer be legal; for example, its characteristics may have
//! changed or an effect may have changed the text of the spell.” Forge still destroys the
//! creature Deathmark targeted before Crystal Spray changed its text.
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
//! [CR 608.2b]: https://yawgatog.com/resources/magic-rules/#R6082b
//! [CR 612.2]: https://yawgatog.com/resources/magic-rules/#R6122
//! [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b

use beryl_card::{Colors, Subtype, TextWord, TurnStep, Zone};
use beryl_engine::DecisionKind;
use beryl_scenario::Scenario;
use beryl_scenario_types::prelude::{ME, P0, P1, Resolve, THEM, advance_to};

use crate::props::Prop;
use crate::{CARDS, names};

const ISLAND: TextWord = TextWord::LandType(Subtype::Island);
const SWAMP: TextWord = TextWord::LandType(Subtype::Swamp);

#[test]
fn a_spells_text_change_goes_on_with_its_permanent() {
    beryl_scenario::run(
        &CARDS,
        &crate::library::continuous::TEXT_CHANGE_ON_PERMANENT_SPELL,
    )
    .unwrap_or_else(|e| panic!("{e}"));
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

/// Magical Hack makes the same change, and it lasts: Dandân attacks P1 again on P0's next
/// turn.
const MAGICAL_HACK_ON_DANDAN: Scenario =
    Scenario::new("Magical Hack lets Dandân attack a player with a Swamp on later turns")
        .interaction(&[names::DANDAN, names::MAGICAL_HACK])
        .battlefield(P0, &[names::DANDAN, names::ISLAND, names::SWAMP])
        .battlefield(P1, &[names::SWAMP])
        .hand(P0, &[names::MAGICAL_HACK])
        .library(P0, &[names::PLAINS])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(names::MAGICAL_HACK),
            ME.targets(names::DANDAN),
            Resolve,
            ME.chooses_word(ISLAND),
            ME.chooses_word(SWAMP),
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::DANDAN),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(16),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::DANDAN),
            advance_to(3, TurnStep::EndOfCombat),
            THEM.life(12),
        ]);

/// Crystal Spray makes the same change until end of turn. By P0's next turn the change has
/// ended and Dandân again needs the defending player to control an Island, so it can't attack
/// P1, who controls only a Swamp: the attack question offers the flier and not Dandân.
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
                Prop::Flier,
            ],
        )
        .battlefield(P1, &[names::SWAMP])
        .hand(P0, &[names::CRYSTAL_SPRAY])
        .library(P0, &[names::PLAINS, names::PLAINS])
        .library(P1, &[names::PLAINS])
        .actions(&[
            ME.casts(names::CRYSTAL_SPRAY),
            ME.targets(names::DANDAN),
            Resolve,
            ME.chooses_word(ISLAND),
            ME.chooses_word(SWAMP),
            advance_to(1, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.attacks_with(names::DANDAN),
            ME.done_attacking(),
            advance_to(1, TurnStep::EndOfCombat),
            THEM.life(16),
            advance_to(3, TurnStep::BeginningOfCombat),
            ME.passes(),
            ME.deciding(DecisionKind::Attacker, 2),
        ]);

#[test]
fn a_text_change_moves_dandans_attack_restriction_to_the_new_land_type() {
    for scenario in [
        &MIND_BEND_ON_DANDAN,
        &MAGICAL_HACK_ON_DANDAN,
        &CRYSTAL_SPRAY_ON_DANDAN,
    ] {
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

#[test]
fn a_text_change_can_invalidate_a_spells_target() {
    let scenario = &crate::library::continuous::TEXT_CHANGE_INVALIDATES_TARGET;
    beryl_scenario::run(&CARDS, scenario).unwrap_or_else(|e| panic!("{e}"));
}
