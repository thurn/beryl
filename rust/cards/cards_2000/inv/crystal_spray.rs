// Generated from the card store by `just card header`: notes go below the end marker.
//! Crystal Spray {2}{U}
//! Instant
//!
//! Change the text of target spell or permanent by replacing all instances of one color word with
//! another or one basic land type with another until end of turn.
//! Draw a card.
// End of the generated card header.

use crate::prelude::*;

/// "Change the text of target spell or permanent by replacing all instances of one color word
/// with another or one basic land type with another until end of turn." A text-changing effect
/// ([CR 612.1]): "Some continuous effects change an object’s text." Its controller chooses the
/// two words as it resolves. A permanent spell keeps the change as the permanent it becomes
/// ([CR 400.7a]): "Effects from spells, activated abilities, and triggered abilities that
/// change the characteristics or controller of a permanent spell on the stack continue to
/// apply to the permanent that spell becomes." The change ends in the cleanup step
/// ([CR 514.2]): "the following actions happen simultaneously: ... all “until end of turn” and
/// “this turn” effects end."
///
/// "Draw a card." follows.
///
/// [CR 400.7a]: https://yawgatog.com/resources/magic-rules/#R4007a
/// [CR 514.2]: https://yawgatog.com/resources/magic-rules/#R5142
/// [CR 612.1]: https://yawgatog.com/resources/magic-rules/#R6121
pub static CARD: CardDef = Printed::CrystalSpray
    .abilities(&[Ability::spell()
        .target(&TargetSpec::one(TargetKind::SpellOrPermanent, Filter::Any))
        .effect(&sequence(&[
            change_text(target(), Duration::EndOfTurn),
            draw(you(), 1),
        ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const SPRAY: &str = Card::CrystalSpray;
    const NACATL: &str = Card::WildNacatl;
    const MOUNTAIN: TextWord = TextWord::LandType(Subtype::Mountain);
    const ISLAND: TextWord = TextWord::LandType(Subtype::Island);

    pub const SCENARIOS: &[Scenario] = &[
        // P1's Wild Nacatl's "Mountain" reads "Island", so P1's Island gives it +1/+1, until
        // the cleanup step ends the change. Crystal Spray draws a card too. (A spell's change that
        // goes on with the permanent it becomes is a test of `beryl_cards`, `tests::text`:
        // XMage has no Crystal Spray and Forge drops the change, so no oracle can show it.)
        Scenario::new("Crystal Spray changes a permanent's text until end of turn")
            .interaction(&[NACATL])
            .battlefield(P0, &[Card::Island, Card::Island, Card::Island])
            .battlefield(P1, &[NACATL, Card::Island])
            .hand(P0, &[SPRAY])
            .library(P0, &[Card::Island, Card::Island, Card::Island])
            .actions(&[
                THEM.pt(NACATL, 1, 1),
                ME.casts(SPRAY),
                ME.targets(NACATL),
                Resolve,
                ME.chooses_word(MOUNTAIN),
                ME.chooses_word(ISLAND),
                ME.has_cards(Zone::Hand, 1),
                THEM.pt(NACATL, 2, 2),
                advance_to(2, TurnStep::Upkeep),
                THEM.pt(NACATL, 1, 1),
            ]),
        // P0's Forest is an Island until end of turn, and taps for the {U} of a second
        // Crystal Spray.
        Scenario::new("Crystal Spray makes a Forest an Island")
            .battlefield(
                P0,
                &[
                    Card::Forest,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                    Card::Plains,
                    Card::Plains,
                ],
            )
            .hand(P0, &[SPRAY, SPRAY])
            .library(P0, &[Card::Island, Card::Island, Card::Island])
            .actions(&[
                ME.casts(SPRAY),
                ME.targets(Card::Forest),
                Resolve,
                ME.chooses_word(TextWord::LandType(Subtype::Forest)),
                ME.chooses_word(ISLAND),
                ME.casts(SPRAY),
                ME.targets(Card::Forest),
                ME.controls(Card::Forest, 1),
                ME.tapped(Card::Forest, 1),
            ]),
        // A permanent with no matching words is a legal target. Word choices start
        // when the spell resolves, and the replacement menu omits the first word.
        Scenario::new("Crystal Spray can target a permanent with no matching words")
            .battlefield(P0, &[Card::Island; 3])
            .battlefield(P1, &[Prop::Vanilla])
            .hand(P0, &[SPRAY])
            .library(P0, &[Card::Plains])
            .actions(&[
                ME.casts(SPRAY),
                ME.targets(Prop::Vanilla),
                Resolve,
                ME.deciding(DecisionKind::ChooseObject, 10),
                ME.chooses_word(ISLAND),
                ME.deciding(DecisionKind::ChooseObject, 4),
                ME.chooses_word(TextWord::LandType(Subtype::Swamp)),
                THEM.controls(Prop::Vanilla, 1),
                ME.has(Zone::Hand, Card::Plains, 1),
            ]),
        // The Lord grants islandwalk to the Merfolk, so changing the Merfolk's own text
        // still leaves it able to attack unblocked against a player who controls an Island.
        Scenario::new("Crystal Spray leaves a granted landwalk ability alone")
            .interaction(&[Card::LordOfAtlantis])
            .battlefield(
                P0,
                &[
                    Card::LordOfAtlantis,
                    Card::MerfolkOfThePearlTrident,
                    Card::Island,
                    Card::Island,
                    Card::Island,
                ],
            )
            .battlefield(P1, &[Card::Island, Prop::Flier])
            .hand(P0, &[SPRAY])
            .actions(&[
                ME.casts(SPRAY),
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
    ];
}
