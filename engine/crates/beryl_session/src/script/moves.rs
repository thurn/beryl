//! How a card moves in a script: the step a change of zone becomes, the record the card has
//! as it arrives, and its place in the pile it lands on.

use super::build::Builder;
use super::hear::{Cause, Copied};
use super::{Act, Subject};
use crate::protocol::ObjectId;
use crate::protocol::{
    Counter, CounterView, Facing, GameEvent, ObjectView, Side, StackItem, TargetRef, Why, ZoneName,
};

/// The order a view lists an object's counters in.
pub(super) const COUNTERS: [Counter; 6] = [
    Counter::PlusOne,
    Counter::MinusOne,
    Counter::Loyalty,
    Counter::Time,
    Counter::Story,
    Counter::Chorus,
];

pub(super) fn counted(counters: &[CounterView], kind: Counter) -> i32 {
    counters
        .iter()
        .find(|c| c.kind == kind)
        .map_or(0, |c| c.count)
}

/// Sets how many `kind` counters `counters` lists, as a view lists them: in order, and none
/// of a kind there are none of.
pub(super) fn set_counters(counters: &mut Vec<CounterView>, kind: Counter, count: i32) {
    counters.retain(|c| c.kind != kind);
    if count > 0 {
        let place = |k: Counter| COUNTERS.iter().position(|&c| c == k);
        let at = counters
            .iter()
            .position(|c| place(c.kind) > place(kind))
            .unwrap_or(counters.len());
        counters.insert(at, CounterView { kind, count });
    }
}

