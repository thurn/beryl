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

/// Jace counts the discarded card toward the five because [CR 608.2c] says “The controller
/// of the spell or ability follows its instructions in the order written.” If Jace leaves
/// in response, [CR 400.7] makes him “a new object with no memory of, or relation to, its
/// previous existence,” so the ability cannot exile and return him.
///
/// [CR 608.2c]: https://yawgatog.com/resources/magic-rules/#R6082c
/// [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
pub static CARD: CardDef = card(
    Card::JaceVrynsProdigy,
    &[
        Printed::JaceVrynsProdigy.abilities(&[Ability::activated(Cost::TAP).effect(&sequence(&[
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
        ]))]),
        Printed::JaceTelepathUnbound.abilities(&[
            Ability::loyalty(1)
                .target(&TargetSpec::creature().optional())
                .effect(&modify(
                    target(),
                    &[Modification::ModifyPt(-2, 0)],
                    Duration::UntilYourNextTurn,
                )),
            Ability::loyalty(-3)
                .target(&TargetSpec::graveyard_card(Filter::All(&[
                    instant_or_sorcery(),
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

pub static EMBLEM: CardDef = card(
    "Jace, Telepath Unbound Emblem",
    &[Face::EMPTY.abilities(&[
        Ability::triggered(&Trigger::casts(Players::You, Filter::Any))
            .effect(&mill(target_player(), 5))
            .target(&TargetSpec::opponent()),
    ])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// The back face's name, which the transformed permanent has (CR 712.6d).
    const TELEPATH: &str = "Jace, Telepath Unbound";

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
