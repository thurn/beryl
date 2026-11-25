// Generated from the card store by `just card header`: notes go below the end marker.
//! Thalia, Guardian of Thraben {1}{W}
//! Legendary Creature — Human Soldier, 2/1
//!
//! First strike
//! Noncreature spells cost {1} more to cast.
// End of the generated card header.

use crate::prelude::*;

/// The increase also applies to alternative costs, including flashback ([CR 601.2f]).
/// It leaves the spell's mana value unchanged.
///
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
pub static CARD: CardDef = Printed::ThaliaGuardianOfThraben
    .keywords(Keywords::FIRST_STRIKE)
    .abilities(&[Ability::rule_change(&RuleChange::spell_costs(
        Players::EachPlayer,
        Filter::Not(&Filter::Types(Types::CREATURE)),
        SpellCost::More(1),
    ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const THALIA: &str = Card::ThaliaGuardianOfThraben;
    const BOLT: &str = Card::LightningBolt;
    const MOTHER: &str = Card::MotherOfRunes;

    pub const SCENARIOS: &[Scenario] = &[
        // Lightning Bolt costs {1}{R}: two of P0's three Mountains pay it.
        Scenario::new("Thalia: Lightning Bolt costs one more")
            .interaction(&[BOLT])
            .battlefield(
                P0,
                &[THALIA, Card::Mountain, Card::Mountain, Card::Mountain],
            )
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 2),
                Resolve,
                THEM.life(17),
            ]),
        // The second ruling: the opponent's spells cost more too. P1's Counterspell costs
        // {1}{U}{U}, three of their four Islands. The Plains in P0's hand keeps P0's main
        // phase a decision afterward, before P1's Islands untap.
        Scenario::new("Thalia: an opponent's Counterspell costs one more")
            .interaction(&[BOLT])
            .battlefield(P0, &[THALIA, Card::Mountain, Card::Mountain])
            .hand(P0, &[BOLT, Card::Plains])
            .battlefield(P1, &[Card::Island; 4])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                // The Bolt is the one target, and P0, with no mana left, only passes, so the
                // Counterspell resolves.
                THEM.casts(Card::Counterspell),
                THEM.tapped(Card::Island, 3),
                ME.has(Zone::Graveyard, BOLT, 1),
                THEM.life(20),
            ]),
        // A creature spell costs what it did: Mother of Runes takes one Plains.
        Scenario::new("Thalia: a creature spell costs no more")
            .interaction(&[MOTHER])
            .battlefield(P0, &[THALIA, Card::Plains, Card::Plains])
            .hand(P0, &[MOTHER])
            .actions(&[
                ME.casts(MOTHER),
                ME.tapped(Card::Plains, 1),
                Resolve,
                ME.controls(MOTHER, 1),
            ]),
        // Thalia adds one generic mana to Force's alternative cost, which
        // starts with no mana. Helm can then remove exactly that mana.
        Scenario::new("Thalia and Helm adjust Force of Will's alternative cost")
            .interaction(&[Card::ForceOfWill, Card::HelmOfAwakening])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[THALIA, Card::HelmOfAwakening])
            .hand(P0, &[Card::ForceOfWill, Prop::UntargetedSpell])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Prop::Artifact, Card::Plains])
            .actions(&[
                THEM.casts(Prop::Artifact),
                Action::Cast {
                    player: P0,
                    card: Card::ForceOfWill,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.life(19),
                ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
                THEM.has(Zone::Graveyard, Prop::Artifact, 1),
            ]),
        // Mana Drain reads Bolt's printed mana value even though Thalia taxes it.
        Scenario::new("Thalia increases the cost without changing mana value")
            .interaction(&[BOLT, Card::ManaDrain])
            .battlefield(P0, &[THALIA, Card::Mountain, Card::Mountain])
            .hand(P0, &[BOLT])
            .battlefield(P1, &[Card::Island; 3])
            .hand(P1, &[Card::ManaDrain, Card::Plains])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 2),
                THEM.casts(Card::ManaDrain),
                ME.has(Zone::Graveyard, BOLT, 1),
                ME.done_attacking(),
                advance_to(2, TurnStep::PrecombatMain),
                THEM.passes(),
                THEM.mana(ManaColor::Colorless, 1),
            ]),
    ];
}
