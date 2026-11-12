//! Damage ([CR 120](https://yawgatog.com/resources/magic-rules/#R120)).

use crate::event::Recipient;
use crate::fixed::Full;
use crate::game::Game;
use crate::layout::Layout;
use crate::state::ObjectRef;

/// Deals `amount` damage from `source` to a player or a permanent, combat damage with
/// `combat` ([CR 120.3](https://yawgatog.com/resources/magic-rules/#R1203)). A permanent that
/// has left the battlefield is dealt none.
///
/// [CR 120.3a](https://yawgatog.com/resources/magic-rules/#R1203a): "Damage dealt to a player
/// by a source without infect causes that player to lose that much life."
///
/// [CR 120.3c](https://yawgatog.com/resources/magic-rules/#R1203c): "Damage dealt to a
/// planeswalker causes that many loyalty counters to be removed from that planeswalker."
///
/// [CR 120.3e](https://yawgatog.com/resources/magic-rules/#R1203e): "Damage dealt to a
/// creature by a source with neither wither nor infect causes that much damage to be marked on
/// that creature."
#[inline]
pub fn deal<L: Layout>(
    game: &mut Game<'_, L>,
    source: ObjectRef<L>,
    to: Recipient<L>,
    amount: u8,
    combat: bool,
) -> Result<(), Full> {
    if amount == 0 {
        return Ok(());
    }
    let e = match to {
        Recipient::Player(p) => game.state.damage_player(source, p, amount, combat),
        Recipient::Permanent(r) => {
            if !game.on_battlefield(r) {
                return Ok(());
            }
            game.state.mark_damage(source, r, amount, combat)
        }
    };
    game.settle(e)
}
