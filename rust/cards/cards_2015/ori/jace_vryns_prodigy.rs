// Generated from the card store by `just card header`: notes go below the end marker.
//! Jace, Vryn's Prodigy {1}{U}
//! Legendary Creature — Human Wizard, 0/2
//!
//! {T}: Draw a card, then discard a card. If there are five or more cards in your graveyard, exile
//! Jace, then return him to the battlefield transformed under his owner's control.
//!
//! Jace, Telepath Unbound
//! Legendary Planeswalker — Jace, loyalty 5
//!
//! +1: Up to one target creature gets -2/-0 until your next turn.
//! −3: You may cast target instant or sorcery card from your graveyard this turn. If that spell
//! would be put into your graveyard, exile it instead.
//! −9: You get an emblem with "Whenever you cast a spell, target opponent mills five cards."
// End of the generated card header.

use crate::prelude::*;

/// [CR 712.2](https://yawgatog.com/resources/magic-rules/#R7122): a double-faced card "has a
/// Magic card face on each side rather than a Magic card face on one side and a Magic card
/// back on the other."
///
/// "{T}: Draw a card, then discard a card. If there are five or more cards in your graveyard,
/// exile Jace, then return him to the battlefield transformed under his owner's control."
/// The graveyard is counted after the discard, as the instructions are followed in order
/// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)). A Jace that has left
/// the battlefield is not exiled, and one that was not exiled does not return
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)). "The cards in your
/// graveyard".
///
/// "+1: Up to one target creature gets -2/-0 until your next turn." A layer 7c effect
/// ([CR 613.4c](https://yawgatog.com/resources/magic-rules/#R6134c)): "Effects and counters
/// that modify power and/or toughness (but don't set power and/or toughness to a specific
/// number or value) are applied." It lasts "as long as stated by the spell or ability creating
/// it" ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)).
///
/// "−3: You may cast target instant or sorcery card from your graveyard this turn. If that
/// spell would be put into your graveyard, exile it instead." The permission carries the
/// replacement to the spell it casts: a new object on the stack
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which "exile it instead"
/// follows wherever it would go to the graveyard: resolving, countered or fizzling
/// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)).
///
/// "−9: You get an emblem with 'Whenever you cast a spell, target opponent mills five
/// cards.'" [CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142): "This means that
/// [player] puts an emblem with [ability] into the command zone. The emblem is both owned and
/// controlled by that player."
pub static CARD: CardDef = card(
    Card::JaceVrynsProdigy,
    &[
        Printed::JaceVrynsProdigy.abilities(&[Ability::activated(Cost::TAP).effect(
            &sequence(&[
                draw(you(), 1),
                discard(you(), 1),
                if_then(
                    Condition::Compare(
                        Amount::Count(&Objects::All {
                            zone: Zone::Graveyard,
                            filter: Filter::Whose(&Players::You),
                        }),
                        Comparison::AtLeast,
                        Amount::Fixed(5),
                    ),
                    &sequence(&[exile(this()), return_transformed(those())]),
                ),
            ]),
        )]),
        Printed::JaceTelepathUnbound.abilities(&[
            Ability::loyalty(1)
                .target(&TargetSpec::creature().optional())
                .effect(&modify(
                    target(),
                    Modification::ModifyPt(-2, 0),
                    Duration::UntilYourNextTurn,
                )),
            Ability::loyalty(-3)
                .target(&TargetSpec::graveyard_card(Filter::All(&[
                    Filter::Types(Types::INSTANT.union(Types::SORCERY)),
                    Filter::Whose(&Players::You),
                ])))
                .effect(&permit_cast(
                    you(),
                    target(),
                    Duration::EndOfTurn,
                    CastTerms::PLAIN.exiled_instead(),
                )),
            Ability::loyalty(-9).effect(&get_emblem(you(), &EMBLEM)),
        ]),
    ],
)
.creating(&[&EMBLEM]);

/// The emblem of the −9 ([CR 114.1](https://yawgatog.com/resources/magic-rules/#R1141)): "An
/// emblem is a marker used to represent an object that has one or more abilities, but usually
/// no other characteristics."
///
/// "Whenever you cast a spell, target opponent mills five cards."
/// [CR 701.17a](https://yawgatog.com/resources/magic-rules/#R70117a): "For a player to mill a
/// number of cards, that player puts that many cards from the top of their library into their
/// graveyard."
pub static EMBLEM: CardDef = card(
    "Jace, Telepath Unbound Emblem",
    &[Face::EMPTY.abilities(&[
        Ability::triggered(&Trigger::casts(Players::You, Filter::Any))
            .effect(&mill(target_player(), 5))
            .target(&TargetSpec::opponent()),
    ])],
);

