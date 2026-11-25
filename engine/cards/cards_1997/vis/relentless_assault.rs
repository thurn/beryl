// Generated from the card store by `just card header`: notes go below the end marker.
//! Relentless Assault {2}{R}{R}
//! Sorcery
//!
//! Untap all creatures that attacked this turn. After this main phase, there is an additional
//! combat phase followed by an additional main phase.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::RelentlessAssault
    .abilities(&[Ability::spell().effect(&sequence(&[
        Effect::Untap(Objects::All {
            zone: Zone::Battlefield,
            filter: Filter::All(&[Filter::Types(Types::CREATURE), Filter::AttackedThisTurn]),
        }),
        Effect::AdditionalPhases {
            after: TurnPhase::Main,
            phases: &[TurnPhase::Combat, TurnPhase::Main],
        },
    ]))])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    const ASSAULT: &str = Card::RelentlessAssault;
    const MOUNTAIN: &str = Card::Mountain;
    const KNIGHT: &str = Card::WhiteKnight;
    const ANGEL: &str = Card::SerraAngel;
    const OVERLORD: &str = Card::OverlordOfTheMistmoors;
    const TEFERI: &str = Card::TeferiTimeRaveler;
    const GROUND: &str = Prop::Vanilla;
    const FLIER: &str = Prop::Flier;
    const BASE: Scenario = Scenario::setup().hand(P0, &[ASSAULT]);

    pub const SCENARIOS: &[Scenario] = &[
        BASE.titled("only declared attackers untap and can attack again")
            .permanents(
                P0,
                &[
                    Permanent::new(MOUNTAIN),
                    Permanent::new(MOUNTAIN),
                    Permanent::new(MOUNTAIN),
                    Permanent::new(MOUNTAIN),
                    Permanent::new(GROUND),
                    Permanent::new(FLIER).tapped(),
                ],
            )
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(GROUND),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(18),
                ME.attacking(GROUND, 0),
                ME.casts(ASSAULT),
                Resolve,
                ME.tapped(GROUND, 0),
                ME.tapped(FLIER, 1),
                ME.tapped(MOUNTAIN, 4),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(GROUND),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(16),
                ME.tapped(GROUND, 1),
            ]),
        BASE.titled("a precombat cast puts a main phase between two combats")
            .interaction(&[ANGEL])
            .hand(P0, &[ASSAULT, Card::Island])
            .battlefield(P0, &[MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, ANGEL])
            .actions(&[
                ME.casts(ASSAULT),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(ANGEL),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(16),
                ME.tapped(ANGEL, 0),
                ME.plays(Card::Island),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(ANGEL),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(12),
                ME.controls(Card::Island, 1),
            ]),
        BASE.titled("two casts after the same main phase give two more combats")
            .interaction(&[ANGEL])
            .hand(P0, &[ASSAULT, ASSAULT])
            .battlefield(
                P0,
                &[
                    MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN,
                    ANGEL,
                ],
            )
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(ANGEL),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(16),
                ME.casts(ASSAULT),
                Resolve,
                ME.casts(ASSAULT),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(ANGEL),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(12),
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(ANGEL),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(8),
            ]),
        BASE.titled("an attack trigger fires in both combats")
            .interaction(&[OVERLORD])
            .battlefield(P0, &[MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, OVERLORD])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(OVERLORD),
                Resolve,
                ME.controls(crate::tokens::INSECT_TOKEN, 2),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(14),
                ME.casts(ASSAULT),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(OVERLORD),
                Resolve,
                ME.controls(crate::tokens::INSECT_TOKEN, 4),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(8),
            ]),
        BASE.titled("a first striker can be blocked in the second combat")
            .interaction(&[KNIGHT])
            .battlefield(P0, &[MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, KNIGHT])
            .battlefield(P1, &[GROUND])
            .actions(&[
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(KNIGHT),
                THEM.answers(DecisionKind::Blocker, Pick::Done),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(18),
                ME.casts(ASSAULT),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(KNIGHT),
                THEM.chooses(DecisionKind::Blocker, GROUND),
                advance_to(1, TurnStep::FirstStrikeDamage),
                THEM.has(Zone::Graveyard, GROUND, 1),
                advance_to(1, TurnStep::PostcombatMain),
                ME.controls(KNIGHT, 1),
                THEM.life(18),
            ]),
        BASE.titled("a cast outside a main phase untaps without adding phases")
            .interaction(&[TEFERI])
            .battlefield(
                P0,
                &[MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, TEFERI, GROUND],
            )
            .actions(&[
                ME.activates(TEFERI, 1),
                Resolve,
                advance_to(1, TurnStep::BeginningOfCombat),
                ME.passes(),
                ME.attacks_with(GROUND),
                advance_to(1, TurnStep::EndOfCombat),
                ME.casts(ASSAULT),
                Resolve,
                ME.tapped(GROUND, 0),
                advance_to(1, TurnStep::PostcombatMain),
                THEM.life(18),
            ]),
        BASE.titled("the opponent gets the extra combat on their turn")
            .interaction(&[TEFERI])
            .battlefield(P0, &[MOUNTAIN, MOUNTAIN, MOUNTAIN, MOUNTAIN, TEFERI])
            .battlefield(P1, &[GROUND])
            .actions(&[
                ME.activates(TEFERI, 1),
                Resolve,
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                ME.passes(),
                THEM.attacks_with(GROUND),
                THEM.answers(DecisionKind::AttackTarget, Pick::DefendingPlayer),
                advance_to(2, TurnStep::PostcombatMain),
                ME.life(18),
                THEM.passes(),
                ME.casts(ASSAULT),
                THEM.tapped(GROUND, 0),
                advance_to(2, TurnStep::BeginningOfCombat),
                THEM.passes(),
                THEM.attacks_with(GROUND),
                THEM.answers(DecisionKind::AttackTarget, Pick::DefendingPlayer),
                advance_to(2, TurnStep::PostcombatMain),
                ME.life(16),
            ]),
    ];
}
