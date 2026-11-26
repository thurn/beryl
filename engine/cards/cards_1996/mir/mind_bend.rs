// Generated from the card store by `just card header`: notes go below the end marker.
//! Mind Bend {U}
//! Instant
//!
//! Change the text of target permanent by replacing all instances of one color word with another or
//! one basic land type with another. (For example, you may change "nonblack creature" to "nongreen
//! creature" or "forestwalk" to "islandwalk." This effect lasts indefinitely.)
// End of the generated card header.

use crate::prelude::*;

/// A text change on a permanent leaves abilities granted by other objects alone
/// ([CR 612.3]): "any abilities that are granted to an object can't be modified by
/// text-changing effects that affect that object". For example, Mind Bend on a Merfolk
/// leaves its islandwalk from Lord of Atlantis alone.
///
/// [CR 612.3]: https://yawgatog.com/resources/magic-rules/#R6123
pub static CARD: CardDef = Printed::MindBend
    .abilities(&[Ability::spell()
        .target(&TargetSpec::permanent(Filter::Any))
        .effect(&change_text(
            target(),
            TextWords::ColorsAndLandTypes,
            Duration::Indefinite,
        ))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const MIND_BEND: &str = Card::MindBend;
    const COP: &str = Card::CircleOfProtectionRed;
    const RED: TextWord = TextWord::Color(Colors::RED);
    const WHITE: TextWord = TextWord::Color(Colors::WHITE);
    const FOREST: TextWord = TextWord::LandType(Subtype::Forest);
    const ISLAND: TextWord = TextWord::LandType(Subtype::Island);
    const MOUNTAIN: TextWord = TextWord::LandType(Subtype::Mountain);

    pub const SCENARIOS: &[Scenario] = &[
        // "Red" on Circle of Protection: Red reads "white", so on P1's next turn its ability
        // chooses the white flier and prevents its 3 combat damage. The change lasts
        // past the end of the turn it was made in.
        Scenario::new("Mind Bend makes Circle of Protection: Red a circle against white")
            .interaction(&[COP])
            .battlefield(P0, &[COP, Card::Island, Card::Plains])
            .battlefield(P1, &[Prop::Flier])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(COP),
                Resolve,
                ME.chooses_word(RED),
                ME.chooses_word(WHITE),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                ME.passes(),
                THEM.attacks_with(Prop::Flier),
                ME.activates(COP, 0),
                ME.picks(Prop::Flier),
                advance_to(2, TurnStep::EndOfCombat),
                ME.life(20),
            ]),
        // Circle of Protection: Red's ability went on the stack before Mind Bend changed
        // "red": it keeps the text its source had then (CR 113.7a), so it chooses among red
        // sources: P1's Hill Giant is the only one, so P0 is asked nothing and gets priority.
        // Read as "white", it would ask between the flier and the Circle itself.
        Scenario::new("an ability on the stack keeps the words its source had")
            .interaction(&[COP])
            .battlefield(P0, &[COP, Card::Island, Card::Plains])
            .battlefield(P1, &[Card::HillGiant, Prop::Flier])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.activates(COP, 0),
                ME.casts(MIND_BEND),
                ME.targets(COP),
                Resolve,
                ME.chooses_word(RED),
                ME.chooses_word(WHITE),
                Resolve,
                ME.deciding(DecisionKind::Priority, 1),
            ]),
        // "Mountains" on Blood Moon reads "Islands": Tundra is an Island, so it pays the {U}
        // of a second Mind Bend, which as a Mountain it could not.
        Scenario::new("Mind Bend makes Blood Moon's nonbasic lands Islands")
            .interaction(&[Card::BloodMoon])
            .battlefield(P0, &[Card::BloodMoon, Card::Island, Card::Tundra])
            .hand(P0, &[MIND_BEND, MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(Card::BloodMoon),
                Resolve,
                ME.chooses_word(MOUNTAIN),
                ME.chooses_word(ISLAND),
                ME.casts(MIND_BEND),
                ME.targets(Card::BloodMoon),
                ME.tapped(Card::Tundra, 1),
            ]),
        // A Forest whose "Forest" reads "Island" is an Island, which taps for {U}
        // (CR 305.6): it pays for a second Mind Bend.
        Scenario::new("Mind Bend makes a Forest an Island")
            .battlefield(P0, &[Card::Forest, Card::Island])
            .hand(P0, &[MIND_BEND, MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(Card::Forest),
                Resolve,
                ME.chooses_word(FOREST),
                ME.chooses_word(ISLAND),
                ME.casts(MIND_BEND),
                ME.targets(Card::Forest),
                ME.controls(Card::Forest, 1),
                ME.tapped(Card::Forest, 1),
            ]),
        // Urborg's "Swamp" reads "Island": every land is an Island and no land is a Swamp, so
        // each of P0's untapped lands taps for {U} (CR 305.7) and a second Mind Bend can be cast.
        Scenario::new("Mind Bend makes Urborg give every land the Island type")
            .interaction(&[Card::UrborgTombOfYawgmoth])
            .battlefield(
                P0,
                &[Card::UrborgTombOfYawgmoth, Card::Island, Card::Plains],
            )
            .hand(P0, &[MIND_BEND, MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(Card::UrborgTombOfYawgmoth),
                Resolve,
                ME.chooses_word(TextWord::LandType(Subtype::Swamp)),
                ME.chooses_word(ISLAND),
                ME.has_subtype(Card::Plains, Subtype::Island, true),
                ME.has_subtype(Card::Plains, Subtype::Swamp, false),
                ME.casts(MIND_BEND),
                ME.targets(Card::Island),
            ]),
        // [CR 400.7]: an object that changes zones becomes a new object with no memory of
        // its previous existence, so a text change on a Forest ends when Flickerwisp exiles it
        // and it returns.
        //
        // [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
        Scenario::new("a text change ends when the permanent leaves and returns")
            .interaction(&[Card::Flickerwisp])
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[MIND_BEND, Card::Flickerwisp])
            .library(P0, &[Card::Plains])
            .library(P1, &[Card::Plains])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(Card::Forest),
                Resolve,
                ME.chooses_word(FOREST),
                ME.chooses_word(ISLAND),
                ME.has_subtype(Card::Forest, Subtype::Island, true),
                ME.casts(Card::Flickerwisp),
                Resolve,
                ME.targets(Card::Forest),
                Resolve,
                advance_to(1, TurnStep::End),
                Resolve,
                ME.has_subtype(Card::Forest, Subtype::Forest, true),
                ME.has_subtype(Card::Forest, Subtype::Island, false),
            ]),
        // [CR 612.3]: text changes on a creature don't touch an ability another object
        // granted it. The Lord grants islandwalk; changing "Island" on the Merfolk itself
        // changes nothing, so P1's lone Island still keeps the flier from blocking.
        //
        // [CR 612.3]: https://yawgatog.com/resources/magic-rules/#R6123
        Scenario::new("Mind Bend on a Merfolk doesn't change the islandwalk the Lord gave")
            .interaction(&[Card::LordOfAtlantis])
            .battlefield(
                P0,
                &[
                    Card::LordOfAtlantis,
                    Card::MerfolkOfThePearlTrident,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[Card::Island, Prop::Flier])
            .hand(P0, &[MIND_BEND])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(Card::MerfolkOfThePearlTrident),
                Resolve,
                ME.chooses_word(ISLAND),
                ME.chooses_word(TextWord::LandType(Subtype::Swamp)),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(Card::MerfolkOfThePearlTrident),
                ME.done_attacking(),
                advance_to(1, TurnStep::EndOfCombat),
                THEM.life(18),
            ]),
        // A permanent with no matching words is a legal target. Word choices start
        // when the spell resolves, and the replacement menu omits the first word.
        Scenario::new("Mind Bend can target a permanent with no matching words")
            .battlefield(P0, &[Card::Island])
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P0, &[MIND_BEND, Card::Plains])
            .actions(&[
                ME.casts(MIND_BEND),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.deciding(DecisionKind::ChooseObject, 10),
                ME.chooses_word(ISLAND),
                ME.deciding(DecisionKind::ChooseObject, 4),
                ME.chooses_word(TextWord::LandType(Subtype::Swamp)),
                THEM.controls(Prop::Vanilla, 1),
            ]),
    ];
}
