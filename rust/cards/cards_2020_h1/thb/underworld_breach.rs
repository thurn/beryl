// Generated from the card store by `just card header`: notes go below the end marker.
//! Underworld Breach {1}{R}
//! Enchantment
//!
//! Each nonland card in your graveyard has escape. The escape cost is equal to the card's mana
//! cost plus exile three other cards from your graveyard. (You may cast cards from your graveyard
//! for their escape cost.)
//! At the beginning of the end step, sacrifice this enchantment.
// End of the generated card header.

use crate::prelude::*;

/// "Each nonland card in your graveyard has escape": a static ability in layer 6 over cards
/// in a graveyard, which reaches them as an effect on objects off the battlefield does
/// ([CR 611.3b]): "The effect applies at all times that the permanent generating it is on
/// the battlefield". Escape lets the card's owner cast it from there for its escape cost
/// ([CR 702.138a]): "'Escape [cost]' means 'You may cast this card from your graveyard by
/// paying [cost] rather than paying its mana cost.'" The escape cost is an alternative cost
/// ([CR 118.9a]): "Only one alternative cost can be applied to any one spell as it's being
/// cast."
///
/// "At the beginning of the end step, sacrifice this enchantment.": every player's end step
/// ([CR 603.2b]): "When a phase or step begins, all abilities that trigger 'at the beginning
/// of' that phase or step trigger."
///
/// [CR 611.3b]: https://yawgatog.com/resources/magic-rules/#R6113b
/// [CR 702.138a]: https://yawgatog.com/resources/magic-rules/#R702138a
/// [CR 118.9a]: https://yawgatog.com/resources/magic-rules/#R1189a
/// [CR 603.2b]: https://yawgatog.com/resources/magic-rules/#R6032b
pub static CARD: CardDef = Printed::UnderworldBreach
    .abilities(&[
        Ability::static_ability(&LayerSpec {
            affected: Objects::Everywhere(Filter::All(&[
                Filter::Not(&Filter::Types(Types::LAND)),
                Filter::Whose(&Players::You),
                Filter::InZone(Zone::Graveyard),
            ])),
            condition: None,
            modifications: &[Modification::GrantEscape],
        }),
        Ability::triggered(&Trigger::step(TurnStep::End, Players::EachPlayer))
            .effect(&sacrifice(this())),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const BREACH: &str = Card::UnderworldBreach;
    const LED: &str = Card::LionsEyeDiamond;
    const SNAPCASTER: &str = Card::SnapcasterMage;
    const FREEZE: &str = Card::BrainFreeze;

    /// Lion's Eye Diamond, sacrificed for {U}{U}{U}.
    const BLUE: [Action; 2] = [ME.activates(LED, 0), ME.chooses_color(Colors::BLUE)];

    /// Escapes Lion's Eye Diamond and lets it resolve. Of the other cards in the graveyard,
    /// Brain Freeze lies on top, so P0 picks an Island first and the rest follow.
    const ESCAPE_LED: [Action; 3] = [ME.escapes(LED), ME.picks(Card::Island), Resolve];

    /// The Breach loop (decision R13 builds no shortcut): Lion's Eye Diamond and Brain Freeze
    /// cast, then escaped in turn, the Diamond sacrificed for {U}{U}{U} with each Brain
    /// Freeze on the stack. Each round exiles six cards of the graveyard, returns the two
    /// spells to it, gains one mana, and its Brain Freeze has two more copies than the last.
    /// Rounds 1 and 2 mill P0 (six and twelve cards), round 3 two copies' worth of P0 and
    /// four of P1 (twelve), and rounds 4 and 5 only P1 (twenty-four, then the last
    /// seventeen of thirty). The whole turn stays inside the decision budget the scenario
    /// runs under (`Limits::DEFAULT`, 500 decisions a turn).
    const LOOP: &[Action] = &[
        // Round 1: storm 1.
        ME.casts(LED),
        Resolve,
        ME.casts(FREEZE),
        ME.targets_player(ME),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(ME),
        Resolve,
        Resolve,
        ME.has_cards(Zone::Graveyard, 8),
        ME.mana(ManaColor::Blue, 3),
        // Round 2: storm 3.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(ME),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(ME),
        ME.targets_player(ME),
        ME.targets_player(ME),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        ME.has_cards(Zone::Graveyard, 14),
        ME.mana(ManaColor::Blue, 4),
        // Round 3: storm 5.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(THEM),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(ME),
        ME.targets_player(ME),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        THEM.has_cards(Zone::Library, 41),
        ME.has_cards(Zone::Graveyard, 14),
        ME.mana(ManaColor::Blue, 5),
        // Round 4: storm 7.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(THEM),
        BLUE[0],
        BLUE[1],
        Resolve,
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        THEM.has_cards(Zone::Library, 17),
        ME.has_cards(Zone::Graveyard, 8),
        ME.mana(ManaColor::Blue, 6),
        // Round 5: storm 9, and P1's library is empty.
        ESCAPE_LED[0],
        ESCAPE_LED[1],
        ESCAPE_LED[2],
        ME.escapes(FREEZE),
        ME.targets_player(THEM),
        Resolve,
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        ME.targets_player(THEM),
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        Resolve,
        THEM.has_cards(Zone::Library, 0),
        ME.mana(ManaColor::Blue, 4),
    ];

    pub const SCENARIOS: &[Scenario] = &[
        Scenario::new("the Breach loop empties a 53-card library")
            .interaction(&[LED, FREEZE])
            .battlefield(P0, &[BREACH, Card::Island, Card::Island])
            .hand(P0, &[LED, FREEZE])
            .library(P0, &[Card::Island; 30])
            .library(P1, &[Card::Plains; 53])
            .actions(LOOP),
        // Ruling: "If it is a permanent spell, it enters the battlefield". Its escape cost is
        // its mana cost, {1}{U}, and the three other cards in the graveyard, which are exiled.
        Scenario::new("a creature card escapes and enters the battlefield")
            .battlefield(P0, &[BREACH, Card::Island, Card::Island])
            .graveyard(P0, &[Prop::Vanilla, Card::Plains, Card::Plains, Card::Plains])
            .actions(&[
                ME.escapes(Prop::Vanilla),
                Resolve,
                ME.controls(Prop::Vanilla, 1),
                ME.has(Zone::Exile, Card::Plains, 3),
                ME.tapped(Card::Island, 2),
                ME.has_cards(Zone::Graveyard, 0),
            ]),
        // Ruling: "After an escaped spell resolves, it returns to its owner's graveyard if
        // it's not a permanent spell." With four other cards, P0 chooses the three to exile,
        // one at a time in the graveyard's order, top first: the Swamp, then an Island, and the
        // last Island is the only choice left. The Plains stays.
        Scenario::new("an escaped sorcery returns to the graveyard")
            .battlefield(P0, &[BREACH, Card::Plains])
            .graveyard(
                P0,
                &[Prop::Sorcery, Card::Island, Card::Island, Card::Plains, Card::Swamp],
            )
            .battlefield(P1, &[Prop::Vanilla])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.escapes(Prop::Sorcery),
                ME.picks(Card::Swamp),
                ME.picks(Card::Island),
                stack(1),
                Resolve,
                THEM.controls(Prop::Vanilla, 0),
                ME.has(Zone::Graveyard, Prop::Sorcery, 1),
                ME.has(Zone::Graveyard, Card::Plains, 1),
                ME.has(Zone::Exile, Card::Island, 2),
                ME.has(Zone::Exile, Card::Swamp, 1),
            ]),
        // Two other cards are not enough to pay the escape cost, a land card has no escape,
        // and neither has a card in an opponent's graveyard: P0 may only pass or play the
        // Island in their hand.
        Scenario::new("escape needs three other cards, a nonland card and your graveyard")
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::Island])
            .graveyard(P0, &[Prop::Vanilla, Card::Plains, Card::Plains])
            .graveyard(
                P1,
                &[Prop::Flier, Card::Plains, Card::Plains, Card::Plains, Card::Plains],
            )
            .actions(&[ME.deciding(DecisionKind::Priority, 2)]),
        // Ruling: "Escape's permission doesn't change when you may cast the spell from your
        // graveyard." On P1's turn, P0 escapes an instant in response to P1's creature
        // spell, and not the sorcery beside it.
        Scenario::new("an instant escapes at instant speed, a sorcery doesn't")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Plains])
            .graveyard(
                P0,
                &[
                    Prop::Instant,
                    Prop::Sorcery,
                    Card::Swamp,
                    Card::Swamp,
                    Card::Swamp,
                ],
            )
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                THEM.casts(Prop::Vanilla),
                ME.deciding(DecisionKind::Priority, 2),
                ME.escapes(Prop::Instant),
                // The Swamps, not Oust.
                ME.picks(Card::Swamp),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.has(Zone::Graveyard, Prop::Instant, 1),
            ]),
        // Ruling: "If a card has multiple abilities giving you permission to cast it, such as
        // ... an escape ability and a flashback ability, you choose which one to apply. The
        // others have no effect." Snapcaster gives Oust flashback; P0 may cast it with
        // flashback or with escape, and escapes it, so it returns to the graveyard rather
        // than being exiled as a flashback spell would be.
        Scenario::new("escape or flashback: the one chosen applies")
            .interaction(&[SNAPCASTER])
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Plains])
            .hand(P0, &[SNAPCASTER])
            .graveyard(P0, &[Prop::Sorcery, Card::Swamp, Card::Swamp, Card::Swamp])
            .battlefield(P1, &[Prop::Vanilla])
            .library(P1, &[Card::Island, Card::Island])
            .actions(&[
                ME.casts(SNAPCASTER),
                Resolve,
                Resolve,
                // Passing, Oust with flashback and Oust with escape.
                ME.deciding(DecisionKind::Priority, 3),
                ME.escapes(Prop::Sorcery),
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.controls(Prop::Vanilla, 0),
                ME.has(Zone::Graveyard, Prop::Sorcery, 1),
                ME.has(Zone::Exile, Prop::Sorcery, 0),
            ]),
        // Ruling: "If you cast a spell with its escape permission, you can't choose to apply
        // any other alternative costs". Force of Will's own alternative cost (a blue card from
        // hand and 1 life) can't be added to its escape: P0 may pass, cast Counterspell from
        // their hand, or escape Force of Will for {3}{U}{U}.
        Scenario::new("escape is the one alternative cost")
            .interaction(&[Card::ForceOfWill])
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[BREACH, Card::Island, Card::Island, Card::Island, Card::Island, Card::Island])
            .hand(P0, &[Card::Counterspell])
            .graveyard(
                P0,
                &[Card::ForceOfWill, Card::Plains, Card::Plains, Card::Plains],
            )
            .battlefield(P1, &[Card::Island, Card::Island])
            .hand(P1, &[Prop::Vanilla])
            .actions(&[
                THEM.casts(Prop::Vanilla),
                ME.deciding(DecisionKind::Priority, 3),
                ME.escapes(Card::ForceOfWill),
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                ME.has(Zone::Hand, Card::Counterspell, 1),
                ME.life(20),
            ]),
        // Ruling: "To determine the total cost of a spell, start with the mana cost or
        // alternative cost you're paying (such as an escape cost), add any cost increases".
        // Under Trinisphere, Lion's Eye Diamond's escape cost of {0} becomes {3}.
        Scenario::new("a cost increase applies to the escape cost")
            .interaction(&[Card::Trinisphere, LED])
            .battlefield(
                P0,
                &[BREACH, Card::Trinisphere, Card::Island, Card::Island, Card::Island],
            )
            .graveyard(P0, &[LED, Card::Plains, Card::Plains, Card::Plains])
            .actions(&[
                ME.escapes(LED),
                Resolve,
                ME.controls(LED, 1),
                ME.tapped(Card::Island, 3),
            ]),
        // It is sacrificed at the beginning of the end step: P0's own.
        Scenario::new("it is sacrificed at the beginning of the end step")
            .battlefield(P0, &[BREACH])
            .actions(&[
                advance_to(1, TurnStep::End),
                Resolve,
                ME.controls(BREACH, 0),
                ME.has(Zone::Graveyard, BREACH, 1),
            ]),
        // And at the beginning of an opponent's end step.
        Scenario::new("it is sacrificed at the beginning of an opponent's end step")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[BREACH])
            .actions(&[
                advance_to(2, TurnStep::End),
                Resolve,
                ME.controls(BREACH, 0),
                ME.has(Zone::Graveyard, BREACH, 1),
            ]),
    ];
}
