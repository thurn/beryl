// Generated from the card store by `just card header`: notes go below the end marker.
//! Lurrus of the Dream-Den {1}{W/B}{W/B}
//! Legendary Creature — Cat Nightmare, 3/2
//!
//! Companion — Each permanent card in your starting deck has mana value 2 or less. (If this card is
//! your chosen companion, you may put it into your hand from outside the game for {3} as a
//! sorcery.)
//! Lifelink
//! Once during each of your turns, you may cast a permanent spell with mana value 2 or less from
//! your graveyard.
// End of the generated card header.

use crate::prelude::*;

const PERMANENT: Filter = Filter::Types(Types::PERMANENT);

pub static CARD: CardDef = Printed::LurrusOfTheDreamDen
    .keywords(Keywords::LIFELINK)
    .abilities(&[
        Ability::companion(&DeckCondition {
            each: PERMANENT,
            has: Filter::ManaValueAtMost(2),
        }),
        Ability::cast_from_graveyard_each_turn(&Filter::All(&[
            PERMANENT,
            Filter::ManaValueAtMost(2),
        ])),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const LURRUS: &str = Card::LurrusOfTheDreamDen;
    /// Lurrus revealed as P0's companion, with six Plains to pay for the action and the card.
    const REVEALED: Scenario = Scenario::setup()
        .sideboard(P0, &[LURRUS])
        .companion(P0, LURRUS)
        .battlefield(P0, &[Card::Plains; 6]);
    /// Lurrus on P0's battlefield, a small and a large creature, an instant and a land in
    /// their graveyard, and Islands to cast any of them.
    const ON_BATTLEFIELD: Scenario = Scenario::setup()
        .battlefield(
            P0,
            &[
                LURRUS,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Plains,
                Card::Plains,
                Card::Plains,
            ],
        )
        .graveyard(
            P0,
            &[
                Prop::Vanilla,
                Prop::Vanilla,
                Prop::Flier,
                Prop::Regeneration,
                Card::Island,
            ],
        );

    pub const SCENARIOS: &[Scenario] = &[
        REVEALED
            .titled("pay {3} to put the companion into hand, then cast it")
            .battlefield(
                P0,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                ],
            )
            .graveyard(P0, &[Prop::Vanilla])
            .actions(&[
                // Passing, or the companion action: the mana abilities are not candidates, and
                // Lurrus outside the game gives no cast from the graveyard.
                ME.deciding(DecisionKind::Priority, 2),
                ME.takes_companion(),
                stack(0),
                ME.has(Zone::Hand, LURRUS, 1),
                ME.tapped(Card::Plains, 3),
                ME.casts(LURRUS),
                Resolve,
                ME.controls(LURRUS, 1),
                ME.casts(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
            ]),
        REVEALED
            .titled("the companion action waits for its player's main phase")
            .at(1, P0, TurnStep::Upkeep)
            .hand(P0, &[Prop::Regeneration])
            .battlefield(
                P0,
                &[
                    Prop::Vanilla,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .actions(&[
                // Passing, or the instant.
                ME.deciding(DecisionKind::Priority, 2),
                advance_to(1, TurnStep::PrecombatMain),
                // And the companion action.
                ME.deciding(DecisionKind::Priority, 3),
                ME.takes_companion(),
                ME.has(Zone::Hand, LURRUS, 1),
            ]),
        REVEALED
            .titled("the companion action waits for an empty stack")
            .hand(P0, &[Prop::Regeneration, Prop::Regeneration])
            .battlefield(
                P0,
                &[
                    Prop::Vanilla,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .actions(&[
                ME.casts(Prop::Regeneration),
                // Passing, or the second instant.
                ME.deciding(DecisionKind::Priority, 2),
                Resolve,
                // And the companion action.
                ME.deciding(DecisionKind::Priority, 3),
                ME.takes_companion(),
                ME.has(Zone::Hand, LURRUS, 1),
            ]),
        REVEALED
            .titled("instants and sorceries of any mana value let the deck reveal it")
            .hand(P0, &[Prop::UntargetedSpell, Prop::Removal])
            .sideboard(P0, &[LURRUS, Prop::Flier])
            .actions(&[ME.takes_companion(), ME.has(Zone::Hand, LURRUS, 1)]),
        Scenario::setup()
            .titled("in the main deck it is an ordinary card")
            .hand(P0, &[LURRUS])
            .battlefield(P0, &[Card::Plains, Card::Swamp, Card::Swamp])
            .actions(&[
                // Passing, or casting it from hand: no companion action.
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(LURRUS),
                Resolve,
                ME.controls(LURRUS, 1),
            ]),
        ON_BATTLEFIELD
            .titled("cast one small permanent spell from the graveyard a turn")
            .actions(&[
                // Passing, or the two-mana creature: not the three-mana creature, the instant
                // or the land.
                ME.deciding(DecisionKind::Priority, 2),
                ME.casts(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
                // The permission is used: only passing is left.
                ME.deciding(DecisionKind::Priority, 1),
            ]),
        ON_BATTLEFIELD
            .titled("a graveyard spell keeps its own timing")
            .at(1, P0, TurnStep::Upkeep)
            .hand(P0, &[Prop::Regeneration])
            .actions(&[
                // Passing, or the instant in hand.
                ME.deciding(DecisionKind::Priority, 2),
                advance_to(1, TurnStep::PrecombatMain),
                // And the creature in the graveyard.
                ME.deciding(DecisionKind::Priority, 3),
                ME.casts(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
            ]),
        ON_BATTLEFIELD
            .titled("a permanent card put into the graveyard can be cast again at once")
            .interaction(&[Card::GetLost])
            .hand(P0, &[Card::GetLost])
            .battlefield(
                P0,
                &[
                    LURRUS,
                    Prop::Vanilla,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .graveyard(P0, &[])
            .actions(&[
                ME.casts(Card::GetLost),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.casts(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
            ]),
    ];
}