impl Builder<'_> {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn zone_change(
        &mut self,
        i: usize,
        slot: ObjectId,
        (from, to): (ZoneName, ZoneName),
        why: Why,
        library_top: Option<bool>,
        still: bool,
        cause: Option<Subject>,
    ) {
        let was = self.table.objects.get(&slot);
        let token = was.is_some_and(|o| o.token);
        // [CR 708.9](https://yawgatog.com/resources/magic-rules/#R7089): "If a face-down
        // permanent or a face-down component of a merged permanent moves from the
        // battlefield to any other zone, its owner must reveal it to all players as they
        // move it." Where it goes to a zone that shows no face, it is held up to be read.
        let unveiled = !public(to)
            && was.is_some_and(|o| o.zone == ZoneName::Battlefield && o.facing == Facing::Down);
        // A token that leaves the battlefield ceases to exist
        // ([CR 704.5d](https://yawgatog.com/resources/magic-rules/#R7045d): "If a token is
        // in a zone other than the battlefield, it ceases to exist"), and a copy of a spell
        // as it leaves the stack: no view has a record of either afterwards.
        let ceases = to == ZoneName::Nowhere
            || (token
                && !matches!(to, ZoneName::Battlefield | ZoneName::Stack)
                && !self.lingers(i, slot, from, to));
        if ceases || from == ZoneName::Nowhere {
            if was.is_some() {
                let subject = Subject::Object { slot };
                self.emit(
                    Act::Vanish {
                        subject,
                        from: if ceases { from } else { ZoneName::Nowhere },
                        why: if ceases { why } else { Why::StateBased },
                        object: None,
                        at: None,
                    },
                    cause,
                );
            }
            if ceases {
                return;
            }
        } else if from == to || was.is_some_and(|o| o.zone == to) {
            // A place in the same pile is settled as the pile's order, and what the table
            // shows already has nothing left to show.
            return;
        }
        let Some(object) = self.arrived(i, slot, to) else {
            return;
        };
        let at = self.landing(&object, library_top);
        let item = (to == ZoneName::Stack).then(|| self.spell(i, slot));
        if to == ZoneName::Stack {
            let after = self.after;
            let place = after
                .stack
                .iter()
                .position(|e| matches!(e.item, StackItem::Spell { slot: s, .. } if s == slot));
            if let Some(below) = place.and_then(|at| at.checked_sub(1)) {
                self.lift_through(below);
            }
        }
        self.travel(from, why, object, at, item, still, cause);
        if unveiled && let Some(object) = self.table.objects.get(&slot).cloned() {
            self.showcase(object, cause);
        }
    }

    /// Whether the copy of a spell in `slot`, which event `i` moves from the stack to `to`, is
    /// still shown there after the move.
    ///
    /// A copy of a spell that leaves the stack goes where a card would and ceases to exist
    /// only at the next state-based check ([CR 704.5e]): "If a copy of a spell is in a zone
    /// other than the stack, it ceases to exist." The engine reports no event for that, so
    /// the script usually lets the copy vanish as it leaves the stack. A copy whose effect
    /// first exiles the spell itself is still in exile while the rest of the effect resolves:
    /// later events name it as their cause, and a view drawn at a choice within the effect
    /// lists it in exile. Then the copy moves to that zone as a card does, and the script's
    /// end (`settle.rs`) makes it vanish once a view no longer lists it.
    ///
    /// [CR 704.5e]: https://yawgatog.com/resources/magic-rules/#R7045e
    fn lingers(&self, i: usize, slot: ObjectId, from: ZoneName, to: ZoneName) -> bool {
        let subject = Some(Subject::Object { slot });
        from == ZoneName::Stack
            && (self.after.objects.get(&slot).is_some_and(|o| o.zone == to)
                || self.causes[i + 1..]
                    .iter()
                    .any(|cause| cause.subject() == subject))
    }

    /// Event `i` reveals the card in `slot`
    /// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
    /// show that card to all players for a brief time." It is held up to be read where it
    /// lies, named as the seat's view comes to name it. One that goes on to a zone that shows
    /// its face is not: its flight there shows it. A card the seat's own effect reveals is
    /// not held up either: the seat already knows it, and the pile is for what the
    /// opponents are shown.
    pub(super) fn reveal(
        &mut self,
        i: usize,
        player: Side,
        slot: ObjectId,
        cause: Option<Subject>,
    ) {
        if player == Side::You {
            return;
        }
        let Some(was) = self.table.objects.get(&slot) else {
            // A card revealed by an effect bringing it in first appears in the final view.
            if let Some(object) = self.after.objects.get(&slot).cloned() {
                self.showcase(object, cause);
            }
            return;
        };
        let mut zone = was.zone;
        // Where its next move takes it, and whether the view's end still names this card:
        // a library gives a slot another card when it is shuffled.
        let (mut next, mut kept) = (None, true);
        for event in &self.events[i + 1..] {
            match event {
                &GameEvent::ZoneChange { slot: s, to, .. } if s == slot => {
                    next.get_or_insert(to);
                    zone = to;
                    kept &= to != ZoneName::Library;
                }
                GameEvent::Draw {
                    slots: Some(slots), ..
                } if slots.contains(&slot) => {
                    next.get_or_insert(ZoneName::Hand);
                    zone = ZoneName::Hand;
                }
                // A shared library is every player's, whoever shuffles it.
                &GameEvent::LibraryShuffled { player } => {
                    kept &= zone != ZoneName::Library
                        || (self.table.shared.is_none() && player != was.owner);
                }
                _ => {}
            }
        }
        if next.is_some_and(public) {
            return;
        }
        let end = self.after.objects.get(&slot).filter(|_| kept);
        let (name, other_face) = if let Some(known) = was.name.is_some().then_some(was).or(end)
            && known.name.is_some()
        {
            (known.name.clone(), known.other_face.clone())
        } else if let Some((_, name, other_face)) = self
            .revealed
            .iter()
            .find(|(revealed, _, _)| *revealed == slot)
        {
            (Some(name.clone()), other_face.clone())
        } else {
            return;
        };
        let object = ObjectView {
            name,
            other_face,
            ..was.clone()
        };
        self.showcase(object, cause);
    }

    /// Holds `object` up to be read, if the seat knows its face: what the seat may not see
    /// has no step.
    fn showcase(&mut self, object: ObjectView, cause: Option<Subject>) {
        if object.name.is_some() && object.facing == Facing::Up {
            self.emit(Act::Showcase { object }, cause);
        }
    }

    /// Writes the step that takes `object` from `from` to where its record puts it, with the
    /// verb that move has (the table of changes on by-w8gv.21.8, from Arena).
    #[allow(clippy::too_many_arguments)]
    pub(super) fn travel(
        &mut self,
        from: ZoneName,
        why: Why,
        object: ObjectView,
        at: Option<u16>,
        item: Option<StackItem>,
        still: bool,
        cause: Option<Subject>,
    ) {
        let to = object.zone;
        let subject = Subject::Object { slot: object.slot };
        let own_spell_back = to == ZoneName::Hand && object.owner == Side::You;
        // A permanent put into a library is at the library face up within a frame and
        // slips in, with no ash or vortex where it was (TbrYIs2juCw 1:09:04.55, jD7VqaT8VC8
        // 23:05.50): it appears there.
        let to_library = from == ZoneName::Battlefield && to == ZoneName::Library;
        let goes = match from {
            ZoneName::Nowhere => false,
            _ if to == ZoneName::Stack || to == ZoneName::Battlefield => false,
            ZoneName::Battlefield => !to_library,
            ZoneName::Stack => !own_spell_back,
            _ => still && to == ZoneName::Library,
        };
        // An object not yet on the table (such as a copy of a card created in exile and cast)
        // has no prior pose to fly from, so it appears at its destination like a token.
        let new = !self.table.objects.contains_key(&object.slot);
        let act = if goes {
            Act::Vanish {
                subject,
                from,
                why,
                object: Some(object),
                at,
            }
        } else if from == ZoneName::Nowhere || new || to_library || (still && to != ZoneName::Stack)
        {
            Act::Appear {
                from,
                why,
                object,
                at,
                item,
            }
        } else {
            Act::Fly {
                from,
                why,
                object,
                at,
                item,
                origin: None,
            }
        };
        self.emit(act, cause);
    }

    /// The record of the object in `slot` once event `i` has put it in `to`: the view's, as
    /// it was before the events still to come, where the object ends there; else what the
    /// seat knows of the card, in that zone.
    fn arrived(&self, i: usize, slot: ObjectId, to: ZoneName) -> Option<ObjectView> {
        let later = &self.events[i + 1..];
        let moves = |e: &GameEvent| match e {
            GameEvent::ZoneChange { slot: s, .. } => *s == slot,
            GameEvent::Draw {
                slots: Some(slots), ..
            } => slots.contains(&slot),
            _ => false,
        };
        let was = self.table.objects.get(&slot);
        let end = self.after.objects.get(&slot);
        if let Some(end) = end
            && end.zone == to
            && !later.iter().any(moves)
        {
            return Some(rewound(end.clone(), later));
        }
        if to == ZoneName::Stack
            && was.is_none()
            && let Some(copy) = self.copy(i, slot)
        {
            return Some(copy.record.clone());
        }
        let base = was.or(end)?;
        let owner = self.owner_after(i, base, to);
        // A library gives a slot another card when it is shuffled: what the view's end
        // calls the slot is then no name for this one.
        let reshuffled = later.iter().any(|e| {
            matches!(
                *e,
                GameEvent::ZoneChange { slot: s, from, to, .. }
                    if s == slot && (from == ZoneName::Library || to == ZoneName::Library)
            )
        });
        let identity = match to {
            ZoneName::Library => None,
            ZoneName::Hand if owner == Side::Opponent => Some(base),
            _ if base.name.is_some() => Some(base),
            _ => end.filter(|_| !reshuffled),
        };
        let controller = if matches!(to, ZoneName::Stack | ZoneName::Battlefield) {
            base.controller
        } else {
            owner
        };
        Some(ObjectView {
            name: identity.and_then(|o| o.name.clone()),
            other_face: identity.and_then(|o| o.other_face.clone()),
            token: base.token,
            ..ObjectView::hidden(slot, to, controller, owner)
        })
    }

    /// The owner of the card whose record is `base` once event `i` has put it in `to`. A card
    /// in a shared library or graveyard has none, and its record names one fixed side
    /// ([`Table::holder`](super::Table::holder)); the engine makes the player who takes a card
    /// out of one its owner. Who that is: the owner the view's end names, where the card ends
    /// out of the shared piles, else the player the event names or acts for.
    fn owner_after(&self, i: usize, base: &ObjectView, to: ZoneName) -> Side {
        let shares = |zone| self.table.shares(zone);
        if shares(to) || !shares(base.zone) {
            return self.table.holder(to, base.owner);
        }
        let end = self.after.objects.get(&base.slot);
        end.filter(|o| !shares(o.zone))
            .map(|o| o.owner)
            .or_else(|| self.actor(i))
            .unwrap_or(base.owner)
    }

    /// The player event `i` names as acting, or whose action or spell or ability caused it.
    pub(super) fn actor(&self, i: usize) -> Option<Side> {
        match self.events[i] {
            GameEvent::Draw { player, .. }
            | GameEvent::LibraryShuffled { player }
            | GameEvent::Scry { player, .. }
            | GameEvent::TopOrBottom { player, .. }
            | GameEvent::ValueChosen { player, .. }
            | GameEvent::Reveal { player, .. } => return Some(player),
            _ => {}
        }
        let controller = |subject: Subject| match subject {
            Subject::Player { player } => Some(player),
            Subject::Object { slot } => self.table.objects.get(&slot).map(|o| o.controller),
            Subject::Ability { .. } => self.table.stack.iter().find_map(|e| match e.item {
                StackItem::Ability { controller, .. } if e.subject() == subject => Some(controller),
                _ => None,
            }),
        };
        match self.causes[i] {
            Cause::Rules => None,
            Cause::Player { player, .. } => Some(player),
            Cause::Effect { entry } => entry.and_then(controller),
            Cause::Object { slot, .. } => controller(Subject::Object { slot }),
        }
    }

    /// Where in its pile `object` lands: the bottom of a library if the event says so, else
    /// where the view's end has it, counted from the top, and the top otherwise.
    fn landing(&self, object: &ObjectView, library_top: Option<bool>) -> Option<u16> {
        if object.zone != ZoneName::Library {
            return None;
        }
        if library_top == Some(false) {
            return Some(0);
        }
        let end = &self.after.piles(object.owner).library;
        let now = &self.table.piles(object.owner).library;
        let others = now.iter().filter(|&&s| s != object.slot).count();
        let place = end.iter().position(|&s| s == object.slot)?;
        let at = others.checked_sub(end.len() - 1 - place)?;
        (at < others).then(|| u16::try_from(at).ok()).flatten()
    }

    /// The copy of a spell that event `i` puts on the stack in `slot`, as it arrived there:
    /// the first such arrival after the event.
    fn copy(&self, i: usize, slot: ObjectId) -> Option<&Copied> {
        self.copies
            .iter()
            .find(|(before, copy)| *before > i && copy.record.slot == slot)
            .map(|(_, copy)| copy)
    }

    /// The spell the card in `slot` becomes as event `i` puts it on the stack: as its cast
    /// goes on to announce it, as a copy arrived, or as the view's stack has it, or with
    /// nothing chosen yet.
    fn spell(&self, i: usize, slot: ObjectId) -> StackItem {
        let announced = self.events[i + 1..].iter().find_map(|e| match e {
            GameEvent::Cast { slot: s, item, .. } if *s == slot => Some(item),
            _ => None,
        });
        let shown = self
            .after
            .stack
            .iter()
            .map(|e| &e.item)
            .find(|item| matches!(item, StackItem::Spell { slot: s, .. } if *s == slot));
        let copied = self.copy(i, slot).map(|copy| &copy.item);
        announced
            .or(copied)
            .or(shown)
            .cloned()
            .unwrap_or_else(|| StackItem::Spell {
                slot,
                targets: Vec::new(),
                x: 0,
                paid: Vec::new(),
            })
    }
}

