// Generated from the card store by `just card header`: notes go below the end marker.
//! Dark Depths
//! Legendary Snow Land
//!
//! Dark Depths enters with ten ice counters on it.
//! {3}: Remove an ice counter from Dark Depths.
//! When Dark Depths has no ice counters on it, sacrifice it. If you do, create Marit Lage, a
//! legendary 20/20 black Avatar creature token with flying and indestructible.
// End of the generated card header.

use crate::prelude::*;

pub const MARIT_LAGE_TOKEN: &str = "Marit Lage";

pub static MARIT_LAGE: CardDef = Token::MaritLage
    .keywords(Keywords::FLYING.union(Keywords::INDESTRUCTIBLE))
    .card();

/// Thespian's Stage copying Dark Depths is already on the battlefield, so it gets no ice
/// counters from the enters replacement ([CR 614.1c]). The state trigger does not check
/// the number of counters again when it resolves ([CR 603.8]). Put the token onto the
/// battlefield only if Dark Depths was still there to be sacrificed.
///
/// [CR 614.1c]: https://yawgatog.com/resources/magic-rules/#R6141c
/// [CR 603.8]: https://yawgatog.com/resources/magic-rules/#R6038
pub static CARD: CardDef = Printed::DarkDepths
    .abilities(&[
        Ability::replacing(&Replacement::enters_with_counters(CounterKind::Ice, 10)),
        Ability::activated(Cost::mana(ManaCost::generic(3))).effect(&remove_counters(
            this(),
            CounterKind::Ice,
            1,
        )),
        Ability::triggered(&Trigger::state(&Condition::Compare(
            Amount::Counters(Objects::This, CounterKind::Ice),
            Comparison::Equal,
            Amount::Fixed(0),
        )))
        .effect(&sequence(&[
            sacrifice(this()),
            if_then(Condition::Did, &create_tokens(you(), &MARIT_LAGE, 1)),
        ])),
    ])
    .card()
    .creating(&[&MARIT_LAGE]);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const DEPTHS: &str = Card::DarkDepths;
    const MARIT_LAGE: &str = super::MARIT_LAGE_TOKEN;
    const ONE: &[Permanent] = &[Permanent::new(DEPTHS).counters(CounterKind::Ice, 1)];
    const TWO: &[Permanent] = &[Permanent::new(DEPTHS).counters(CounterKind::Ice, 2)];

    /// P0 controls Dark Depths with the ice counters `depths` gives it and six Islands.
    const fn depths(name: &'static str, depths: &'static [Permanent]) -> Scenario {
        Scenario::new(name)
            .battlefield(P0, &[Card::Island; 6])
            .permanents(P0, depths)
    }

    pub const SCENARIOS: &[Scenario] = &[
        // One Marit Lage: had two been made, the legend rule would put one into the graveyard
        // and Blood Artist's ability would trigger.
        Scenario::new("it makes one Marit Lage")
            .interaction(&[Card::BloodArtist])
            .battlefield(
                P0,
                &[Card::BloodArtist, Card::Island, Card::Island, Card::Island],
            )
            .permanents(P0, ONE)
            .actions(&[
                ME.activates(DEPTHS, 1),
                Resolve,
                Resolve,
                stack(0),
                ME.controls(MARIT_LAGE, 1),
                ME.life(20),
            ]),
        // A state trigger doesn't trigger again while it is on the stack (CR 603.8): removing
        // nothing in response leaves it the only one.
        depths("it doesn't trigger again while its trigger waits", ONE).actions(&[
            ME.activates(DEPTHS, 1),
            Resolve,
            stack(1),
            ME.activates(DEPTHS, 1),
            stack(2),
            Resolve,
            stack(1),
            Resolve,
            stack(0),
            ME.controls(MARIT_LAGE, 1),
        ]),
        // Countered, the trigger triggers again at once while Dark Depths still has no ice
        // counters (CR 603.8).
        depths("countered, it triggers again", ONE)
            .interaction(&[Card::Stifle])
            .battlefield(P1, &[Card::Island])
            .hand(P1, &[Card::Stifle])
            .actions(&[
                ME.activates(DEPTHS, 1),
                Resolve,
                ME.passes(),
                THEM.casts(Card::Stifle),
                Resolve,
                stack(1),
                Resolve,
                ME.controls(MARIT_LAGE, 1),
            ]),
        // Destroyed while its trigger waits, Dark Depths can't be sacrificed, so there is no
        // Marit Lage ("if you do").
        depths("gone before its trigger resolves, no Marit Lage", ONE)
            .interaction(&[Card::StripMine])
            .battlefield(P1, &[Card::StripMine])
            .actions(&[
                ME.activates(DEPTHS, 1),
                Resolve,
                ME.passes(),
                THEM.activates(Card::StripMine, 1),
                THEM.targets(DEPTHS),
                Resolve,
                ME.has(Zone::Graveyard, DEPTHS, 1),
                Resolve,
                stack(0),
                ME.controls(MARIT_LAGE, 0),
            ]),
        // With the Plains tapped, Dark Depths cannot supply mana for a Mana Tithe.
        Scenario::new("Dark Depths cannot pay for Mana Tithe")
            .interaction(&[Card::ManaTithe])
            .battlefield(P0, &[Card::Plains])
            .permanents(P0, TWO)
            .hand(P0, &[Prop::Artifact])
            .battlefield(P1, &[Card::Plains])
            .hand(P1, &[Card::ManaTithe])
            .actions(&[
                ME.casts(Prop::Artifact),
                THEM.casts(Card::ManaTithe),
                ME.has(Zone::Graveyard, Prop::Artifact, 1),
                ME.tapped(DEPTHS, 0),
            ]),
    ];
}
