// Generated from the card store by `just card header`: notes go below the end marker.
//! Thespian's Stage
//! Land
//!
//! {T}: Add {C}.
//! {2}, {T}: This land becomes a copy of target land, except it has this ability.
// End of the generated card header.

use crate::prelude::*;

/// Copying Dark Depths gives the Stage no ice counters. [CR 707.2] says that “status,
/// counters, and stickers are not copied.” The Stage already occupies the battlefield, so
/// the copied replacement that gives Dark Depths its initial counters never applies.
///
/// The copy exception grants a copy of the activated ability. [CR 707.9a] says “This ability
/// becomes part of the copiable values for the copy,” so the printed face and the exception
/// each carry the same ability.
///
/// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
/// [CR 707.9a]: https://yawgatog.com/resources/magic-rules/#R7079a
pub static CARD: CardDef = Printed::ThespiansStage
    .abilities(&[
        Ability::tap_for_mana(ManaSet::one(ManaColor::Colorless)),
        BECOME_COPY,
    ])
    .grants(&[BECOME_COPY])
    .excepts(&[CopyException(&[CopyChange::Has(GrantIx::new(0))])])
    .card();

/// "{2}, {T}: This land becomes a copy of target land, except it has this ability."
const BECOME_COPY: Ability = Ability::activated(Cost::of(ManaCost::generic(2), &[CostPart::TAP]))
    .target(&TargetSpec::permanent(Filter::Types(Types::LAND)))
    .effect(&become_copy_except(this(), target(), ExceptIx::new(0)));

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const STAGE: &str = Card::ThespiansStage;
    const DEPTHS: &str = Card::DarkDepths;
    const MARIT_LAGE: &str = "Marit Lage";
    const DEPTHS_ICED: &[Permanent] = &[Permanent::new(DEPTHS).counters(CounterKind::Ice, 10)];

    pub const SCENARIOS: &[Scenario] = &[
        // P1's Dark Depths: no legend rule between permanents of two players. The copy became
        // one without entering, so it has no ice counters, and its state trigger triggers at
        // once. (Copying P0's own Dark Depths meets the legend rule, in which P0 may keep the
        // copy: both are named Dark Depths, which a scenario can't tell apart, so
        // `beryl_cards`' `tests::copies` shows it.)
        Scenario::new("it copies an opponent's Dark Depths for Marit Lage")
            .interaction(&[DEPTHS])
            .battlefield(P0, &[STAGE, Card::Island, Card::Island])
            .permanents(P1, DEPTHS_ICED)
            .actions(&[
                ME.activates(STAGE, 1),
                ME.targets(DEPTHS),
                Resolve,
                Resolve,
                ME.controls(MARIT_LAGE, 1),
                THEM.counters(DEPTHS, CounterKind::Ice, 10),
            ]),
        // Its {C} and two Islands pay Dark Depths' {3}.
        Scenario::new("it taps for colorless mana")
            .interaction(&[DEPTHS])
            .battlefield(P0, &[STAGE, Card::Island, Card::Island])
            .permanents(P0, DEPTHS_ICED)
            .actions(&[
                ME.activates(DEPTHS, 1),
                Resolve,
                ME.counters(DEPTHS, CounterKind::Ice, 9),
                ME.tapped(STAGE, 1),
            ]),
        // A copy of an Island is an Island, which taps for {U} and no longer for {C}, and
        // keeps the ability that made it a copy: the next turn it copies a Plains.
        Scenario::new("it copies a basic land and can copy again")
            .battlefield(P0, &[STAGE, Card::Island, Card::Island, Card::Plains])
            .actions(&[
                ME.activates(STAGE, 1),
                ME.targets(Card::Island),
                Resolve,
                ME.controls(Card::Island, 3),
                ME.controls(STAGE, 0),
                advance_to(3, TurnStep::PrecombatMain),
                // The ability the copy exception gives it, among those it is granted.
                Action::activate_granted(P0, Card::Island, 0),
                ME.targets(Card::Plains),
                Resolve,
                ME.controls(Card::Plains, 2),
                ME.controls(Card::Island, 2),
            ]),
        // The Stage copies a basic Island, so the Blood Moon cast after leaves it alone, and it
        // keeps its ability. Next turn, under Blood Moon, it copies P1's Dark Depths, which Blood Moon
        // has made a Mountain. What it copies is what is printed on Dark Depths, not what Blood
        // Moon makes it ([CR 707.2]), but Blood Moon applies to the copy too: a nonbasic land
        // with no abilities, so no state trigger. Once Blood Moon is destroyed, the copy is
        // Dark Depths with no ice counters, and P0 gets Marit Lage.
        //
        // [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
        Scenario::new("it copies what is printed on a land Blood Moon made a Mountain")
            .interaction(&[DEPTHS, Card::BloodMoon, Card::GetLost])
            .battlefield(
                P0,
                &[
                    STAGE,
                    Card::Mountain,
                    Card::Plains,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[Card::BloodMoon, Card::GetLost])
            .library(P0, &[Card::Island, Card::Island])
            .permanents(P1, DEPTHS_ICED)
            .actions(&[
                ME.activates(STAGE, 1),
                ME.targets(Card::Island),
                Resolve,
                ME.casts(Card::BloodMoon),
                Resolve,
                advance_to(3, TurnStep::PrecombatMain),
                Action::activate_granted(P0, Card::Island, 0),
                ME.targets(DEPTHS),
                Resolve,
                ME.controls(DEPTHS, 1),
                ME.counters(DEPTHS, CounterKind::Ice, 0),
                stack(0),
                ME.casts(Card::GetLost),
                stack(1),
                Resolve,
                stack(1),
                Resolve,
                ME.controls(MARIT_LAGE, 1),
                ME.has(Zone::Graveyard, STAGE, 1),
                THEM.counters(DEPTHS, CounterKind::Ice, 10),
            ]),
        // The Stage remains tapped after copying a land that is still untapped.
        Scenario::new("the Stage stays tapped after copying an untapped land")
            .battlefield(P0, &[STAGE, Card::Island, Card::Island])
            .battlefield(P1, &[Card::Plains])
            .actions(&[
                ME.activates(STAGE, 1),
                ME.targets(Card::Plains),
                Resolve,
                ME.tapped(Card::Plains, 1),
                THEM.tapped(Card::Plains, 0),
            ]),
    ];
}