/// Whether every player sees the faces of the cards in `zone`
/// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)): "Public zones are zones
/// in which all players can see the cards' faces, except for those cards that some rule or
/// effect specifically allow to be face down. Graveyard, battlefield, stack, exile, ante,
/// and command are public zones."
fn public(zone: ZoneName) -> bool {
    matches!(
        zone,
        ZoneName::Battlefield | ZoneName::Stack | ZoneName::Graveyard | ZoneName::Exile
    )
}

/// `object`, a record from the view's end, as it was before the events of `later`.
fn rewound(mut object: ObjectView, later: &[GameEvent]) -> ObjectView {
    let slot = object.slot;
    let count = |object: &mut ObjectView, counter: Counter, by: i16| {
        let count = counted(&object.counters, counter) + i32::from(by);
        let count = count.clamp(0, i32::from(u8::MAX));
        set_counters(&mut object.counters, counter, count);
    };
    for event in later.iter().rev() {
        match event {
            GameEvent::Tap { slot: s } if *s == slot => object.tapped = false,
            GameEvent::Untap { slot: s } if *s == slot => object.tapped = true,
            &GameEvent::CountersAdded {
                slot: s,
                counter,
                amount,
            } if s == slot => count(&mut object, counter, -i16::from(amount)),
            &GameEvent::CountersRemoved {
                slot: s,
                counter,
                amount,
            } if s == slot => count(&mut object, counter, i16::from(amount)),
            GameEvent::Damage {
                to: TargetRef::Object { slot: s },
                amount,
                ..
            } if *s == slot => object.damage = (object.damage - i32::from(*amount)).max(0),
            GameEvent::GainControl { slot: s, .. } if *s == slot => {
                object.controller = object.owner;
            }
            GameEvent::Attack { attackers, .. } if attackers.contains(&slot) => {
                object.combat = None;
            }
            _ => {}
        }
    }
    object
}
