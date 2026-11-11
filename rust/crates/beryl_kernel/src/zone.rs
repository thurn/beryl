//! The zone-move chokepoints: every change of zone, every draw and every shuffle goes through
//! here, so identities, knowledge and zone membership stay in agreement (architecture §4.3,
//! §5, §13).
//!
//! Transitional: these are free functions, `pub` for `beryl_engine`'s event pipeline. They
//! become the zone mutators of the crate's contract with by-w8gv.5.3 and by-w8gv.5.4.

use crate::bitboard::Bitboard;
use crate::chance;
use crate::event::Sink;
use crate::fixed::{Fixed, Full};
use crate::ids::{CardIx, Seat, SlotId};
use crate::layout::{Layout, Queue};
use crate::rng::SplitMix64;
use crate::state::{Counters, DECK_SLOTS, Derived, Object, ObjectFlags, State, Zone};
use beryl_card::FaceIx;

/// Where in a library an object goes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Place {
    Top,
    Bottom,
    /// Under this many cards from the top, or at the bottom of a library with fewer: Oust's
    /// "second from the top" is `UnderTop(1)`.
    UnderTop(u8),
}

/// Moves an object to the top of a zone that has an order: the raw execution step of the
/// pipeline (architecture §8.2), which `event` wraps with replacements and triggers.
///
/// [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): "An object that moves from
/// one zone to another becomes a new object with no memory of, or relation to, its previous
/// existence." The slot's incarnation is bumped and everything but its identity and owner is
/// reset.
///
/// [CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003): "If an object would go to
/// any library, graveyard, or hand other than its owner's, it goes to its owner's
/// corresponding zone." Zones are kept by owner, so that holds by construction.
pub fn move_to<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    slot: L::Slot,
    to: Zone,
) -> Result<(), Full> {
    move_to_place(s, sink, slot, to, Place::Top)
}

/// Moves an object to the bottom of its owner's library.
pub fn put_on_bottom<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    slot: L::Slot,
) -> Result<(), Full> {
    move_to_place(s, sink, slot, Zone::Library, Place::Bottom)
}

