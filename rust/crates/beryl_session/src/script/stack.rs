//! The stack in a script (plan K23): the builder follows it by the engine's names. An ability
//! goes on the stack as the engine says it arrived and leaves as the engine says it left
//! ([`Move`]), a spell with its card; an event that announces an ability, pays for one or
//! is one's effect names it ([`Cause::entry`]), and nothing here tells two entries apart by
//! what they look like. What is staged apart from the engine's order is said where it is: an
//! ability a view held back, and a trigger lifting out of a permanent that is leaving.

use super::build::{Builder, activator};
use super::hear::{Cause, Move};
use super::staging::Told;
use super::{Act, Fact, Subject};
use crate::protocol::{GameEvent, StackItem, Why, ZoneName};

/// An ability the engine said arrived on the stack, which no view has shown: the opponent's,
/// while its cost is being paid (`hold_back_activation_being_paid`).
#[derive(Clone, Debug)]
pub(super) struct Held {
    id: u32,
    item: StackItem,
    cause: Option<Subject>,
}

impl Builder<'_> {
    /// Whether the table's stack holds `subject`.
    pub(super) fn stacked(&self, subject: Subject) -> bool {
        self.table.stack.iter().any(|e| e.subject() == subject)
    }

    /// Whether this script shows the ability `id`: the view's stack has it, or it leaves the
    /// stack before the view, having been there to be seen. One removed as it was being put
    /// there never was
    /// ([CR 603.3d](https://yawgatog.com/resources/magic-rules/#R6033d): "If a choice is
    /// required when the triggered ability goes on the stack but no legal choices can be made
    /// for it ... the ability is simply removed from the stack."), which is its controller's
    /// doing and no effect's.
    fn shows(&self, id: u32) -> bool {
        let subject = Subject::Ability { id };
        self.after.stack.iter().any(|e| e.subject() == subject)
            || self.moves.iter().any(|(_, m)| {
                matches!(*m, Move::Departure { id: left, cause }
                    if left == id && !matches!(cause, Cause::Player { .. }))
            })
    }

    /// An ability arrives on the stack or leaves it, as the engine said.
    pub(super) fn stack_move(&mut self, moved: &Move) {
        match *moved {
            Move::Arrival {
                id,
                ref item,
                cause,
            } => {
                let cause = cause.subject();
                if self.stacked(Subject::Ability { id }) {
                    return;
                }
                let item = item.clone();
                if self.shows(id) {
                    self.lift(id, item, cause);
                } else {
                    self.held.push(Held { id, item, cause });
                }
            }
            Move::Departure { id, cause } => {
                let subject = Subject::Ability { id };
                self.held.retain(|held| held.id != id);
                if !self.stacked(subject) {
                    return;
                }
                // It resolved, or something else took it off the stack: it was countered.
                let resolved = cause
                    == (Cause::Effect {
                        entry: Some(subject),
                    });
                let why = if resolved { Why::Resolve } else { Why::Effect };
                self.open = false;
                self.emit(
                    Act::Vanish {
                        subject,
                        from: ZoneName::Stack,
                        why,
                        object: None,
                        at: None,
                    },
                    cause.subject(),
                );
                self.open = false;
            }
        }
    }

    /// The ability `id` as this script first shows it: as an event of the script goes on to
    /// announce it, or as the view's stack has it, or else as `item`, as it arrived.
    fn announced(&self, id: u32, item: StackItem) -> StackItem {
        let subject = Some(Subject::Ability { id });
        let announced = self
            .events
            .iter()
            .zip(&self.causes)
            .find_map(|(e, cause)| match e {
                GameEvent::Ability { item, .. } if cause.entry() == subject => Some(item),
                _ => None,
            });
        let shown = self
            .after
            .stack
            .iter()
            .find(|e| Some(e.subject()) == subject)
            .map(|e| &e.item);
        announced.or(shown).cloned().unwrap_or(item)
    }

    /// Puts the ability `id` on the stack, in a beat of its own.
    pub(super) fn lift(&mut self, id: u32, item: StackItem, cause: Option<Subject>) {
        self.held.retain(|held| held.id != id);
        let item = self.announced(id, item);
        self.open = false;
        self.emit(Act::Emerge { id, item }, cause);
        self.open = false;
    }

    /// The cause an ability the view's stack has goes on the stack with, where no arrival of
    /// this script says: the one it was held with, or its activator.
    pub(super) fn lifter(&self, id: u32, item: &StackItem) -> Option<Subject> {
        let held = self.held.iter().find(|held| held.id == id);
        held.map_or_else(|| activator(item), |held| held.cause)
    }

    /// Puts on the stack, each in a beat of its own, the abilities the view's stack has up
    /// to its entry `through` that the table lacks: what goes on above one must find it
    /// there.
    pub(super) fn lift_through(&mut self, through: usize) {
        let after = self.after;
        for entry in after.stack.iter().take(through + 1) {
            if let Some(id) = entry.id
                && !self.stacked(entry.subject())
            {
                let arrives = self.moves.iter().find_map(|(_, m)| match *m {
                    Move::Arrival { id: i, cause, .. } if i == id => Some(cause.subject()),
                    _ => None,
                });
                let cause = arrives.unwrap_or_else(|| self.lifter(id, &entry.item));
                self.lift(id, entry.item.clone(), cause);
            }
        }
    }

    /// Before the permanents that leave in the beat event `i` begins: the triggered
    /// abilities that come out of them lift first, though the engine puts them on the stack
    /// afterwards
    /// ([CR 603.3](https://yawgatog.com/resources/magic-rules/#R6033): "the next time a
    /// player would receive priority"): an ability lifts out of a permanent that is leaving
    /// before that goes (Arena, `I3Te0m1bBFw 26:56.75`).
    pub(super) fn lift_ahead(&mut self, i: usize, told: &[Told]) {
        let after = self.after;
        let leaving: Vec<u16> = (i..told.len())
            .take_while(|&j| j == i || told[j].together)
            .filter_map(|j| match self.events[j] {
                GameEvent::ZoneChange {
                    slot,
                    from: ZoneName::Battlefield,
                    to,
                    ..
                } if to != ZoneName::Battlefield => Some(slot),
                _ => None,
            })
            .collect();
        let from_leaving = |item: &StackItem| {
            matches!(item, StackItem::Ability { source: Some(source), triggered: true, .. }
                if leaving.contains(source))
        };
        let through = (0..after.stack.len()).rev().find(|&at| {
            from_leaving(&after.stack[at].item) && !self.stacked(after.stack[at].subject())
        });
        if let Some(through) = through {
            self.lift_through(through);
        }
    }

    /// An ability an event of this script names, held back by the views so far: it goes on
    /// the stack before the event shows.
    pub(super) fn unhold(&mut self, cause: Cause) {
        if let Some(Subject::Ability { id }) = cause.entry()
            && self.shows(id)
            && let Some(at) = self.held.iter().position(|held| held.id == id)
        {
            let held = self.held.remove(at);
            self.lift(id, held.item, held.cause);
        }
    }

    /// An event says the ability `said` names was activated, or put on the stack with its
    /// targets: it is as the event has it, on the stack by now unless the views held it back.
    pub(super) fn announce(&mut self, item: &StackItem, said: Cause) {
        let Some(subject @ Subject::Ability { id }) = said.entry() else {
            return;
        };
        let cause = said.subject();
        match self.table.stack.iter().find(|e| e.subject() == subject) {
            Some(entry) if entry.item != *item => {
                let item = item.clone();
                let fact = Fact::Item { subject, item };
                self.emit(Act::Set { fact }, cause);
            }
            Some(_) => {}
            None => {
                let after = self.after;
                let at = after.stack.iter().position(|e| e.subject() == subject);
                if let Some(below) = at.and_then(|at| at.checked_sub(1)) {
                    self.lift_through(below);
                }
                self.lift(id, item.clone(), cause);
            }
        }
        let controller = match *item {
            StackItem::Ability { controller, .. } => controller,
            StackItem::Spell { .. } => return,
        };
        if let Some(Subject::Player { player }) = activator(item)
            && self.drains(player)
        {
            self.drain(player, subject, cause);
        }
        self.dwell(controller, subject);
    }
}
