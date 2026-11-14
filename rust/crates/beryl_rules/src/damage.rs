//! Damage ([CR 120](https://yawgatog.com/resources/magic-rules/#R120)).

use beryl_card::{Amount, Keywords, Recipients};

use crate::bitboard::Bitboard;
use crate::event::Recipient;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::Layout;
use crate::life;
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
    let lifelink = lifelink(game, source);
    game.settle(e)?;
    match lifelink {
        Some(controller) => life::gain(game, controller, amount),
        None => Ok(()),
    }
}

/// Who gains the life a source's damage gives for its lifelink, if it has lifelink
/// ([CR 702.15b]): "Damage dealt by a source with lifelink causes that source's controller, or
/// its owner if it has no controller, to gain that much life (in addition to any other results
/// that damage causes)." The gain is part of the damage event, not a trigger, so it follows the
/// damage at once.
///
/// Per [CR 702.15d], "The lifelink rules function no matter what zone an object with lifelink deals
/// damage from." A permanent's keywords are its layer results; anywhere else, those of the face
/// it has there. A source that has left its zone since is a new object (CR 400.7) whose keywords
/// are not kept, so it gives none.
///
/// [CR 702.15b]: https://yawgatog.com/resources/magic-rules/#R70215b
/// [CR 702.15d]: https://yawgatog.com/resources/magic-rules/#R70215d
#[inline]
fn lifelink<L: Layout>(game: &Game<'_, L>, source: ObjectRef<L>) -> Option<Seat> {
    let s = game.state();
    if !s.is_current(source) {
        return None;
    }
    let keywords = if game.on_battlefield(source) {
        s.derived(source.slot).keywords
    } else {
        s.copiable(source.slot).face(game.table()).keywords
    };
    keywords
        .contains(Keywords::LIFELINK)
        .then(|| s.controller(source.slot))
}

/// [`Effect::DealDamage`](beryl_card::Effect::DealDamage): "This deals [amount] damage to
/// [recipients]": noncombat damage from the source of what is resolving
/// ([CR 120.2b](https://yawgatog.com/resources/magic-rules/#R1202b)), to each of the players
/// in turn order or each of the permanents in the order they entered the battlefield. The
/// outcome is the permanents dealt damage, and how much was dealt in all.
///
/// # Panics
///
/// For an emblem's ability, which has no source.
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    to: Recipients,
    amount: Amount,
) -> Result<Outcome<L>, Full> {
    let amount = run.scene().amount(amount);
    let source = run.resolution().source();
    let (mut damaged, mut dealt) = (Bitboard::EMPTY, 0);
    match to {
        Recipients::Players(players) => {
            for player in run.in_turn_order(players) {
                deal(run.game(), source, Recipient::Player(player), amount, false)?;
                dealt += u32::from(amount);
            }
        }
        Recipients::Permanents(objects) => {
            damaged = run.permanents(objects);
            let mut left = damaged;
            while let Some(permanent) = run.next_permanent(&mut left) {
                let to = Recipient::Permanent(permanent);
                deal(run.game(), source, to, amount, false)?;
                dealt += u32::from(amount);
            }
        }
    }
    Ok(Outcome::of(damaged, dealt))
}
