//! Mana ([CR 106](https://yawgatog.com/resources/magic-rules/#R106)).

use beryl_engine::TurnStep;
use beryl_engine::card::ManaColor;
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Scenario};

use crate::{names, props};

/// Mana a triggered ability adds is in its controller's pool like any other
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect instructs
/// a player to add mana, that mana goes into a player's mana pool." Unspent, it is gone once
/// the phase has ended (CR 500.5). The amount is the one settled as the ability was made:
/// three, the mana value of the sorcery countered.
pub const SCENARIOS: &[Scenario] =
    &[
        Scenario::new("Mana a triggered ability adds is gone once its phase has ended")
            .at(2, P1, TurnStep::PrecombatMain)
            .battlefield(P0, &[Permanent::new(names::ISLAND); 2])
            .hand(P0, &[names::MANA_DRAIN])
            .library(P0, &[names::PLAINS, names::PLAINS])
            .battlefield(P1, &[Permanent::new(names::ISLAND); 3])
            .hand(P1, &[props::UNTARGETED_SPELL])
            .actions(&[
                Action::cast(P1, props::UNTARGETED_SPELL),
                Action::cast(P0, names::MANA_DRAIN),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::PrecombatMain,
                },
                Action::pass(P0),
                Action::assert(Assertion::Mana {
                    player: P0,
                    color: ManaColor::Colorless,
                    amount: 3,
                }),
                Action::AdvanceTo {
                    turn: 3,
                    step: TurnStep::BeginningOfCombat,
                },
                Action::assert(Assertion::Mana {
                    player: P0,
                    color: ManaColor::Colorless,
                    amount: 0,
                }),
            ]),
    ];