pub fn move_to_place<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    slot: L::Slot,
    to: Zone,
    place: Place,
) -> Result<(), Full> {
    // A card about to be seen has its identity settled first, before anything reads it
    // ([`chance`]): CR 400.2 below has every player see a card in a public zone, and CR 402.3
    // its owner see one in their hand.
    if to.is_public() || to == Zone::Hand {
        chance::settle(s, sink, slot);
    }
    // [CR 708.9](https://yawgatog.com/resources/magic-rules/#R7089): "If a face-down permanent
    // ... moves from the battlefield to any other zone, its owner must reveal it to all players
    // as they move it." Whatever zone it goes to, a hidden one included.
    let o = *s.object(slot);
    if o.zone == Zone::Battlefield && o.face == FaceIx::DOWN {
        for k in &mut s.knowledge {
            k.insert(slot);
        }
    }
    let from = o.zone;
    leave(s, slot);
    let timestamp = s.zones.next_timestamp;
    let o = s.object_mut(slot);
    o.incarnation = o.incarnation.wrapping_add(1);
    o.zone = to;
    o.base_controller = o.owner;
    // A token stays a token wherever it goes, until it ceases to exist (CR 111.7).
    o.flags = ObjectFlags(o.flags.0 & (ObjectFlags::TOKEN | ObjectFlags::ONE_ONE));
    o.damage = 0;
    o.counters = Counters::default();
    o.attached_to = None;
    o.linked = None;
    o.face = FaceIx::FRONT;
    o.combat = None;
    let owner = o.owner.index();
    match to {
        Zone::Library => match place {
            Place::Top => s.zones.library[owner].push(slot)?,
            Place::Bottom => s.zones.library[owner].insert(0, slot)?,
            Place::UnderTop(n) => {
                let library = &mut s.zones.library[owner];
                library.insert(library.len().saturating_sub(usize::from(n)), slot)?;
            }
        },
        Zone::Graveyard => s.zones.graveyard[owner].push(slot)?,
        Zone::Battlefield => {
            s.enter_battlefield(slot)?;
            // CR 302.6: a permanent that just arrived hasn't been under its controller's
            // control since their most recent turn began.
            let o = s.object_mut(slot);
            o.timestamp = timestamp;
            o.flags.set(ObjectFlags::SUMMONING_SICK, true);
            o.flags.set(ObjectFlags::ENTERED_THIS_TURN, true);
            s.zones.next_timestamp = timestamp.wrapping_add(1);
            s.flags.derived_dirty = true;
        }
        Zone::Stack => s.zones.stack.insert(slot),
        Zone::Hand | Zone::Exile | Zone::Nowhere => {}
    }
    if (to as usize) < Zone::OWNED {
        s.zones.members[owner][to as usize].insert(slot);
    }
    if matches!(to, Zone::Battlefield | Zone::Nowhere) {
        forget_exiled_with(s, slot);
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
    if from == Zone::Hand && to == Zone::Library {
        forget_hand(s, Seat::BOTH[owner], slot);
    }
    Ok(())
}

/// A card put from its owner's hand into their library unrevealed, as Brainstorm puts two
/// back: the other player sees a card back go and cannot tell which of the hand's cards it
/// was, so they forget it and every card still in that hand, however they came to know them
/// (a look at the hand, a card they saw on top of the library drawn). A hand and a library are
/// hidden zones ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002): "Library and
/// hand are hidden zones, even if all the cards in one such zone happen to be revealed"), and
/// the owner keeps their hand in any order
/// ([CR 402.3](https://yawgatog.com/resources/magic-rules/#R4023): "A player may arrange their
/// hand in any convenient fashion").
fn forget_hand<L: Layout>(s: &mut State<L>, owner: Seat, moved: L::Slot) {
    let hand = s.zones.members[owner.index()][Zone::Hand as usize];
    for p in Seat::BOTH {
        if p != owner {
            let k = &mut s.knowledge[p.index()];
            k.remove(moved);
            *k &= !hand;
        }
    }
}

/// Unlinks the cards exiled with an earlier object in `slot`, as a new object arrives there
/// (or a token's slot is freed). The cards stay "exiled with" the object that exiled them
/// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
/// refers only to cards in the exile zone that were put there as a result of an instruction
/// to exile them in the first ability." An ability of that object still on the stack after it
/// left finds them; the new object is not that object
/// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) and does not.
fn forget_exiled_with<L: Layout>(s: &mut State<L>, slot: L::Slot) {
    let exile = s.zones.members[0][Zone::Exile as usize] | s.zones.members[1][Zone::Exile as usize];
    for x in exile.iter() {
        let o = s.object_mut(x);
        if o.flags.has(ObjectFlags::EXILED_WITH) && o.linked == Some(slot) {
            o.flags.set(ObjectFlags::EXILED_WITH, false);
            o.linked = None;
        }
    }
}

fn leave<L: Layout>(s: &mut State<L>, slot: L::Slot) {
    let o = *s.object(slot);
    let owner = o.owner.index();
    match o.zone {
        Zone::Library => remove_slot(&mut s.zones.library[owner], slot),
        Zone::Graveyard => remove_slot(&mut s.zones.graveyard[owner], slot),
        Zone::Battlefield => {
            s.leave_battlefield(slot);
            s.flags.derived_dirty = true;
            // [CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064): "A permanent is
            // removed from combat if it leaves the battlefield". The creatures attacking it or
            // blocked by it stay in combat, related to nothing; the slot may soon hold
            // another object. What was attached to it is attached to nothing: the object it
            // returns as is new ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)),
            // and an Aura left unattached goes to the graveyard as a state-based action
            // (CR 704.5m).
            for i in 0..s.battlefield.len() {
                let other = s.object_mut(s.battlefield.as_slice()[i]);
                if other.combat == Some(slot) {
                    other.combat = None;
                }
                if other.attached_to == Some(slot) {
                    other.attached_to = None;
                }
            }
        }
        Zone::Stack => s.zones.stack.remove(slot),
        Zone::Hand | Zone::Exile | Zone::Nowhere => {}
    }
    if (o.zone as usize) < Zone::OWNED {
        s.zones.members[owner][o.zone as usize].remove(slot);
    }
}

fn remove_slot<S: SlotId>(list: &mut impl Queue<S>, slot: S) {
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
pub fn draw<L: Layout>(s: &mut State<L>, sink: &mut Sink<L>, player: Seat) -> Result<(), Full> {
    let Some(&slot) = s.zones.library[player.index()].as_slice().last() else {
        s.player_mut(player).drew_from_empty = true;
        return Ok(());
    };
    move_to(s, sink, slot, Zone::Hand)
}

/// Draws `n` cards, one at a time ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)):
/// "If a player is instructed to draw multiple cards, that player performs that many
/// individual card draws."
pub fn draw_n<L: Layout>(
    s: &mut State<L>,
    sink: &mut Sink<L>,
    player: Seat,
    n: u8,
) -> Result<(), Full> {
    for _ in 0..n {
        draw(s, sink, player)?;
    }
    Ok(())
}

/// Moves a card within its owner's library to its top or bottom. Not a zone change, so the
/// card stays the same object and whoever knew it still does: a player who scries or looks
/// knows where they put each card.
pub fn reorder_library<L: Layout>(
    s: &mut State<L>,
    slot: L::Slot,
    place: Place,
) -> Result<(), Full> {
    let library = &mut s.zones.library[s.objects[slot.index()].owner.index()];
    let i = library
        .position(slot)
        .expect("a card in its owner's library");
    library.remove(i);
    match place {
        Place::Top => library.push(slot),
        Place::Bottom => library.insert(0, slot),
        Place::UnderTop(n) => library.insert(library.len().saturating_sub(usize::from(n)), slot),
    }
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
pub fn shuffle_library<L: Layout>(s: &mut State<L>, player: Seat) {
    let p = player.index();
    let library = s.zones.library[p].clone();
    let mut rng = s.rng.shuffle;
    rebind(s, library.as_slice(), &mut rng);
    s.rng.shuffle = rng;
    s.rng.shuffle.shuffle(s.zones.library[p].as_mut_slice());
}

/// Deals the identities of the cards in `slots`, all in one library, anew among those slots
/// with `rng`, and every player forgets them: the slots are public handles (architecture
/// §13), so this is how cards come to be in an order no player knows. Each becomes a new
/// object ([CR 701.20d](https://yawgatog.com/resources/magic-rules/#R70120d)): "If cards in a
/// player's library are shuffled or otherwise reordered, any revealed cards that are
/// reordered stop being revealed and become new objects."
pub fn rebind<L: Layout>(s: &mut State<L>, slots: &[L::Slot], rng: &mut SplitMix64) {
    let mut cards: Fixed<CardIx, DECK_SLOTS> = Fixed::new();
    for &slot in slots {
        // A library never holds more than its owner's deck slots.
        let _ = cards.push(s.object(slot).card);
    }
    rng.shuffle(cards.as_mut_slice());
    for (&slot, &card) in slots.iter().zip(cards.iter()) {
        let o = s.object_mut(slot);
        o.card = card;
        o.incarnation = o.incarnation.wrapping_add(1);
    }
    let rebound: Bitboard<L::Slot> = slots.iter().copied().collect();
    for k in &mut s.knowledge {
        *k &= !rebound;
    }
}

/// A token in a zone other than the battlefield ceases to exist, and its slot returns to the
/// shared pool ([CR 704.5d](https://yawgatog.com/resources/magic-rules/#R7045d)): "If a token
/// is in a zone other than the battlefield, it ceases to exist." The incarnation is kept and
/// bumped, so references to the token stay stale after the slot is reused. The cards exiled
/// with it are exiled with nothing any more.
pub fn cease_to_exist<L: Layout>(s: &mut State<L>, slot: L::Slot) {
    leave(s, slot);
    let incarnation = s.object(slot).incarnation.wrapping_add(1);
    *s.object_mut(slot) = Object {
        incarnation,
        ..Object::EMPTY
    };
    forget_exiled_with(s, slot);
    for k in &mut s.knowledge {
        k.remove(slot);
    }
}

/// What makes two objects interchangeable (§7.2): every field but the incarnation, the
/// timestamp and the battlefield index, and for a permanent its characteristics after continuous effects, so two
/// untapped basic lands match but a stolen Mana Crypt does not match your own, nor a
/// creature that got -2/-0 one that didn't.
pub fn signature<L: Layout>(s: &State<L>, slot: L::Slot) -> (Object<L>, Option<Derived>) {
    let o = s.object(slot);
    let derived = (o.zone == Zone::Battlefield).then(|| *s.derived(slot));
    (
        Object {
            incarnation: 0,
            timestamp: 0,
            battlefield_index: 0,
            ..*o
        },
        derived,
    )
}

/// The number of cards in a player's hand.
pub fn hand_size<L: Layout>(s: &State<L>, player: Seat) -> u32 {
    s.zone(player, Zone::Hand).count()
}
