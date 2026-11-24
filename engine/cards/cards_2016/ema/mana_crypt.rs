// Generated from the card store by `just card header`: notes go below the end marker.
//! Mana Crypt {0}
//! Artifact
//!
//! At the beginning of your upkeep, flip a coin. If you lose the flip, this artifact deals 3 damage
//! to you.
//! {T}: Add {C}{C}.
// End of the generated card header.

use crate::prelude::*;

pub static CARD: CardDef = Printed::ManaCrypt
    .abilities(&[
        Ability::triggered(&Trigger::step(TurnStep::Upkeep, Players::You)).effect(&sequence(&[
            flip_coin(you()),
            if_then(if_you_lose_the_flip(), &deal_damage_to(you(), 3)),
        ])),
        Ability::tap_for_mana(ManaSet {
            colors: 1 << ManaColor::Colorless as u8,
            amount: 2,
        }),
    ])
    .card();

#[cfg(feature = "scenarios")]
pub mod scenarios {
    use crate::prelude::*;

    /// From P1's turn 2 to P0's upkeep, where the flip's trigger waits on the stack. P0's pass
    /// resolves it: P1 has nothing to do but pass, which the engine settles silently (§7.2).
    const TO_UPKEEP: Action = advance_to(3, TurnStep::Upkeep);

    /// The seeds are chosen so that the game's one coin flip comes out each way.
    pub const SCENARIOS: &[Scenario] =
        &[
            Scenario::new("Mana Crypt's flip lost: 3 damage to its controller")
                .seed(LOST_SEED)
                .at(2, P1, TurnStep::PostcombatMain)
                .battlefield(P0, &[Card::ManaCrypt])
                .actions(&[TO_UPKEEP, ME.passes(), ME.life(17), THEM.life(20)]),
        ];

    const LOST_SEED: u64 = 2;
}
