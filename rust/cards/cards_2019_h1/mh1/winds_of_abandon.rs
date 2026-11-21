// Generated from the card store by `just card header`: notes go below the end marker.
//! Winds of Abandon {1}{W}
//! Sorcery
//!
//! Exile target creature you don't control. For each creature exiled this way, its controller
//! searches their library for a basic land card. Those players put those cards onto the battlefield
//! tapped, then shuffle.
//! Overload {4}{W}{W} (You may cast this spell for its overload cost. If you do, change "target" in
//! its text to "each.")
// End of the generated card header.

use crate::prelude::*;

/// This filter includes a teammate's creature in a team game. [CR 102.3] says “the player's
/// opponents are all players not on their team.”
///
/// [CR 102.3]: https://yawgatog.com/resources/magic-rules/#R1023
const CREATURES_YOU_DONT_CONTROL: Filter = Filter::All(&[
    Filter::Types(Types::CREATURE),
    Filter::Not(&Filter::Whose(&Players::You)),
]);

/// Each player searches according to their total of creatures actually exiled. The count
/// uses each creature's controller on the battlefield. [CR 608.2h] says “the effect uses
/// the object's last known information” once the object leaves its expected public zone.
///
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
const FETCH: Effect = for_each_player(
    each_player(),
    &Effect::Search {
        player: that_player(),
        filter: Filter::All(&[
            Filter::Types(Types::LAND),
            Filter::Supertypes(Supertypes::BASIC),
        ]),
        count: Amount::ThoseOf(that_player()),
        found: Found::OntoBattlefieldTapped,
    },
);

pub static CARD: CardDef = Printed::WindsOfAbandon
    .abilities(&[
        Ability::spell()
            .target(&TargetSpec::permanent(CREATURES_YOU_DONT_CONTROL))
            .effect(&sequence(&[exile(target()), FETCH])),
        Ability::spell().effect(&sequence(&[
            exile(Objects::All {
                zone: Zone::Battlefield,
                filter: CREATURES_YOU_DONT_CONTROL,
            }),
            FETCH,
        ])),
    ])
    .casting(&[CastingOption::overload(
        ManaCost::new(4, [2, 0, 0, 0, 0, 0]),
        1,
    )])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const WINDS: &str = Card::WindsOfAbandon;

    const FLIER: &str = Prop::Flier;

    /// P0's creature, or P1's second.
    const TWO_DROP: &str = Prop::Vanilla;

    const SIX_PLAINS_AND_ADELINE: &[&str] = &[
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        Card::Plains,
        TWO_DROP,
    ];

    pub const SCENARIOS: &[Scenario] = &[
        // Its one target is the creature P0 doesn't control; P1 finds a Plains, which enters
        // tapped, from a library with a nonbasic land too.
        Scenario::new("Winds of Abandon exiles a creature; its controller fetches a tapped basic")
            .battlefield(P0, &[Card::Plains, Card::Plains, TWO_DROP])
            .hand(P0, &[WINDS, Card::Plains])
            .battlefield(P1, &[FLIER])
            .library(P1, &[Card::Tundra, Card::Plains, Card::Island])
            .actions(&[
                ME.casts(WINDS),
                Resolve,
                THEM.picks_up_to(Card::Plains),
                THEM.has(Zone::Exile, FLIER, 1),
                ME.controls(TWO_DROP, 1),
                THEM.controls(Card::Plains, 1),
                THEM.tapped(Card::Plains, 1),
                THEM.has_cards(Zone::Library, 2),
            ]),
        // Overloaded, it exiles both of P1's creatures and spares P0's; P1 may find two basics
        // and finds the Island, then is done.
        Scenario::new("Winds of Abandon overloaded: a basic per creature exiled")
            .battlefield(P0, SIX_PLAINS_AND_ADELINE)
            .hand(P0, &[WINDS, Card::Plains])
            .battlefield(P1, &[FLIER, TWO_DROP])
            .library(P1, &[Card::Island, Card::Plains, Card::Tundra])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: WINDS,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                // Done, the Island and the Plains, not the Tundra, which is no basic land.
                THEM.deciding(DecisionKind::ChooseUpTo, 3),
                THEM.picks_up_to(Card::Island),
                THEM.done_picking(),
                THEM.has_cards(Zone::Exile, 2),
                ME.controls(TWO_DROP, 1),
                THEM.controls(Card::Island, 1),
                THEM.controls(Card::Plains, 0),
                THEM.tapped(Card::Island, 1),
            ]),
        // UI regression by-yfw: the token leaves while Winds stays on the stack for the search.
        Scenario::new("Winds of Abandon exiles a token before its controller searches")
            .interaction(&[Card::StaffOfTheStoryteller])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[WINDS, Card::Plains])
            .battlefield(P1, &[Card::Plains, Card::Plains])
            .hand(P1, &[Card::StaffOfTheStoryteller])
            .library(P0, &[Card::Island, Card::Island])
            .library(P1, &[Card::Island, Card::Plains])
            .actions(&[
                // Create the token in play: external engines do not share Beryl token names.
                THEM.casts(Card::StaffOfTheStoryteller),
                advance_to(3, TurnStep::PrecombatMain),
                ME.casts(WINDS),
                Resolve,
                THEM.picks_up_to(Card::Island),
                THEM.controls("Spirit Token", 0),
            ]),
    ];
}
