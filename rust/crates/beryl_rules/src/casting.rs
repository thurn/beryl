//! Casting spells ([CR 601](https://yawgatog.com/resources/magic-rules/#R601)) as an effect
//! allows while it resolves.

use crate::ids::Seat;
use crate::layout::Layout;
use crate::resolution::Resolution;
use crate::state::ObjectRef;

/// Lets `player` cast the card `object` as an effect of what is resolving, without paying
/// its mana cost when `free`
/// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin to
/// cast a spell only if a rule or effect allows that player to cast it". The effect's last
/// action: the card is offered as the resolution finishes, which is when a spell cast
/// during it would otherwise have mattered, since "no player receives priority after it's
/// cast" ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)). The engine asks
/// then whether, and how, the player casts it.
pub fn offer<L: Layout>(
    resolution: &mut Resolution<'_, L>,
    object: ObjectRef<L>,
    player: Seat,
    free: bool,
) {
    resolution.offer = Some((object, player, free));
}
