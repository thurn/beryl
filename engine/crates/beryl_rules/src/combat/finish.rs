//! The end of the attack declaration: the attackers tap, any cost to attack is paid, and the
//! attack is announced for the triggers that see it.
//!
//! Implements: none. The combat module declares the rules of declaring attackers.

use super::{attack_cost, has};
use crate::bitboard::Bitboard;
use crate::card::Keywords;
use crate::fixed::Full;
use crate::layout::Layout;
use crate::replacement::event::{self, Sink};
use crate::state::State;
use crate::table::GameTable;

/// Finishes the attack declaration: the attackers tap, any cost to attack is paid, then attack
/// triggers trigger.
///
/// [CR 508.1f](https://yawgatog.com/resources/magic-rules/#R5081f): "The active player taps the
/// chosen creatures. Tapping a creature when it's declared as an attacker isn't a cost; attacking
/// simply causes creatures to become tapped."
///
/// [CR 508.1g](https://yawgatog.com/resources/magic-rules/#R5081g): "If there are any optional
/// costs to attack with the chosen creatures (expressed as costs a player may pay "as" a creature
/// attacks), the active player chooses which, if any, they will pay."
///
/// [CR 508.1h](https://yawgatog.com/resources/magic-rules/#R5081h): "If any of the chosen
/// creatures require paying costs to attack, or if any optional costs to attack were chosen, the
/// active player determines the total cost to attack." The taps come first, so an attacker
/// that taps can't also tap for mana to pay ([`super::attack_cost`]).
///
/// The taps and the attack are kept as one group: the one declaration ([CR 508.1]: "First, the
/// active player declares attackers. This turn-based action doesn't use the stack."). A cost
/// to attack that asks how to pay it splits the group at the question.
///
/// [CR 508.1]: https://yawgatog.com/resources/magic-rules/#R5081
pub(super) fn finish_attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    s.settle_question();
    sink.together();
    let declared = tap_and_declare(s, t, sink);
    sink.apart();
    declared
}

/// [`finish_attack`], inside the group it opens.
#[inline]
fn tap_and_declare<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<(), Full> {
    let attackers = attack_cost::attacking(s);
    tap_attackers(s, t, sink, attackers)?;
    if attack_cost::possible(t) {
        return attack_cost::pay_then_announce(s, t, sink);
    }
    announce_attack(s, t, sink, attackers)
}

/// Taps the attackers without vigilance, in battlefield order.
#[inline]
fn tap_attackers<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    attackers: Bitboard<L::Slot>,
) -> Result<(), Full> {
    // In battlefield order, never slot order: the taps' events are seen in this order, and a
    // slot number says nothing about a card (architecture §13). Tapping moves nothing. A
    // sole attacker has no order, and needs no walk of the battlefield.
    let tap = |s: &mut State<L>, sink: &mut Sink<L>, slot| {
        if has(s, slot, Keywords::VIGILANCE) {
            return Ok(());
        }
        let Some(permanent) = s.permanent_at(slot) else {
            return Ok(());
        };
        event::set_tapped(s, t, sink, permanent, true)
    };
    let mut untapped = attackers;
    if attackers.count() > 1 {
        for i in 0..s.battlefield().len() {
            let slot = s.battlefield()[i];
            if untapped.contains(slot) {
                untapped.remove(slot);
                tap(s, sink, slot)?;
                if untapped.is_empty() {
                    break;
                }
            }
        }
    }
    if let Some(slot) = untapped.sole() {
        tap(s, sink, slot)?;
    }
    Ok(())
}

/// The attack itself, once the attackers have tapped and any cost is paid: the event attack
/// triggers see.
///
/// [CR 508.3a](https://yawgatog.com/resources/magic-rules/#R5083a): "An ability that reads
/// "Whenever [a creature] attacks, . . ." triggers if that creature is declared as an attacker. ...
/// Such abilities won't trigger if a creature is put onto the battlefield attacking." The event
/// names only the declared attackers (see [CR 508.4]): creatures put onto the battlefield attacking,
/// "for the purposes of trigger events and effects, ... never 'attacked.'"
///
/// [CR 508.4]: https://yawgatog.com/resources/magic-rules/#R5084
#[inline]
pub(super) fn announce_attack<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    attackers: Bitboard<L::Slot>,
) -> Result<(), Full> {
    if attackers.is_empty() {
        return Ok(());
    }
    let declared = s.declare_attack(s.permanents_in(attackers));
    event::settle(s, t, sink, declared)
}
