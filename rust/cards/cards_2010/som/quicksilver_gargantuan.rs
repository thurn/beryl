// Generated from the card store by `just card header`: notes go below the end marker.
//! Quicksilver Gargantuan {5}{U}{U}
//! Creature — Shapeshifter, 7/7
//!
//! You may have this creature enter as a copy of any creature on the battlefield, except it's 7/7.
// End of the generated card header.

use crate::prelude::*;

/// "You may have this creature enter as a copy of any creature on the battlefield" is a
/// replacement effect that has it become a copy as it enters ([CR 707.5]): "An object that
/// enters the battlefield 'as a copy' or 'that's a copy' of another object becomes a copy as it
/// enters the battlefield." The creature is chosen, not targeted, so hexproof does not stop it.
///
/// "Except it's 7/7" is part of the copy's copiable values ([CR 707.9b]): "The final set of
/// values for that characteristic becomes part of the copiable values of the copy." So a copy
/// of the Gargantuan is 7/7 too.
///
/// It stands in for Phyrexian Metamorph, whose Phyrexian mana the vocabulary cannot say.
///
/// [CR 707.5]: https://yawgatog.com/resources/magic-rules/#R7075
/// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
pub static CARD: CardDef = Printed::QuicksilverGargantuan
    .abilities(&[Ability::replacing(&Replacement::enter_as_copy(
        &Filter::Types(Types::CREATURE),
        Some(ExceptIx::new(0)),
    ))])
    .excepts(&[CopyException(&[CopyChange::PowerToughness(7, 7)])])
    .card();
#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GARGANTUAN: &str = Card::QuicksilverGargantuan;
    const BEZA: &str = Card::BezaTheBoundingSpring;
    const PAINTER: &str = Card::PaintersServant;

    /// P0, at 10 life against P1's 20, holds Quicksilver Gargantuan and seven Islands; P1
    /// controls `permanents`.
    const fn copying(name: &'static str, permanents: &'static [&'static str]) -> Scenario {
        Scenario::new(name)
            .life(P0, 10)
            .battlefield(P0, &[Card::Island; 7])
            .hand(P0, &[GARGANTUAN])
            .battlefield(P1, permanents)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // The Gargantuan enters as Beza, the Bounding Spring, a 7/7 for its exception, and
        // Beza's enters ability triggers for it
        // ([CR 707.5](https://yawgatog.com/resources/magic-rules/#R7075)), gaining P0 4 life.
        copying("the Gargantuan enters as a 7/7 copy", &[Card::Plains])
            .interaction(&[BEZA])
            .permanents(
                P1,
                &[Permanent::new(BEZA)
                    .tapped()
                    .counters(CounterKind::PlusOne, 1)],
            )
            .actions(&[
                ME.casts(GARGANTUAN),
                Resolve,
                ME.picks_up_to(BEZA),
                ME.pt(BEZA, 7, 7),
                Resolve,
                ME.life(14),
                ME.tapped(BEZA, 0),
                ME.counters(BEZA, CounterKind::PlusOne, 0),
                THEM.pt(BEZA, 5, 6),
            ]),
        // Declining, it enters as itself, a 7/7 Shapeshifter, and Beza's ability is not its.
        copying(
            "a Gargantuan that copies nothing is itself",
            &[BEZA, Card::Plains],
        )
        .interaction(&[BEZA])
        .actions(&[
            ME.casts(GARGANTUAN),
            Resolve,
            ME.done_picking(),
            ME.pt(GARGANTUAN, 7, 7),
            ME.life(10),
        ]),
        // A copied "as this enters" applies to the copy
        // ([CR 707.5](https://yawgatog.com/resources/magic-rules/#R7075)): "If the text
        // that's being copied includes any abilities that replace the enters-the-battlefield
        // event (such as 'enters with' or 'as [this] enters' abilities), those abilities will
        // take effect." The Gargantuan enters as Painter's Servant and chooses green.
        copying(
            "a copy of Painter's Servant chooses a color as it enters",
            &[PAINTER, Card::Plains],
        )
        .interaction(&[PAINTER])
        .actions(&[
            ME.casts(GARGANTUAN),
            Resolve,
            ME.picks_up_to(PAINTER),
            ME.chooses_color(Colors::GREEN),
            ME.pt(PAINTER, 7, 7),
            ME.colored(Zone::Battlefield, PAINTER, Colors::GREEN),
            THEM.colored(Zone::Battlefield, Card::Plains, Colors::GREEN),
        ]),
        // The Gargantuan takes Marit Lage's name and types, with its own 7/7 exception.
        copying(
            "the Gargantuan copies a Marit Lage token as a 7/7 card",
            &[],
        )
        .interaction(&[Card::DarkDepths])
        .permanents(
            P1,
            &[Permanent::new(Card::DarkDepths).counters(CounterKind::Ice, 0)],
        )
        .actions(&[
            Resolve,
            THEM.token_pt("Marit Lage", 20, 20),
            ME.casts(GARGANTUAN),
            Resolve,
            ME.picks_up_to("Marit Lage"),
            ME.pt("Marit Lage", 7, 7),
            ME.has_subtype("Marit Lage", Subtype::Avatar, true),
        ]),
        // The Gargantuan copies what the Image became, including its Illusion exception.
        copying(
            "a Gargantuan of an Image keeps the Image's Illusion type",
            &[],
        )
        .interaction(&[Card::PhantasmalImage])
        .at(2, P1, TurnStep::PrecombatMain)
        .battlefield(
            P0,
            &[
                Prop::Vanilla,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Island,
                Card::Island,
            ],
        )
        .battlefield(P1, &[Card::Island, Card::Island, Card::Plains])
        .hand(P1, &[Card::PhantasmalImage, Prop::Artifact])
        .actions(&[
            THEM.casts(Card::PhantasmalImage),
            Resolve,
            THEM.picks_up_to(Prop::Vanilla),
            THEM.casts(Prop::Artifact),
            Resolve,
            Resolve,
            ME.controls(Prop::Vanilla, 0),
            advance_to(3, TurnStep::PrecombatMain),
            ME.casts(GARGANTUAN),
            Resolve,
            ME.picks_up_to(Prop::Vanilla),
            ME.pt(Prop::Vanilla, 7, 7),
            ME.has_subtype(Prop::Vanilla, Subtype::Illusion, true),
        ]),
    ];
}
// Scratch comment for a gate timing measurement.
