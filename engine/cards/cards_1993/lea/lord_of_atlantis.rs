// Generated from the card store by `just card header`: notes go below the end marker.
//! Lord of Atlantis {U}{U}
//! Creature — Merfolk, 2/2
//!
//! Other Merfolk get +1/+1 and have islandwalk. (They can't be blocked as long as defending player
//! controls an Island.)
// End of the generated card header.

use crate::prelude::*;

/// Mind Bend on Lord of Atlantis can change the land type of the landwalk it grants
/// ([CR 612.2]): "A text-changing effect changes only those words that are used in the
/// correct way". Changing an affected Merfolk's text leaves the granted landwalk alone
/// ([CR 612.3]): "any abilities that are granted to an object can't be modified by
/// text-changing effects that affect that object".
///
/// [CR 612.2]: https://yawgatog.com/resources/magic-rules/#R6122
/// [CR 612.3]: https://yawgatog.com/resources/magic-rules/#R6123
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
