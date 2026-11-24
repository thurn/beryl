//! Playing a land: the special action and the questions a land asks as it enters.
//!
//! Implements: CR 116.3, CR 305.1

use crate::bitboard::Bitboard;
use crate::engine::land_face;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::replacement::event::{self, Entry, Reason, Sink};
use crate::state::{Pending, State, Zone};
use crate::table::GameTable;

/// Plays a land: a special action that puts it onto the battlefield. A land that enters
/// tapped unless its controller pays life asks them first, and one that chooses a value as it
/// enters asks them for it.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "Playing a land is a
/// special action; it doesn't use the stack (see rule 116). Rather, the player simply puts the
/// land onto the battlefield."
///
/// [CR 116.3](https://yawgatog.com/resources/magic-rules/#R1163): "If a player takes a special
/// action, that player receives priority afterward."
///
/// See [CR 119.4](https://yawgatog.com/resources/magic-rules/#R1194): "If a cost or effect allows
/// a player to pay an amount of life greater than 0, the player may do so only if their life
/// total is greater than or equal to the amount of the payment."
pub(crate) fn play_land<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    slot: L::Slot,
) -> Result<(), Full> {
    s.count_land_played(player);
    s.give_priority(player);
    let face = land_face(s, t, slot).expect("a land face");
    let entry = Entry {
        face,
        ..Entry::default()
    };
    match crate::replacement::life_to_enter(s, t, slot, entry)? {
        Some(life) if crate::game::life::can_pay(s, t, player, life) => {
            s.ask(Pending::EnterPayingLife { slot, life });
            Ok(())
        }
        _ => enter_or_choose(s, t, sink, slot, false),
    }
}

/// Puts the land being played onto the battlefield, or first asks for the value it chooses as
/// it enters ([`Pending::EnterChoosing`]).
pub(crate) fn enter_or_choose<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    pay_life: bool,
) -> Result<(), Full> {
    let entry = Entry {
        face: land_face(s, t, slot).expect("a land face"),
        ..Entry::default()
    };
    match crate::replacement::chosen_to_enter(s, t, slot, entry)? {
        Some(kind) => {
            s.ask(Pending::EnterChoosing {
                slot,
                kind,
                pay_life,
            });
            Ok(())
        }
        None => enter_from_hand(s, t, sink, slot, pay_life),
    }
}

/// Moves the land being played from its owner's hand onto the battlefield.
pub(crate) fn enter_from_hand<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    pay_life: bool,
) -> Result<(), Full> {
    event::move_batch(
        s,
        t,
        sink,
        Bitboard::single(slot),
        Zone::Battlefield,
        Reason::Play,
        Entry {
            pay_life,
            face: land_face(s, t, slot).expect("a land face"),
            ..Entry::default()
        },
    )
}
