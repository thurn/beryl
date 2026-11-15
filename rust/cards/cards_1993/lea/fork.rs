// Generated from the card store by `just card header`: notes go below the end marker.
//! Fork {R}{R}
//! Instant
//!
//! Copy target instant or sorcery spell, except that the copy is red. You may choose new targets
//! for the copy.
// End of the generated card header.

use crate::prelude::*;

/// "Copy target instant or sorcery spell, except that the copy is red. You may choose new targets
/// for the copy." The copy has the spell's targets, X and costs paid, and is not cast
/// ([CR 707.10]): "a copy of a spell isn't cast ... A copy of a spell or ability copies both the
/// characteristics of the spell or ability and all decisions made for it, including modes,
/// targets, the value of X, and additional or alternative costs." Its color is part of its
/// copiable values ([CR 707.9b]): "Some copy effects modify a characteristic as part of the
/// copying process."
///
/// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
pub static CARD: CardDef = Printed::Fork
    .abilities(&[Ability::spell()
        .target(&TargetSpec::one(
            TargetKind::Spell,
            Filter::Types(Types::INSTANT.union(Types::SORCERY)),
        ))
        .effect(&copy_spell_except(target(), ExceptIx::new(0)))])
    .excepts(&[CopyException::RED])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BOLT: &str = Card::LightningBolt;
    const FORK: &str = Card::Fork;
    const GEYSER: &str = Card::Braingeyser;
    const ISLAND: &str = Card::Island;
    const MOUNTAIN: &str = Card::Mountain;

    pub const SCENARIOS: &[Scenario] = &[
        // The opponent's Bolt aims at P0; the copy, which P0 controls, aims at the opponent.
        Scenario::new("Fork copies Lightning Bolt with a new target")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Mountain, Card::Mountain])
            .hand(P0, &[FORK])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[BOLT])
            .actions(&[
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                ME.casts(FORK),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                THEM.life(17),
                Resolve,
                ME.life(17),
                THEM.has(Zone::Graveyard, BOLT, 1),
                ME.has(Zone::Graveyard, BOLT, 0),
            ]),
        // Keeping the target: both Bolts hit P0.
        Scenario::new("Fork copies Lightning Bolt with the same target")
            .interaction(&[BOLT])
            .battlefield(P0, &[Card::Mountain, Card::Mountain])
            .hand(P0, &[FORK])
            .battlefield(P1, &[Card::Mountain])
            .hand(P1, &[BOLT])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(BOLT),
                THEM.targets_player(ME),
                ME.casts(FORK),
                Resolve,
                ME.targets_player(ME),
                Resolve,
                Resolve,
                ME.life(14),
                THEM.life(20),
            ]),
        // The copy keeps X, is red, and draws for the player Fork's controller chooses. A
        // Bolt left in hand keeps priority with Fork's controller while the copy is looked at.
        Scenario::new("Fork copies Braingeyser for 3 with a new target")
            .interaction(&[GEYSER, BOLT])
            .battlefield(P0, &[MOUNTAIN, MOUNTAIN, MOUNTAIN])
            .hand(P0, &[FORK, BOLT])
            .library(P0, &[Card::Plains; 4])
            .battlefield(P1, &[Card::Island; 5])
            .hand(P1, &[GEYSER])
            .library(P1, &[Card::Plains; 4])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(GEYSER),
                THEM.x(3),
                THEM.targets_player(THEM),
                ME.casts(FORK),
                Resolve,
                ME.targets_player(ME),
                ME.colored(Zone::Stack, GEYSER, Colors::RED),
                THEM.colored(Zone::Stack, GEYSER, Colors::BLUE),
                Resolve,
                ME.has(Zone::Hand, Card::Plains, 3),
                Resolve,
                THEM.has(Zone::Hand, Card::Plains, 3),
                ME.has(Zone::Hand, BOLT, 1),
            ]),
        // The copy's effect exiles the copy first, and the copy is still in exile while each
        // player wheels and its controller untaps lands; then it ceases to exist (CR 704.5e).
        // The spell resolves after it, for its own controller, and stays in exile.
        Scenario::new("Fork copies Time Spiral, which exiles the copy first")
            .interaction(&[Card::TimeSpiral])
            .battlefield(P0, &[MOUNTAIN, MOUNTAIN])
            .hand(P0, &[FORK])
            .library(P0, &[Card::Plains; 16])
            .battlefield(P1, &[ISLAND, ISLAND, ISLAND, ISLAND, ISLAND, ISLAND])
            .hand(P1, &[Card::TimeSpiral])
            .library(P1, &[Card::Island; 16])
            .at(2, P1, TurnStep::PrecombatMain)
            .actions(&[
                THEM.casts(Card::TimeSpiral),
                ME.casts(FORK),
                Resolve,
                Resolve,
                ME.deciding(DecisionKind::ChooseUpTo, 3),
                ME.picks_up_to(MOUNTAIN),
                ME.done_picking(),
                ME.tapped(MOUNTAIN, 1),
                ME.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Exile, 0),
                Resolve,
                THEM.picks_up_to(ISLAND),
                THEM.done_picking(),
                THEM.tapped(ISLAND, 5),
                THEM.has(Zone::Exile, Card::TimeSpiral, 1),
                THEM.has_cards(Zone::Hand, 7),
                ME.has_cards(Zone::Hand, 7),
            ]),
        // A copy is countered on its own, and like the spell it leaves no card behind.
        Scenario::new("Fork's copy and its spell are countered one at a time")
            .interaction(&[GEYSER])
            .battlefield(P0, &[ISLAND, ISLAND, ISLAND, MOUNTAIN, MOUNTAIN])
            .hand(P0, &[GEYSER, FORK])
            .library(P0, &[Card::Plains; 4])
            .battlefield(P1, &[Card::Island; 4])
            .hand(P1, &[Card::Counterspell, Card::Counterspell])
            .actions(&[
                ME.casts(GEYSER),
                ME.x(1),
                ME.targets_player(ME),
                ME.casts(FORK),
                Resolve,
                ME.targets_player(ME),
                THEM.casts(Card::Counterspell),
                THEM.targets(GEYSER),
                Resolve,
                stack(1),
                ME.passes(),
                THEM.casts(Card::Counterspell),
                stack(0),
                ME.has(Zone::Hand, Card::Plains, 0),
                ME.has(Zone::Graveyard, GEYSER, 1),
            ]),
    ];
}
