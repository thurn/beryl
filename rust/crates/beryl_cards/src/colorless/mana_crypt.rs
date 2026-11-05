//! Mana Crypt. {0} Artifact. "At the beginning of your upkeep, flip a coin. If you lose the
//! flip, Mana Crypt deals 3 damage to you. {T}: Add {C}{C}."

use beryl_engine::card::{Ability, CardDef, Face, Flow, ManaColor, ManaSet, Trigger, Types};
use beryl_engine::effect::EffectCtx;
use beryl_engine::event::Recipient;
use beryl_engine::filter::Whose;
use beryl_engine::{TurnStep, Zone};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::names;

pub static CARD: CardDef = CardDef {
    name: names::MANA_CRYPT,
    faces: &[Face {
        types: Types::ARTIFACT,
        abilities: &[
            Ability::triggered(Trigger::Step(TurnStep::Upkeep, Whose::You), flip),
            Ability::tap_for_mana(ManaSet {
                colors: 1 << ManaColor::Colorless as u8,
                amount: 2,
            }),
        ],
        ..Face::EMPTY
    }],
    tokens: &[],
};

/// [CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052): "If the call matches the
/// result, the player wins the flip. Otherwise, the player loses the flip." The damage is
/// noncombat damage from Mana Crypt, which makes its controller lose that much life
/// ([CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a)): "Damage dealt to a
/// player by a source without infect causes that player to lose that much life."
fn flip(ctx: &mut EffectCtx) -> Flow {
    let you = ctx.controller();
    if !ctx.flip_coin(you) {
        ctx.deal_damage(Recipient::Player(you), 3);
    }
    Ok(())
}

/// From P1's turn 2 to P0's upkeep, where the flip's trigger waits on the stack. P0's pass
/// resolves it: P1 has nothing to do but pass, which the engine settles silently (§7.2).
const TO_UPKEEP: Action = Action::AdvanceTo {
    turn: 3,
    step: TurnStep::Upkeep,
};

/// The seeds are chosen so that the game's one coin flip comes out each way.
pub const SCENARIOS: &[Scenario] = &[
    Scenario::new("Mana Crypt's flip lost: 3 damage to its controller")
        .seed(LOST_SEED)
        .at(2, P1, TurnStep::PostcombatMain)
        .battlefield(P0, &[Permanent::new(names::MANA_CRYPT)])
        .library(P0, &[names::ISLAND])
        .actions(&[
            TO_UPKEEP,
            Action::pass(P0),
            Action::assert(Assertion::Life {
                player: P0,
                life: 17,
            }),
            Action::assert(Assertion::Life {
                player: P1,
                life: 20,
            }),
        ]),
    Scenario::new("Mana Crypt's flip won: no damage")
        .seed(WON_SEED)
        .at(2, P1, TurnStep::PostcombatMain)
        .battlefield(P0, &[Permanent::new(names::MANA_CRYPT)])
        .library(P0, &[names::ISLAND])
        .actions(&[
            TO_UPKEEP,
            Action::pass(P0),
            Action::assert(Assertion::Life {
                player: P0,
                life: 20,
            }),
        ]),
    // {C}{C} from Mana Crypt and {U} from an Island pay for Timetwister's {2}{U}. Neither
    // player can do anything else, so it resolves at once.
    Scenario::new("Mana Crypt's two colorless mana")
        .battlefield(
            P0,
            &[
                Permanent::new(names::MANA_CRYPT),
                Permanent::new(names::ISLAND),
            ],
        )
        .hand(P0, &[names::TIMETWISTER])
        .library(P0, &[names::PLAINS; 7])
        .library(P1, &[names::PLAINS; 7])
        .actions(&[
            Action::cast(P0, names::TIMETWISTER),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::MANA_CRYPT,
                count: 1,
            }),
            Action::assert(Assertion::Tapped {
                player: P0,
                card: names::ISLAND,
                count: 1,
            }),
            Action::assert(Assertion::Count {
                player: P0,
                zone: Zone::Graveyard,
                card: Some(names::TIMETWISTER),
                count: 1,
            }),
        ]),
];

const LOST_SEED: u64 = 2;
const WON_SEED: u64 = 0;
