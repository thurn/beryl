//! The end of a script: what the view's table has that the events did not tell. A view's
//! derived state changes with no event (summoning sickness, power and toughness, a blocker's
//! part in combat, a mana pool), and a cast taken back leaves none. Each difference left
//! between the working table and the view's becomes a step here, so that the script replays
//! to the view exactly (plan K24). The stack's entries are told apart by the engine's names
//! for them (`hear.rs`), which both tables carry.

use super::build::Builder;
use super::moves::{COUNTERS, counted, set_counters};
use super::table::Table;
use super::{Act, Fact, Subject, Tally};
use crate::protocol::{ObjectView, Side, StackItem, Why, ZoneName};

impl Builder<'_> {
    /// Closes the script: one beat for what changed on the table and in its numbers, then one
    /// for the stack.
    pub(super) fn settle(&mut self) {
        self.open = false;
        let after = self.after;
        let gone: Vec<(u16, ZoneName)> = self
            .table
            .objects
            .values()
            .filter(|o| !after.objects.contains_key(&o.slot))
            .map(|o| (o.slot, o.zone))
            .collect();
        for (slot, from) in gone {
            self.emit(
                Act::Vanish {
                    subject: Subject::Object { slot },
                    from,
                    why: Why::StateBased,
                    object: None,
                    at: None,
                },
                None,
            );
        }
        // The stack's own order places what is bound for it. A mulligan's hand has gone back
        // before the next is dealt, a beat of its own: every card of it, those whose slots the
        // shuffled library deals out again too, which it has given other cards. A hand dealt
        // again is as large as it was; the cards a kept hand puts on the bottom leave it smaller.
        let hand = |table: &Table, side: Side| {
            table
                .objects
                .values()
                .filter(|o| o.owner == side && o.zone == ZoneName::Hand)
                .count()
        };
        let shuffled: Vec<Side> = after
            .objects
            .values()
            .filter(|o| self.mulligan(o) && hand(after, o.owner) >= hand(&self.table, o.owner))
            .map(|o| o.owner)
            .collect();
        let redealt: Vec<u16> = after
            .objects
            .values()
            .filter(|o| {
                shuffled.contains(&o.owner)
                    && o.zone == ZoneName::Hand
                    && self
                        .table
                        .objects
                        .get(&o.slot)
                        .is_some_and(|w| w.zone == ZoneName::Hand)
            })
            .map(|o| o.slot)
            .collect();
        let (back, rest): (Vec<&ObjectView>, Vec<&ObjectView>) = after
            .objects
            .values()
            .filter(|o| o.zone != ZoneName::Stack)
            .partition(|o| self.mulligan(o) || redealt.contains(&o.slot));
        for &next in &back {
            let card = if next.zone == ZoneName::Library {
                next.clone()
            } else {
                unknown_in_library(next)
            };
            self.arrive(Some(ZoneName::Hand), &card, None);
        }
        self.open &= back.is_empty();
        let dealt_again = back.into_iter().filter(|o| redealt.contains(&o.slot));
        for next in rest.into_iter().chain(dealt_again) {
            match self.table.objects.get(&next.slot).map(|o| o.zone) {
                Some(zone) if zone == next.zone => self.record(next),
                from => self.arrive(from, next, None),
            }
        }
        self.orders();
        self.numbers();
        self.open = false;
        self.stack();
    }

    /// `next` comes to its zone from `from`, or from nowhere, with no event to say why.
    fn arrive(&mut self, from: Option<ZoneName>, next: &ObjectView, item: Option<StackItem>) {
        let at = self.table.order(next.zone, next.owner).and_then(|now| {
            let end = self.after.order(next.zone, next.owner)?;
            let at = end.iter().position(|&s| s == next.slot)?;
            let others = now.iter().filter(|&&s| s != next.slot).count();
            (at < others).then(|| u16::try_from(at).ok()).flatten()
        });
        // An opening hand is dealt, and a mulligan's goes back, with no event before the
        // first turn: their cards do not travel, as none a view's events move before the game
        // does (`infer::show`).
        let dealt = self.table.turn.number == 0
            && (self.mulligan(next)
                || (from == Some(ZoneName::Library) && next.zone == ZoneName::Hand));
        let (from, why) = match from {
            None => (ZoneName::Nowhere, Why::Created),
            Some(from) if dealt && next.zone == ZoneName::Hand => (from, Why::Draw),
            // A cast taken back: the card is in its hand again.
            Some(from) if next.zone == ZoneName::Hand => (from, Why::Return),
            Some(from) => (from, Why::Effect),
        };
        self.travel(from, why, next.clone(), at, item, dealt, None);
    }

    /// Whether `next` is a card of a hand going back to its library before the first turn: a
    /// mulligan's.
    fn mulligan(&self, next: &ObjectView) -> bool {
        self.table.turn.number == 0
            && next.zone == ZoneName::Library
            && self
                .table
                .objects
                .get(&next.slot)
                .is_some_and(|o| o.zone == ZoneName::Hand)
    }

    /// The object's record is the view's, by the steps that show what changed of it: a turn,
    /// a face, a side or a host, counters, power and toughness, and the rest with no motion.
    fn record(&mut self, next: &ObjectView) {
        let Some(mut now) = self.table.objects.get(&next.slot).cloned() else {
            return;
        };
        if now == *next {
            return;
        }
        if now.tapped != next.tapped {
            now.tapped = next.tapped;
            now.combat.clone_from(&next.combat);
            self.emit(
                Act::Tap {
                    object: now.clone(),
                },
                None,
            );
        }
        if now.facing != next.facing || now.name.is_some() != next.name.is_some() {
            now.facing = next.facing;
            now.name.clone_from(&next.name);
            now.types.clone_from(&next.types);
            now.keywords.clone_from(&next.keywords);
            now.pt = next.pt;
            self.emit(
                Act::Flip {
                    object: now.clone(),
                },
                None,
            );
        }
        if now.controller != next.controller || now.attached_to != next.attached_to {
            now.controller = next.controller;
            now.attached_to = next.attached_to;
            self.emit(
                Act::Fly {
                    from: now.zone,
                    why: Why::Effect,
                    object: now.clone(),
                    at: None,
                    item: None,
                },
                None,
            );
        }
        for counter in COUNTERS {
            let count = counted(&next.counters, counter);
            if counted(&now.counters, counter) != count {
                set_counters(&mut now.counters, counter, count);
                // A counter's badge and the stats it changes show together.
                now.pt = next.pt;
                let object = now.clone();
                let tally = Tally::Counters { counter, object };
                self.emit(Act::Count { tally }, None);
            }
        }
        if now.pt != next.pt {
            now.pt = next.pt;
            let object = now.clone();
            let tally = Tally::Stats { object };
            self.emit(Act::Count { tally }, None);
        }
        if now == *next {
            return;
        }
        // What is left shows no motion: the latest step with the object's record carries it,
        // or a step of its own does.
        let step = self
            .touched
            .get(&next.slot)
            .and_then(|&(beat, step)| self.beats[beat][step].act.object_mut());
        if let Some(object) = step {
            object.clone_from(next);
            self.table.objects.insert(next.slot, next.clone());
        } else {
            let object = next.clone();
            let fact = Fact::Object { object };
            self.emit(Act::Set { fact }, None);
        }
    }

    /// Each pile and the battlefield in the view's order.
    fn orders(&mut self) {
        let after = self.after;
        for (zone, owner) in [
            (ZoneName::Battlefield, Side::You),
            (ZoneName::Graveyard, Side::You),
            (ZoneName::Graveyard, Side::Opponent),
            (ZoneName::Library, Side::You),
            (ZoneName::Library, Side::Opponent),
        ] {
            let (Some(now), Some(end)) = (self.table.order(zone, owner), after.order(zone, owner))
            else {
                continue;
            };
            if now == end {
                continue;
            }
            if zone == ZoneName::Library {
                let (player, order) = (owner, end.clone());
                let fact = Fact::Library { player, order };
                self.emit(Act::Set { fact }, None);
                continue;
            }
            for (at, slot) in end.iter().enumerate() {
                let now = self.table.order(zone, owner).and_then(|now| now.get(at));
                if now != Some(slot)
                    && let Some(object) = after.objects.get(slot)
                    && let Ok(at) = u16::try_from(at)
                {
                    self.emit(
                        Act::Fly {
                            from: zone,
                            why: Why::Effect,
                            object: object.clone(),
                            at: Some(at),
                            item: None,
                        },
                        None,
                    );
                }
            }
        }
    }

    /// Life totals, mana pools and the turn.
    fn numbers(&mut self) {
        let after = self.after;
        for player in [Side::You, Side::Opponent] {
            let (now, end) = (self.table.piles(player), after.piles(player));
            let (life, mana) = (end.life, end.mana);
            let (counts_life, counts_mana) = (now.life != life, now.mana != mana);
            if counts_life {
                let tally = Tally::Life { player, life };
                self.emit(Act::Count { tally }, None);
            }
            if counts_mana {
                let tally = Tally::Pool { player, mana };
                self.emit(Act::Count { tally }, None);
            }
        }
        if self.table.turn != after.turn {
            let fact = Fact::Turn {
                number: after.turn.number,
                active: after.turn.active,
                step: after.turn.step,
            };
            self.emit(Act::Set { fact }, None);
        }
    }

    /// The stack is the view's, entry by entry from the bottom: an entry the table has higher
    /// up comes down to its place as the abilities under it leave, and one it lacks goes on:
    /// an ability the views held back while its cost was paid, shown now.
    fn stack(&mut self) {
        let after = self.after;
        for (at, entry) in after.stack.iter().enumerate() {
            let subject = entry.subject();
            let stack = &self.table.stack;
            let found = (at..stack.len()).find(|&k| stack[k].subject() == subject);
            self.clear(at, found.unwrap_or(stack.len()));
            let item = entry.item.clone();
            match (self.table.stack.get(at), &entry.item, entry.id) {
                (Some(now), _, _) if now.subject() == subject => {
                    if now.item != item {
                        let fact = Fact::Item { subject, item };
                        self.emit(Act::Set { fact }, None);
                    }
                }
                (_, StackItem::Ability { .. }, Some(id)) => {
                    let cause = self.lifter(id, &item);
                    self.lift(id, item, cause);
                }
                (_, StackItem::Ability { .. }, None) => {}
                (_, &StackItem::Spell { slot, .. }, _) => {
                    if let Some(next) = after.objects.get(&slot) {
                        let from = self.table.objects.get(&slot).map(|o| o.zone);
                        self.arrive(from, next, Some(item));
                    }
                }
            }
        }
        self.clear(after.stack.len(), self.table.stack.len());
        // A spell's card as the view has it, once it is on the stack.
        for next in after.objects.values().filter(|o| o.zone == ZoneName::Stack) {
            self.record(next);
        }
    }

    /// The abilities at `from` up to `to` on the stack leave it, the top one first: with no
    /// event that one finished, it was countered, or its activation taken back.
    fn clear(&mut self, from: usize, to: usize) {
        let ids: Vec<u32> = self.table.stack[from.min(to)..to]
            .iter()
            .filter_map(|e| e.id)
            .collect();
        for id in ids.into_iter().rev() {
            self.emit(
                Act::Vanish {
                    subject: Subject::Ability { id },
                    from: ZoneName::Stack,
                    why: Why::Effect,
                    object: None,
                    at: None,
                },
                None,
            );
        }
    }
}

/// The card in `object`'s slot as a library holds it: unknown, with none of what a hand or
/// the battlefield shows of it.
fn unknown_in_library(object: &ObjectView) -> ObjectView {
    ObjectView {
        zone: ZoneName::Library,
        controller: object.owner,
        name: None,
        types: Vec::new(),
        keywords: Vec::new(),
        pt: None,
        counters: Vec::new(),
        ..object.clone()
    }
}
