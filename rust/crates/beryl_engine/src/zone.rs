//! The zone-move chokepoints: every change of zone, every draw and every shuffle goes through
//! here, so identities, knowledge and zone membership stay in agreement (architecture §4.3,
//! §5, §13).

use crate::bitboard::Bitboard;
use crate::fixed::{Fixed, Full};
use crate::state::{Counters, DECK_SLOTS, NONE, ObjectFlags, State, Zone};

/// Where in a library an object goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    Top,
    Bottom,
}

/// Moves an object to the top of a zone that has an order.
///
/// [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves from
/// one zone to another becomes a new object with no memory of, or relation to, its previous
/// existence." The slot's incarnation is bumped and everything but its identity and owner is
/// reset.
///
/// [CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." Zones are kept by owner, so that holds by construction.
pub(crate) fn move_to(s: &mut State, slot: u8, to: Zone) -> Result<(), Full> {
    move_to_place(s, slot, to, Place::Top)
}

/// Moves an object to the bottom of its owner's library.
pub(crate) fn put_on_bottom(s: &mut State, slot: u8) -> Result<(), Full> {
    move_to_place(s, slot, Zone::Library, Place::Bottom)
}

fn move_to_place(s: &mut State, slot: u8, to: Zone, place: Place) -> Result<(), Full> {
    leave(s, slot);
    let timestamp = s.zones.next_timestamp;
    let o = s.object_mut(slot);
    o.incarnation = o.incarnation.wrapping_add(1);
    o.zone = to;
    o.controller = o.owner;
    o.flags = ObjectFlags::default();
    o.damage = 0;
    o.counters = Counters::default();
    o.attached_to = NONE;
    o.linked = NONE;
    o.copy_of = NONE;
    o.chosen = NONE;
    o.face = 0;
    let owner = usize::from(o.owner);
    match to {
        Zone::Library => match place {
            Place::Top => s.zones.library[owner].push(slot)?,
            Place::Bottom => s.zones.library[owner].insert(0, slot)?,
        },
        Zone::Graveyard => s.zones.graveyard[owner].push(slot)?,
        Zone::Battlefield => {
            s.battlefield.push(slot)?;
            // CR 302.6: a permanent that just arrived hasn't been under its controller's
            // control since their most recent turn began.
            let o = s.object_mut(slot);
            o.timestamp = timestamp;
            o.flags.set(ObjectFlags::SUMMONING_SICK, true);
            s.zones.next_timestamp = timestamp.wrapping_add(1);
            s.flags.derived_dirty = true;
        }
        Zone::Stack => s.zones.stack.insert(slot),
        Zone::Hand | Zone::Exile | Zone::Nowhere => {}
    }
    if (to as usize) < Zone::OWNED {
        s.zones.members[owner][to as usize].insert(slot);
    }
    // CR 400.2: cards in public zones are seen by every player.
    if to.is_public() {
        for k in &mut s.knowledge {
            k.insert(slot);
        }
    }
    // [CR 402.3](https://yawgatog.com/resources/magic-rules/#R4023): "A player may arrange their hand in any convenient fashion and look at it
    // as much as they want." Whatever put the card there: a draw, or a return from a library.
    if to == Zone::Hand {
        s.knowledge[owner].insert(slot);
    }
    Ok(())
}

fn leave(s: &mut State, slot: u8) {
    let o = *s.object(slot);
    let owner = usize::from(o.owner);
    match o.zone {
        Zone::Library => remove_slot(&mut s.zones.library[owner], slot),
        Zone::Graveyard => remove_slot(&mut s.zones.graveyard[owner], slot),
        Zone::Battlefield => {
            if let Some(i) = s.battlefield.position(slot) {
                s.battlefield.remove(i);
            }
            s.flags.derived_dirty = true;
        }
        Zone::Stack => s.zones.stack.remove(slot),
        Zone::Hand | Zone::Exile | Zone::Nowhere => {}
    }
    if (o.zone as usize) < Zone::OWNED {
        s.zones.members[owner][o.zone as usize].remove(slot);
    }
}

