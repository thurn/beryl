// Generated from the card store by `just card header`: notes go below the end marker.
//! Lord of Atlantis {U}{U}
//! Creature — Merfolk, 2/2
//!
//! Other Merfolk get +1/+1 and have islandwalk. (They can't be blocked as long as defending player
//! controls an Island.)
// End of the generated card header.

use crate::prelude::*;

/// "Other Merfolk get +1/+1 and have islandwalk." is a static ability over every Merfolk on
/// the battlefield, whoever controls it, but the Lord itself ([CR 109.1]): an "object" is "an
/// ability on the stack, a card, a copy of a card, a token, a spell, a permanent, or an
/// emblem", and "other" leaves out the one with the ability. So two Lords each give the other
/// +1/+1. Islandwalk is added in layer 6 ([CR 613.1f]) and the bonus in layer 7c
/// ([CR 613.4c]).
///
/// A Merfolk with islandwalk can't be blocked while the defending player controls an Island
/// ([CR 702.14c]): "A creature with landwalk can't be blocked as long as the defending player
/// controls at least one land with the specified land type (as in “islandwalk”)". "Island" in
/// "islandwalk" is a land type word of the Lord's text, so a text change of the Lord that
/// makes it "Swamp" gives the other Merfolk swampwalk ([CR 612.1]).
///
/// [CR 109.1]: https://yawgatog.com/resources/magic-rules/#R1091
/// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
/// [CR 613.1f]: https://yawgatog.com/resources/magic-rules/#R6131f
/// [CR 613.4c]: https://yawgatog.com/resources/magic-rules/#R6134c
/// [CR 702.14c]: https://yawgatog.com/resources/magic-rules/#R70214c
pub static CARD: CardDef = Printed::LordOfAtlantis
    .abilities(&[Ability::static_ability(&LayerSpec {
        affected: Objects::Other(&Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::Subtype(Subtype::Merfolk),
        }),
        condition: None,
        modifications: &[
            Modification::ModifyPt(1, 1),
            Modification::AddKeywords(Keywords::ISLANDWALK),
        ],
    })])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const LORD: &str = Card::LordOfAtlantis;
    const MERFOLK: &str = Card::MerfolkOfThePearlTrident;
    const MIND_BEND: &str = Card::MindBend;

    // Merfolk of the Pearl Trident is a vanilla 1/1 Merfolk.
    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("other Merfolk get +1/+1, whoever controls them")
            .battlefield(P0, &[LORD, MERFOLK, Prop::Flier])
            .battlefield(P1, &[MERFOLK])
            .actions(&[
                ME.pt(LORD, 2, 2),
                ME.pt(MERFOLK, 2, 2),
                THEM.pt(MERFOLK, 2, 2),
                ME.pt(Prop::Flier, 3, 1),
            ]),
        Scenario::new("two Lords of Atlantis pump each other")
            .battlefield(P0, &[LORD])
            .battlefield(P1, &[LORD])
            .actions(&[ME.pt(LORD, 3, 3), THEM.pt(LORD, 3, 3)]),
        // P1 controls an Island, so the flier can't block the Merfolk.
        Scenario::new("other Merfolk have islandwalk")
            .battlefield(P0, &[LORD, MERFOLK])
            .battlefield(P1, &[Card::Island, Prop::Flier])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(MERFOLK),
                ME.done_attacking(),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(18),
            ]),
        // "Islandwalk" on the Lord reads "swampwalk": P1's Swamp makes the Merfolk
        // unblockable.
        Scenario::new("Mind Bend on the Lord gives other Merfolk swampwalk")
            .interaction(&[MIND_BEND])
            .battlefield(P0, &[LORD, MERFOLK, Card::Island])
            .battlefield(P1, &[Card::Swamp, Prop::Flier])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(LORD),
                Resolve,
                ME.chooses_word(TextWord::LandType(Subtype::Island)),
                ME.chooses_word(TextWord::LandType(Subtype::Swamp)),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(MERFOLK),
                ME.done_attacking(),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(18),
            ]),
        // With "islandwalk" read as "swampwalk", P1's Island no longer stops a block: the
        // flier blocks and kills the 2/2 Merfolk.
        Scenario::new("Mind Bend on the Lord takes islandwalk from other Merfolk")
            .interaction(&[MIND_BEND])
            .battlefield(P0, &[LORD, MERFOLK, Card::Island])
            .battlefield(P1, &[Card::Island, Prop::Flier])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(LORD),
                Resolve,
                ME.chooses_word(TextWord::LandType(Subtype::Island)),
                ME.chooses_word(TextWord::LandType(Subtype::Swamp)),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(MERFOLK),
                ME.done_attacking(),
                Action::choose(P1, DecisionKind::Blocker, Prop::Flier),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(20),
                ME.has(Zone::Graveyard, MERFOLK, 1),
            ]),
    ];
}
