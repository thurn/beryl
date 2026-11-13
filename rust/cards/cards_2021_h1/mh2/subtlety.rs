// Generated from the card store by `just card header`: notes go below the end marker.
//! Subtlety {2}{U}{U}
//! Creature — Elemental Incarnation, 3/3
//!
//! Flash
//! Flying
//! When this creature enters, choose up to one target creature spell or planeswalker spell. Its
//! owner puts it on their choice of the top or bottom of their library.
//! Evoke—Exile a blue card from your hand.
// End of the generated card header.

use crate::prelude::*;

/// The spell leaves the stack without resolving, to the library of its owner, who chooses
/// the end ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object
/// would go to any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." The trigger resolves whether or not Subtlety is still there, as after
/// evoke's sacrifice ([CR 113.7a](https://yawgatog.com/resources/magic-rules/#R1137a)): "Once
/// activated or triggered, an ability exists on the stack independently of its source."
///
/// Evoke's cost exiles a blue card from the hand rather than paying {2}{U}{U}
/// ([CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)), and its sacrifice
/// trigger enters alongside the tuck, both Subtlety's controller's to order
/// ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b)).
pub static CARD: CardDef = Printed::Subtlety
    .keywords(Keywords::FLASH.union(Keywords::FLYING))
    .abilities(&[
        Ability::triggered(&Trigger::ENTERS)
            .effect(&put_into_library(target(), LibraryPosition::TopOrBottom))
            .target(&TargetSpec::up_to_one(
                TargetKind::Spell,
                Filter::Types(Types::CREATURE.union(Types::PLANESWALKER)),
            )),
        Ability::evoke(),
    ])
    .casting(&[CastingOption::Evoke {
        cost: ManaCost::NONE,
        parts: &[CostPart::exile(1, Zone::Hand, Filter::Colors(Colors::BLUE))],
    }])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    pub const SCENARIOS: &[Scenario] = &[
        // Cast for {2}{U}{U} in response to P1's Elite Spellbinder, which P1 puts on top. The
        // Plains in P1's hand keeps P1's main phase a decision once the stack is empty.
        Scenario::new("Subtlety hard cast: the creature spell's owner puts it on top")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Island; 4])
            .hand(P0, &[Card::Subtlety])
            .battlefield(P1, &[Card::Plains; 3])
            .hand(P1, &[Card::EliteSpellbinder, Card::Plains])
            .actions(&[
                THEM.casts(Card::EliteSpellbinder),
                // Passing and the cast for mana: with no other blue card in hand, no evoke.
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Card::Subtlety),
                Resolve,
                ME.target_count(1),
                Resolve,
                THEM.yes(),
                THEM.library_top(&[Card::EliteSpellbinder, Card::Island]),
                stack(0),
                ME.controls(Card::Subtlety, 1),
            ]),
        // Evoked with no mana, exiling Timetwister, against P1's Teferi. P0 orders the triggers
        // so the sacrifice resolves first; the tuck resolves anyway, and P1 puts Teferi on the
        // bottom. The planeswalker spell is a named card because the `XMage` harness fails this
        // scenario with the planeswalker prop on the stack, though it plays it with Teferi.
        Scenario::new("Subtlety evoked: sacrificed, its trigger still tucks a planeswalker spell")
            .interaction(&[Card::TeferiTimeRaveler])
            .at(2, P1, TurnStep::PrecombatMain)
            .hand(P0, &[Card::Subtlety, Prop::UntargetedSpell])
            .battlefield(P1, &[Card::Plains, Card::Island, Card::Island])
            .hand(P1, &[Card::TeferiTimeRaveler, Card::Plains])
            .actions(&[
                THEM.casts(Card::TeferiTimeRaveler),
                Action::Cast {
                    player: P0,
                    card: Card::Subtlety,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
                Resolve,
                ME.chooses(DecisionKind::TriggerOrder, Card::Subtlety),
                ME.target_count(1),
                // The sacrifice: the tuck and Teferi are left.
                Resolve,
                stack(2),
                Resolve,
                THEM.no(),
                THEM.library_top(&[Card::Island, Card::TeferiTimeRaveler]),
                ME.has(Zone::Graveyard, Card::Subtlety, 1),
            ]),
        // With mana for its cost and two other blue cards in hand, both casts are offered; evoked,
        // P0 picks which blue card to exile.
        Scenario::new("Subtlety with mana and blue cards: cast or evoke, then which card to exile")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Island; 4])
            .hand(
                P0,
                &[Card::Subtlety, Prop::UntargetedSpell, Prop::Planeswalker],
            )
            .battlefield(P1, &[Card::Plains; 3])
            .hand(P1, &[Card::EliteSpellbinder, Card::Plains])
            .actions(&[
                THEM.casts(Card::EliteSpellbinder),
                // Passing, the cast for mana and the evoke.
                ME.deciding(DecisionKind::Priority, 3),
                Action::Cast {
                    player: P0,
                    card: Card::Subtlety,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                ME.deciding(DecisionKind::ChooseObject, 2),
                ME.picks(Prop::UntargetedSpell),
                ME.has(Zone::Exile, Prop::UntargetedSpell, 1),
                Resolve,
                ME.chooses(DecisionKind::TriggerOrder, Card::Subtlety),
                ME.target_count(1),
                Resolve,
                Resolve,
                THEM.yes(),
                THEM.library_top(&[Card::EliteSpellbinder, Card::Island]),
                ME.has(Zone::Graveyard, Card::Subtlety, 1),
            ]),
    ];
}
