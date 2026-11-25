// Generated from the card store by `just card header`: notes go below the end marker.
//! Brisela, Voice of Nightmares
//! Legendary Creature — Eldrazi Angel, 9/10
//!
//! Flying, first strike, vigilance, lifelink
//! Your opponents can't cast spells with mana value 3 or less.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = card_with_shape(
    Card::BriselaVoiceOfNightmares,
    PrintedShape::BriselaVoiceOfNightmares,
    &[Printed::BriselaVoiceOfNightmares
        .keywords(
            Keywords::FLYING
                .union(Keywords::FIRST_STRIKE)
                .union(Keywords::VIGILANCE)
                .union(Keywords::LIFELINK),
        )
        .abilities(&[Ability::rule_change(&RuleChange {
            players: Players::EachOpponent,
            what: Governed::CastSpells(Filter::ManaValueAtMost(3)),
            when: When::Never,
        })])],
);

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const GISELA: &str = Card::GiselaTheBrokenBlade;
    const BRUNA: &str = Card::BrunaTheFadingLight;
    const BRISELA: &str = Card::BriselaVoiceOfNightmares;

    /// Brisela is never a card of its own: P0's Gisela and Bruna meld into it at P0's first
    /// end step ([`MELD`]). Each player has cards to draw until P0's second turn.
    const MELDED: Scenario = Scenario::setup()
        .interaction(&[GISELA, BRUNA])
        .at(1, P0, TurnStep::PostcombatMain)
        .permanents(P0, &[Permanent::new(GISELA), Permanent::new(BRUNA)])
        .library(P0, &[Card::Plains, Card::Plains])
        .library(P1, &[Card::Island, Card::Island]);

    /// The actions that meld Gisela and Bruna at P0's first end step.
    const MELD: [Action; 2] = [advance_to(1, TurnStep::End), Resolve];

    pub const SCENARIOS: &[Scenario] = &[
        // P0 may still cast Get Lost (mana value 2) on P1's 2/1. Then P1 may cast Jace (mana
        // value 4) but not Flickerwisp (mana value 3).
        MELDED
            .titled("Brisela's opponents can't cast spells with mana value 3 or less")
            .interaction(&[GISELA, BRUNA, Card::GetLost, Card::Flickerwisp])
            .battlefield(P0, &[Card::Plains, Card::Plains])
            .hand(P0, &[Card::GetLost])
            .battlefield(
                P1,
                &[Prop::Vanilla, Card::Plains, Card::Plains, Card::Island],
            )
            .permanents(P1, &[Permanent::new(Card::Island)])
            .hand(P1, &[Card::Flickerwisp, Card::JaceTheMindSculptor])
            .actions(&[
                MELD[0],
                MELD[1],
                ME.casts(Card::GetLost),
                ME.targets(Prop::Vanilla),
                Resolve,
                THEM.has(Zone::Graveyard, Prop::Vanilla, 1),
                advance_to(2, TurnStep::PrecombatMain),
                // Passing, playing the Island P1 drew, or casting Jace.
                THEM.deciding(DecisionKind::Priority, 3),
            ]),
        // P1's 2/1 can't block the flier. Brisela has first strike, so combat has a first
        // strike damage step, where it deals 9 damage and its controller gains 9 life; with
        // vigilance it attacked without tapping. P0 holds a flash creature it could cast, so
        // it has priority in each step of combat.
        MELDED
            .titled("Brisela attacks with flying, first strike, vigilance and lifelink")
            .battlefield(P0, &[Card::Swamp, Card::Swamp, Card::Swamp])
            .hand(P0, &[Prop::FlashCreature])
            .battlefield(P1, &[Prop::Vanilla])
            .actions(&[
                MELD[0],
                MELD[1],
                // P1's 2/1 stays home on turn 2.
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                ME.passes(),
                THEM.done_attacking(),
                advance_to(3, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(BRISELA),
                ME.tapped(BRISELA, 0),
                advance_to(3, TurnStep::FirstStrikeDamage),
                THEM.life(11),
                ME.life(29),
            ]),
    ];
}
