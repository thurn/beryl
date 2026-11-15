// Generated from the card store by `just card header`: notes go below the end marker.
//! Trinisphere {3}
//! Artifact
//!
//! As long as this artifact is untapped, each spell that would cost less than three mana to cast
//! costs three mana to cast. (Additional mana in the cost may be paid with any color of mana or
//! colorless mana. For example, a spell that would cost {1}{B} to cast costs {2}{B} to cast
//! instead.)
// End of the generated card header.

use crate::prelude::*;

/// "As long as this artifact is untapped, each spell that would cost less than three mana to
/// cast costs three mana to cast." A rule change for every player that sets a minimum total
/// cost. It affects the total cost directly, so it applies after every increase and
/// reduction ([CR 601.2f]): "Once the total cost is determined, any effects that directly
/// affect the total cost are applied." It counts the whole total, X at its value, so a
/// spell cast without paying its mana cost costs three; the difference is generic mana. "As
/// long as this artifact is untapped" is the ability's condition. It leaves the spell's mana value
/// as it was.
///
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
pub static CARD: CardDef = Printed::Trinisphere
    .abilities(&[Ability::rule_change(&RuleChange::spell_costs(
        Players::EachPlayer,
        Filter::Any,
        SpellCost::AtLeast(3),
    ))
    .only_if(&Condition::Is(Objects::This, Filter::Not(&Filter::Tapped)))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const TRINISPHERE: &str = Card::Trinisphere;
    const BOLT: &str = Card::LightningBolt;
    const THALIA: &str = Card::ThaliaGuardianOfThraben;
    const BRAINGEYSER: &str = Card::Braingeyser;
    const SNAPCASTER: &str = Card::SnapcasterMage;

    pub const SCENARIOS: &[Scenario] = &[
        // Lightning Bolt costs {2}{R}: P0's three Mountains pay it.
        Scenario::new("Trinisphere: a one-mana spell costs three")
            .interaction(&[BOLT])
            .battlefield(
                P0,
                &[TRINISPHERE, Card::Mountain, Card::Mountain, Card::Mountain],
            )
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 3),
                Resolve,
                THEM.life(17),
            ]),
        // Tapped, it changes nothing: the Bolt takes one Mountain. The opponent's Trinisphere,
        // so that P0's untap step can't untap it in an oracle that plays one.
        Scenario::new("Trinisphere: tapped, it changes no cost")
            .interaction(&[BOLT])
            .permanents(P1, &[Permanent::new(TRINISPHERE).tapped()])
            .battlefield(P0, &[Card::Mountain, Card::Mountain, Card::Mountain])
            .hand(P0, &[BOLT])
            .actions(&[
                THEM.tapped(TRINISPHERE, 1),
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 1),
                Resolve,
                THEM.life(17),
            ]),
        // The opponent's spells cost three too: P1's Counterspell costs {1}{U}{U}. The Plains
        // in P0's hand keeps P0's main phase a decision afterward, before P1's Islands untap.
        Scenario::new("Trinisphere: an opponent's Counterspell costs three")
            .interaction(&[BOLT])
            .battlefield(
                P0,
                &[TRINISPHERE, Card::Mountain, Card::Mountain, Card::Mountain],
            )
            .hand(P0, &[BOLT, Card::Plains])
            .battlefield(P1, &[Card::Island; 4])
            .hand(P1, &[Card::Counterspell])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                // The Bolt is the one target, and P0, with no mana left, only passes.
                THEM.casts(Card::Counterspell),
                THEM.tapped(Card::Island, 3),
                ME.has(Zone::Graveyard, BOLT, 1),
                THEM.life(20),
            ]),
        // X counts at its value: Braingeyser for X = 0 costs {U}{U} and is raised to three.
        Scenario::new("Trinisphere: X counts toward the three")
            .interaction(&[BRAINGEYSER])
            .battlefield(
                P0,
                &[
                    TRINISPHERE,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[BRAINGEYSER])
            .actions(&[
                ME.casts(BRAINGEYSER),
                ME.x(1),
                ME.targets_player(ME),
                // {1}{U}{U} is three already, so it costs no more.
                ME.tapped(Card::Island, 3),
                Resolve,
                ME.has_cards(Zone::Hand, 1),
            ]),
        // The ruling: the increase first, then the minimum. Bolt costs {1}{R} under Thalia,
        // less than three, so it costs {2}{R}, not four.
        Scenario::new("Trinisphere and Thalia: one more, then three")
            .interaction(&[BOLT, THALIA])
            .battlefield(
                P0,
                &[
                    TRINISPHERE,
                    THALIA,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                ],
            )
            .hand(P0, &[BOLT])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.tapped(Card::Mountain, 3),
                Resolve,
                THEM.life(17),
            ]),
        // An alternative cost is raised too. Snapcaster Mage costs {2}{U} (a creature, so
        // Thalia doesn't tax it) and gives the Bolt flashback; the flashback cost {R} is {1}{R}
        // under Thalia and {2}{R} under both.
        Scenario::new("Trinisphere and Thalia: flashback costs three")
            .interaction(&[BOLT, THALIA, SNAPCASTER])
            .battlefield(
                P0,
                &[
                    TRINISPHERE,
                    THALIA,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[BOLT])
            .actions(&[
                ME.casts(SNAPCASTER),
                Resolve,
                Resolve,
                ME.casts(BOLT),
                ME.targets_player(THEM),
                Resolve,
                THEM.life(17),
                ME.has(Zone::Exile, BOLT, 1),
                ME.tapped(Card::Mountain, 3),
            ]),
        // Fork costs {R}{R}, three under Trinisphere ({1}{R}{R}); the copy it makes is not cast
        // and costs nothing ([CR 707.10]). Bolt, Fork: six Mountains.
        Scenario::new("Trinisphere: Fork costs three and its copy is free")
            .interaction(&[BOLT, Card::Fork])
            .battlefield(
                P0,
                &[
                    TRINISPHERE,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                    Card::Mountain,
                ],
            )
            .hand(P0, &[BOLT, Card::Fork])
            .actions(&[
                ME.casts(BOLT),
                ME.targets_player(THEM),
                ME.casts(Card::Fork),
                ME.tapped(Card::Mountain, 6),
                Resolve,
                ME.targets_player(THEM),
                Resolve,
                Resolve,
                THEM.life(14),
            ]),
        // Trinisphere affects spells only: an activated ability costing {2} still costs {2}.
        Scenario::new("Trinisphere does not raise an activated ability")
            .interaction(&[Card::ThespiansStage])
            .battlefield(
                P0,
                &[
                    TRINISPHERE,
                    Card::ThespiansStage,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                ],
            )
            .actions(&[
                ME.activates(Card::ThespiansStage, 1),
                ME.targets(Card::Plains),
                ME.tapped(Card::Island, 2),
                Resolve,
                ME.controls(Card::Plains, 2),
            ]),
    ];
}
