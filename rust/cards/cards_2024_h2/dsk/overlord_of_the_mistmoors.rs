// Generated from the card store by `just card header`: notes go below the end marker.
//! Overlord of the Mistmoors {5}{W}{W}
//! Enchantment Creature — Avatar Horror, 6/6
//!
//! Impending 4—{2}{W}{W} (If you cast this spell for its impending cost, it enters with four time
//! counters and isn't a creature until the last is removed. At the beginning of your end step,
//! remove a time counter from it.)
//! Whenever this permanent enters or attacks, create two 2/1 white Insect creature tokens with
//! flying.
// End of the generated card header.

use crate::prelude::*;

pub use crate::tokens::INSECT_TOKEN;

pub static INSECT: CardDef = Token::InsectToken.keywords(Keywords::FLYING).card();

static TWO_INSECTS: Effect = create_tokens(you(), &INSECT, 2);

/// The impending casting option carries the alternative cost and initial time counters.
/// The other two abilities apply the type change and count down at the end step.
/// [CR 702.176a] calls impending “a keyword that represents four abilities.”
///
/// [CR 702.176a]: https://yawgatog.com/resources/magic-rules/#R702176a
pub static CARD: CardDef = Printed::OverlordOfTheMistmoors
    .abilities(&[
        Ability::impending(),
        Ability::impending_countdown(),
        when_enters(&TWO_INSECTS),
        whenever_attacks(&TWO_INSECTS).in_paragraph_before(),
    ])
    .casting(&[CastingOption::Impending {
        cost: mana("{2}{W}{W}"),
        counters: 4,
    }])
    .card()
    .creating(&[&INSECT]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use super::INSECT_TOKEN;
    use crate::prelude::*;

    const OVERLORD: &str = Card::OverlordOfTheMistmoors;

    const fn plains(n: usize) -> &'static [&'static str] {
        const SEVEN: &[&str] = &[Card::Plains; 7];
        SEVEN.split_at(n).0
    }

    /// One of your turns from its beginning of combat: no attack, then the end step's countdown
    /// trigger resolves.
    macro_rules! countdown_turn {
        ($turn:expr) => {
            [
                advance_to($turn, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.done_attacking(),
                advance_to($turn, TurnStep::End),
                Resolve,
            ]
        };
    }

    const IMPENDING_TURNS: [[Action; 5]; 3] =
        [countdown_turn!(3), countdown_turn!(5), countdown_turn!(7)];

    pub const SCENARIOS: &[Scenario] = &[
        // Cast for {2}{W}{W}: four time counters, not a creature, two Insects all the same. Each
        // of its controller's end steps removes one; with the last gone it is a 6/6.
        Scenario::new("Overlord cast for impending")
            .hand(P0, &[OVERLORD])
            .battlefield(P0, plains(4))
            .library(P0, &[Card::Plains, Card::Plains, Card::Plains])
            .library(P1, &[Card::Island, Card::Island, Card::Island])
            .actions(&[
                Action::Cast {
                    player: P0,
                    card: OVERLORD,
                    face: FaceIx::FRONT,
                    costs: Castings::only(0),
                },
                Resolve,
                ME.creature(OVERLORD, false),
                ME.counters(OVERLORD, CounterKind::Time, 4),
                Resolve,
                ME.controls(INSECT_TOKEN, 2),
                advance_to(1, TurnStep::End),
                Resolve,
                ME.counters(OVERLORD, CounterKind::Time, 3),
                IMPENDING_TURNS[0][0],
                IMPENDING_TURNS[0][1],
                IMPENDING_TURNS[0][2],
                IMPENDING_TURNS[0][3],
                IMPENDING_TURNS[0][4],
                ME.counters(OVERLORD, CounterKind::Time, 2),
                IMPENDING_TURNS[1][0],
                IMPENDING_TURNS[1][1],
                IMPENDING_TURNS[1][2],
                IMPENDING_TURNS[1][3],
                IMPENDING_TURNS[1][4],
                ME.counters(OVERLORD, CounterKind::Time, 1),
                ME.creature(OVERLORD, false),
                IMPENDING_TURNS[2][0],
                IMPENDING_TURNS[2][1],
                IMPENDING_TURNS[2][2],
                IMPENDING_TURNS[2][3],
                IMPENDING_TURNS[2][4],
                ME.counters(OVERLORD, CounterKind::Time, 0),
                ME.creature(OVERLORD, true),
                ME.pt(OVERLORD, 6, 6),
            ]),
    ];
}