/// The back face's name, which the transformed permanent has (CR 712.6d).
const TELEPATH: &str = "Jace, Telepath Unbound";

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use super::TELEPATH;
    use crate::prelude::*;

    const JACE: &str = Card::JaceVrynsProdigy;

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("looting to five cards in the graveyard transforms him")
            .battlefield(P0, &[JACE])
            .hand(P0, &[Card::Plains])
            .graveyard(P0, &[Card::Island, Card::Island, Card::Island])
            .library(P0, &[Card::Island, Card::Island, Card::Island])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                // Four in the graveyard after the first loot: no transformation.
                ME.activates(JACE, 0), // {T}
                Resolve,
                ME.picks(Card::Plains),
                ME.creature(JACE, true),
                advance_to(3, TurnStep::PrecombatMain),
                // Nothing else to do: the loot resolves as soon as its controller passes, and
                // with only Islands in hand, the discard needs no choice.
                ME.activates(JACE, 0), // {T}
                // Exiled and returned with his back face up: a planeswalker with loyalty 5.
                ME.creature(TELEPATH, false),
                ME.has_loyalty(TELEPATH, 5),
                ME.has_cards(Zone::Graveyard, 5),
                // A new object, so its loyalty abilities may be activated this turn. No creature
                // to target: none is chosen.
                ME.activates(TELEPATH, 0), // +1
                ME.has_loyalty(TELEPATH, 6),
            ]),
        Scenario::new("+1 shrinks a creature until his controller's next turn")
            .permanents(P0, &[Permanent::new(JACE).transformed()])
            .battlefield(P1, &[JACE])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Plains, Card::Plains])
            .actions(&[
                ME.activates(TELEPATH, 0), // +1
                ME.target_count(1),
                Resolve,
                ME.has_loyalty(TELEPATH, 6),
                THEM.pt(JACE, -2, 2),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.done_attacking(),
                advance_to(2, TurnStep::End),
                THEM.pt(JACE, -2, 2),
                advance_to(3, TurnStep::Upkeep),
                THEM.pt(JACE, 0, 2),
            ]),
        Scenario::new("−3 recasts a sorcery from the graveyard, which is then exiled")
            .permanents(
                P0,
                &[Permanent::new(JACE).transformed(), Permanent::new(Card::Plains)],
            )
            .graveyard(P0, &[Card::Oust])
            // A land to play keeps the game in this main phase for the assertions.
            .hand(P0, &[Card::Island])
            .battlefield(P1, &[JACE])
            .library(P0, &[Card::Island])
            .library(P1, &[Card::Plains, Card::Island])
            .actions(&[
                ME.activates(TELEPATH, 1), // −3
                Resolve,
                ME.has_loyalty(TELEPATH, 2),
                ME.casts(Card::Oust),
                Resolve,
                ME.has(Zone::Exile, Card::Oust, 1),
                ME.has_cards(Zone::Graveyard, 0),
                THEM.library_top(&[Card::Plains, JACE, Card::Island]),
            ]),
        Scenario::new("−9's emblem mills an opponent five whenever its owner casts a spell")
            .permanents(
                P0,
                &[
                    Permanent::new(JACE)
                        .transformed()
                        .counters(CounterKind::Loyalty, 9),
                    Permanent::new(Card::Plains),
                    Permanent::new(Card::Plains),
                ],
            )
            .hand(P0, &[Prop::Artifact, Card::Plains])
            .library(P0, &[Card::Island])
            .library(
                P1,
                &[
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                    Card::Island,
                ],
            )
            .actions(&[
                ME.activates(TELEPATH, 2), // −9
                // Loyalty 0: he is put into his owner's graveyard; the emblem stays.
                ME.has(Zone::Graveyard, JACE, 1),
                // The trigger goes on the stack above the artifact and resolves first.
                ME.casts(Prop::Artifact),
                THEM.has(Zone::Graveyard, Card::Plains, 5),
                THEM.library_top(&[Card::Island]),
                ME.controls(Prop::Artifact, 1),
            ]),
    ];
}
