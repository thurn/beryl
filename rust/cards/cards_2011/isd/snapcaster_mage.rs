// Generated from the card store by `just card header`: notes go below the end marker.
//! Snapcaster Mage {1}{U}
//! Creature — Human Wizard, 2/1
//!
//! Flash
//! When this creature enters, target instant or sorcery card in your graveyard gains flashback
//! until end of turn. The flashback cost is equal to its mana cost. (You may cast that card from
//! your graveyard for its flashback cost. Then exile it.)
// End of the generated card header.

use crate::prelude::*;

/// Flashback for the card's mana cost, an alternative cost ([CR 702.34a]): "'Flashback [cost]'
/// means 'You may cast this card from your graveyard if the resulting spell is an instant or
/// sorcery spell by paying [cost] rather than paying its mana cost' and 'If the flashback cost was
/// paid, exile this card instead of putting it anywhere else any time it would leave the stack.'"
/// The card is in its owner's graveyard, and its owner is the trigger's controller.
///
/// With no instant or sorcery card in its controller's graveyard the trigger has no target, and is
/// removed from the stack ([CR 603.3d]).
///
/// [CR 702.34a]: https://yawgatog.com/resources/magic-rules/#R70234a
/// [CR 603.3d]: https://yawgatog.com/resources/magic-rules/#R6033d
pub static CARD: CardDef = Printed::SnapcasterMage
    .keywords(Keywords::FLASH)
    .abilities(&[Ability::triggered(&Trigger::ENTERS)
        .target(&TargetSpec::graveyard_card(Filter::All(&[
            Filter::Types(Types::INSTANT.union(Types::SORCERY)),
            Filter::Whose(&Players::You),
        ])))
        .effect(&gain_flashback(target()))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SNAPCASTER: &str = Card::SnapcasterMage;

    const SPELLBINDER: &str = Card::EliteSpellbinder;

    pub const SCENARIOS: &[Scenario] = &[
        // Flashed in against P1's Timetwister, it gives the Counterspell in P0's graveyard
        // flashback; Counterspell counters Timetwister and is exiled rather than going back to
        // the graveyard. The Plains in P1's hand keeps P1's main phase a decision afterward.
        Scenario::new("Snapcaster Mage: flashback a counterspell, then exile it")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Island; 4])
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Card::Counterspell])
            .battlefield(P1, &[Card::Island; 3])
            .hand(P1, &[Card::Timetwister, Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                THEM.casts(Card::Timetwister),
                ME.casts(SNAPCASTER),
                // Snapcaster, then its trigger, which no one can respond to.
                Resolve,
                THEM.passes(),
                // Counterspell resolves at once: neither player can respond.
                ME.casts(Card::Counterspell),
                THEM.has(Zone::Graveyard, Card::Timetwister, 1),
                ME.has(Zone::Exile, Card::Counterspell, 1),
                ME.has(Zone::Graveyard, Card::Counterspell, 0),
            ]),
        // P1 answers the flashback Counterspell with Sink into Stupor, which would return it to
        // P0's hand: cast with flashback, it is exiled instead (CR 702.34a), and the creature
        // resolves.
        Scenario::new("Snapcaster Mage: a flashback spell returned to hand is exiled")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Island; 4])
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Card::Counterspell])
            .battlefield(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P1, &[Prop::Creature, Prop::Removal, Card::Plains])
            .library(P1, &[Card::Island])
            .actions(&[
                THEM.casts(Prop::Creature),
                ME.casts(SNAPCASTER),
                // Snapcaster resolves; P1, who could cast Sink into Stupor, passes as its trigger
                // resolves and again with the creature alone on the stack.
                Resolve,
                THEM.passes(),
                THEM.passes(),
                ME.casts(Card::Counterspell),
                THEM.casts(Prop::Removal),
                THEM.targets(Card::Counterspell),
                Resolve,
                ME.has(Zone::Exile, Card::Counterspell, 1),
                ME.has(Zone::Hand, Card::Counterspell, 0),
                Resolve,
                THEM.controls(Prop::Creature, 1),
            ]),
        // Damn given flashback is cast for its mana cost only: the flashback cost is the
        // alternative cost, so overload can't be added (CR 118.9a), though P0 could pay it. The
        // two candidates are passing and the {B}{B} cast.
        Scenario::new("Snapcaster Mage: flashback is the one alternative cost")
            .interaction(&[Card::Damn])
            .battlefield(
                P0,
                &[
                    Card::Island,
                    Card::Island,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Card::Damn])
            .battlefield(P1, &[SPELLBINDER])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(SNAPCASTER),
                Resolve,
                Resolve,
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Card::Damn),
                ME.targets(SPELLBINDER),
                Resolve,
                THEM.has(Zone::Graveyard, SPELLBINDER, 1),
                ME.has(Zone::Exile, Card::Damn, 1),
            ]),
        // Oust gains flashback on turn 1 and P0 leaves it; by P0's next turn it has none.
        Scenario::new("Snapcaster Mage: flashback ends with the turn")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Card::Oust])
            .library(P0, &[Prop::Instant])
            .library(P1, &[Prop::Instant])
            .actions(&[
                ME.casts(SNAPCASTER),
                Resolve,
                Resolve,
                ME.deciding(DecisionKind::Priority, 2),
                advance_to(3, TurnStep::PrecombatMain),
                ME.deciding(DecisionKind::Priority, 1),
                ME.has(Zone::Graveyard, Card::Oust, 1),
            ]),
        // Nothing in P0's graveyard is an instant or sorcery: the trigger has no target and does
        // nothing.
        Scenario::new("Snapcaster Mage with no instant or sorcery to target")
            .battlefield(P0, &[Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Card::Plains, Prop::Creature])
            .library(P1, &[Card::Island])
            .actions(&[
                ME.casts(SNAPCASTER),
                Resolve,
                stack(0),
                ME.deciding(DecisionKind::Priority, 1),
                ME.controls(SNAPCASTER, 1),
            ]),
    ];
}