fn remove_slot<const N: usize>(list: &mut Fixed<u8, N>, slot: u8) {
    if let Some(i) = list.position(slot) {
        list.remove(i);
    }
}

/// Draws one card.
///
/// [CR 121.1](https://yawgatog.com/resources/magic-rules/#R1211): "A player draws a card by
/// putting the top card of their library into their hand."
///
/// [CR 121.4](https://yawgatog.com/resources/magic-rules/#R1214): "A player who attempts to
/// draw a card from a library with no cards in it loses the game the next time a player would
/// receive priority."
pub(crate) fn draw(s: &mut State, player: u8) -> Result<(), Full> {
    let Some(&slot) = s.zones.library[usize::from(player)].as_slice().last() else {
        s.player_mut(player).drew_from_empty = true;
        return Ok(());
    };
    move_to(s, slot, Zone::Hand)
}

/// Draws `n` cards, one at a time ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)):
/// "If a player is instructed to draw multiple cards, that player performs that many
/// individual card draws."
pub(crate) fn draw_n(s: &mut State, player: u8, n: u8) -> Result<(), Full> {
    for _ in 0..n {
        draw(s, player)?;
    }
    Ok(())
}

/// Shuffles a library.
///
/// [CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a): "To shuffle a library
/// or a face-down pile of cards, randomize the cards within it so that no player knows their
/// order."
///
/// Slots are public handles (architecture §13), so randomizing the order of the slots alone
/// would let a player follow a card they know through the shuffle. The identities are dealt
/// anew among the shuffled slots and every player forgets them; then the slots are
/// reordered. [CR 701.20d](https://yawgatog.com/resources/magic-rules/#R70120d): "If cards in
/// a player's library are shuffled or otherwise reordered, any revealed cards that are
/// reordered stop being revealed and become new objects."
pub(crate) fn shuffle_library(s: &mut State, player: u8) {
    let p = usize::from(player);
    let library = s.zones.library[p];
    let mut cards: Fixed<u8, DECK_SLOTS> = Fixed::new();
    for &slot in &library {
        // A library never holds more than its owner's deck slots.
        let _ = cards.push(s.object(slot).card);
    }
    s.rng.shuffle.shuffle(cards.as_mut_slice());
    let shuffled: Bitboard = library.iter().copied().collect();
    for (&slot, &card) in library.iter().zip(cards.iter()) {
        let o = s.object_mut(slot);
        o.card = card;
        o.incarnation = o.incarnation.wrapping_add(1);
    }
    for k in &mut s.knowledge {
        *k &= !shuffled;
    }
    s.rng.shuffle.shuffle(s.zones.library[p].as_mut_slice());
}

/// Plays a land: a special action that puts it onto the battlefield.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "Playing a land is a
/// special action; it doesn't use the stack (see rule 116). Rather, the player simply puts the
/// land onto the battlefield."
///
/// [CR 116.3](https://yawgatog.com/resources/magic-rules/#R1163): "If a player takes a special
/// action, that player receives priority afterward."
pub(crate) fn play_land(s: &mut State, player: u8, slot: u8) -> Result<(), Full> {
    move_to(s, slot, Zone::Battlefield)?;
    s.player_mut(player).lands_played += 1;
    s.turn.priority = player;
    s.turn.passes = 0;
    Ok(())
}

/// Discards a card ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To
/// discard a card, move it from its owner's hand to that player's graveyard."
pub(crate) fn discard(s: &mut State, slot: u8) -> Result<(), Full> {
    move_to(s, slot, Zone::Graveyard)
}

/// The number of cards in a player's hand.
pub(crate) fn hand_size(s: &State, player: u8) -> u32 {
    s.zone(player, Zone::Hand).count()
}
