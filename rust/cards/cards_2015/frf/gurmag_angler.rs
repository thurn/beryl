// Generated from the card store by `just card header`: notes go below the end marker.
//! Gurmag Angler {6}{B}
//! Creature — Zombie Fish, 5/5
//!
//! Delve (Each card you exile from your graveyard while casting this spell pays for {1}.)
// End of the generated card header.

use crate::prelude::*;

/// Delve ([CR 702.66a]): "For each generic mana in this spell's total cost, you may exile a card
/// from your graveyard rather than pay that mana." The card store alone would give this
/// definition; the file holds the scenarios that show delve.
///
/// [CR 702.66a]: https://yawgatog.com/resources/magic-rules/#R70266a
pub static CARD: CardDef = Printed::GurmagAngler.keywords(Keywords::DELVE).card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ANGLER: &str = Card::GurmagAngler;
    const TRINISPHERE: &str = Card::Trinisphere;

    pub const SCENARIOS: &[Scenario] = &[
        // Six of the seven cards pay {6}, and the Swamp pays {B}. Delve pays generic mana
        // only, so the seventh card stays.
        Scenario::new("Gurmag Angler with seven cards in the graveyard costs {B}")
            .battlefield(P0, &[Card::Swamp])
            .hand(P0, &[ANGLER])
            .graveyard(P0, &[Card::Island; 7])
            .actions(&[
                ME.casts(ANGLER),
                ME.has(Zone::Exile, Card::Island, 6),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.tapped(Card::Swamp, 1),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
        // Three cards pay {3}; four lands pay {3}{B}.
        Scenario::new("Gurmag Angler with three cards in the graveyard")
            .battlefield(P0, &[Card::Swamp, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[ANGLER])
            .graveyard(P0, &[Card::Plains; 3])
            .actions(&[
                ME.casts(ANGLER),
                ME.has(Zone::Exile, Card::Plains, 3),
                ME.tapped(Card::Island, 3),
                ME.tapped(Card::Swamp, 1),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
        // Its total cost, {6}{B}, is three or more, so Trinisphere leaves it alone, and delve
        // pays the generic mana of the total after it is locked in.
        Scenario::new("Gurmag Angler under Trinisphere with a full graveyard")
            .interaction(&[TRINISPHERE])
            .battlefield(P0, &[TRINISPHERE, Card::Swamp])
            .hand(P0, &[ANGLER])
            .graveyard(P0, &[Card::Island; 7])
            .actions(&[
                ME.casts(ANGLER),
                ME.has(Zone::Exile, Card::Island, 6),
                ME.tapped(Card::Swamp, 1),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
        // Five lands and three cards: two cards must go, and P0 chooses which. Exiling the
        // instant and the sorcery keeps the Island in the graveyard, and the five lands pay
        // the rest rather than a third card. Cards are offered from the bottom of
        // the graveyard up, so each set of them is one sequence of choices: the sorcery, put
        // there last of the three, is offered first.
        Scenario::new("Gurmag Angler: the caster chooses which cards to exile")
            .battlefield(
                P0,
                &[
                    Card::Swamp,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .hand(P0, &[ANGLER])
            .graveyard(P0, &[Card::Island, Prop::Instant, Prop::Sorcery])
            .actions(&[
                ME.casts(ANGLER),
                // The top card alone would leave none to pay the second {1}.
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(Prop::Sorcery),
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.picks_up_to(Prop::Instant),
                // The Island too, or mana for the rest.
                ME.deciding(DecisionKind::ChooseUpTo, 2),
                ME.done_picking(),
                ME.has(Zone::Exile, Prop::Instant, 1),
                ME.has(Zone::Exile, Prop::Sorcery, 1),
                ME.has(Zone::Graveyard, Card::Island, 1),
                ME.tapped(Card::Island, 4),
                ME.tapped(Card::Swamp, 1),
                Resolve,
                ME.controls(ANGLER, 1),
            ]),
    ];
}
