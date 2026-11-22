//! The order each hand lists its cards in, kept across views as the web interface's
//! `Table.order` keeps it (`ui/src/stage/table.ts`).
//!
//! A hand keeps no order in the rules, and the session lists a view's objects by slot. A card
//! drawn from a library often has a lower slot than the cards already in the hand, so a hand
//! fanned in slot order would put a newly drawn card at its left end. On the table, as in
//! Arena (`vPdoSA5H5xU 17:47.3`), a card joins the right end of its hand. So the show
//! rearranges the hand cards of each view it is given into their order of arrival before it
//! plays or places anything: the cards that were in a hand when the show started, in slot
//! order, then each card that came to a hand later, the latest last. Every placement of the
//! view, at rest or while a script plays over it (`table::arrival`), then reads that order.

use beryl_protocol::prompt::{PlaceName, ShapeBody};
use beryl_protocol::protocol::{ObjectId, ObjectView, PlayerView, Status, ZoneName};
use beryl_protocol::script::Act;

use kit_frame::frame::OBJECTS;

/// Each card in a hand, by slot, and its turn among the cards that came to a hand. The lists
/// it works in are kept between views, so that arranging a view allocates nothing.
#[derive(Debug)]
pub(super) struct HandOrder {
    /// The turn of the card in each slot, or 0 for a slot whose card is in no hand.
    turns: Vec<u32>,
    /// The turn the last card to arrive took.
    last: u32,
    /// The turns of the view being arranged, which become `turns`.
    next: Vec<u32>,
    /// The indexes of the hand cards in the view's objects, in the order the view lists them.
    in_hand: Vec<usize>,
    /// The hand cards that arrive in the view being arranged.
    arriving: Vec<ObjectId>,
    /// The slots the view's script moves into a hand, in the order of the steps that first do.
    moved: Vec<Moved>,
}

impl HandOrder {
    /// Exchanges the arrival positions of two cards still in the hand.
    pub(super) fn trade(&mut self, first: usize, second: usize) {
        self.turns.swap(first, second);
    }

    /// An order that knows no hand yet, its lists made room for as many objects as a table
    /// holds (`OBJECTS`), so that arranging a view allocates nothing.
    pub(super) fn new() -> Self {
        Self {
            turns: Vec::with_capacity(OBJECTS),
            last: 0,
            next: Vec::with_capacity(OBJECTS),
            in_hand: Vec::with_capacity(OBJECTS),
            arriving: Vec::with_capacity(OBJECTS),
            moved: Vec::with_capacity(OBJECTS),
        }
    }

    /// Lists the hand cards of `view` in their order of arrival, and keeps that order for the
    /// next view. A card arrives now when it is in a hand now and either was in none in the
    /// view before or `view`'s script moves it into a hand from another zone, as a mulligan
    /// shuffles a hand away and deals a new one that may hold some of the same cards. The web
    /// interface's `arrived` (`ui/src/stage/table.ts`) likewise takes a card out of the order
    /// when it leaves a hand and puts it last when it comes back. The cards that arrive in one
    /// view come last, in the order its script moves them into a hand, and those it does not
    /// move in slot order. Only the hand cards change places in `view.objects`, among the
    /// indexes they held. A manual table's view keeps its own order, which the players set.
    pub(super) fn arrange(&mut self, view: &mut PlayerView) {
        if view.table.is_some() {
            self.turns.clear();
            return;
        }
        self.in_hand.clear();
        self.in_hand.extend(
            (view.objects.iter().enumerate())
                .filter(|(_, object)| object.zone == ZoneName::Hand)
                .map(|(index, _)| index),
        );
        let end = (self.in_hand.iter())
            .map(|&index| usize::from(view.objects[index].slot.0) + 1)
            .max()
            .unwrap_or(0);
        self.next.clear();
        self.next.resize(end, 0);
        self.arriving.clear();
        moved_into_hand(view, &mut self.moved);
        for &index in &self.in_hand {
            let object = &view.objects[index];
            let slot = usize::from(object.slot.0);
            match self.turns.get(slot).copied() {
                Some(turn)
                    if turn > 0
                        && !self
                            .moved
                            .iter()
                            .any(|m| m.entered && m.slot == object.slot) =>
                {
                    self.next[slot] = turn;
                }
                _ => self.arriving.push(object.slot),
            }
        }
        if !self.arriving.is_empty() {
            let moved = &self.moved;
            self.arriving.sort_by_key(|slot| {
                let step = moved.iter().position(|moved| moved.slot == *slot);
                (step.unwrap_or(usize::MAX), *slot)
            });
            for &slot in &self.arriving {
                self.last += 1;
                self.next[usize::from(slot.0)] = self.last;
            }
        }
        std::mem::swap(&mut self.turns, &mut self.next);
        // A selection sort of the hand cards among their own indexes, by swapping: a hand
        // holds a few cards, and this moves no record out of the list.
        let turn =
            |objects: &[ObjectView], index: usize| self.turns[usize::from(objects[index].slot.0)];
        for (k, &at) in self.in_hand.iter().enumerate() {
            let least = (self.in_hand[k..].iter().copied())
                .min_by_key(|&index| turn(&view.objects, index))
                .unwrap_or(at);
            if least != at {
                view.objects.swap(at, least);
            }
        }
        put_back_in_hand_order(view);
    }
}

/// A slot that a view's script moves into a hand.
#[derive(Debug)]
struct Moved {
    slot: ObjectId,
    /// Whether a step of the script brings it into a hand from another zone.
    entered: bool,
}

/// Fills `moved` with the slots `view`'s script moves into a hand, in the order of the steps
/// that first do, each marked when some step brings it from another zone.
fn moved_into_hand(view: &PlayerView, moved: &mut Vec<Moved>) {
    moved.clear();
    let steps = (view.script.iter())
        .flat_map(|script| &script.beats)
        .flatten();
    for step in steps {
        let Some(object) = step.act.object() else {
            continue;
        };
        if object.zone != ZoneName::Hand {
            continue;
        }
        let entered = match &step.act {
            Act::Fly { from, .. } | Act::Appear { from, .. } | Act::Vanish { from, .. } => {
                *from != ZoneName::Hand
            }
            _ => false,
        };
        match moved.iter_mut().find(|m| m.slot == object.slot) {
            Some(found) => found.entered |= entered,
            None => moved.push(Moved {
                slot: object.slot,
                entered,
            }),
        }
    }
}

/// Lists the hand's pile of an open put-back (a split whose first pile goes on top of the
/// library and whose second stays in the hand) in the order `view` lists the hand, as the
/// web's `inHandOrder` does. The session lists that pile in slot order, so its cards would
/// trade places as they rise from the hand into the prompt's tray.
fn put_back_in_hand_order(view: &mut PlayerView) {
    let Status::Deciding { decision } = &mut view.status else {
        return;
    };
    let ShapeBody::Split { piles } = &mut decision.prompt.body else {
        return;
    };
    let [onto, hand] = &mut piles[..] else {
        return;
    };
    if onto.place != Some(PlaceName::OnTop) || hand.place.is_some() {
        return;
    }
    let place = |slot: &ObjectId| {
        (view.objects.iter())
            .filter(|object| object.zone == ZoneName::Hand)
            .position(|object| object.slot == *slot)
            .unwrap_or(usize::MAX)
    };
    hand.cards.sort_by_key(place);
}
