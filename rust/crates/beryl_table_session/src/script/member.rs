use std::collections::BTreeSet;

use beryl_session::protocol::{
    Counter, ObjectId as Slot, ObjectView, Side, StackItem, Why, ZoneName, ZoneRef,
};
use beryl_session::script::{Act, Fact, Notice, Step, Subject, Tally};
use beryl_table::change::{ObjectEdit, TableEdit};
use beryl_table::redact::AnimationEvent;
use beryl_table::state::{ObjectId, Zone};
use beryl_table::{Change, Table};

use super::{Builder, spans};
use crate::view::{Slots, Viewer, arrows, cards, handling, phase, piles, turn, zone};

impl Builder {
    pub(super) fn member(
        &mut self,
        events: &[AnimationEvent],
        table: &Table,
        viewer: Viewer,
        slots: &mut Slots,
        final_slots: &Slots,
    ) {
        let previous = slots.clone();
        let transfers: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                AnimationEvent::HandMove { owner_transfer, .. } => *owner_transfer,
                _ => None,
            })
            .collect();
        if slots.between(table, &transfers, final_slots).is_none() {
            self.error = Some("ran out of stage slots".into());
            return;
        }
        let member = Member {
            table,
            viewer,
            slots,
            previous: &previous,
            draw: events
                .iter()
                .any(|event| matches!(event, AnimationEvent::Draw)),
            returning_hand: events
                .iter()
                .any(|event| matches!(event, AnimationEvent::MulliganReturn)),
        };
        let beat = self.member_steps(&member, events);
        let mut wanted = spans::wanted(handling(table, viewer), &arrows(table, slots, viewer));
        wanted.extend(self.spans.iter().copied().filter(|span| {
            matches!(span, beryl_session::script::Open::Mark { on: Subject::Object { slot }, .. } if self.stage.objects.contains_key(slot))
        }));
        let new_look = events
            .iter()
            .any(|event| matches!(event, AnimationEvent::Look { .. }));
        self.end_spans(&wanted, &beat, new_look);
        self.finish(beat);
        self.begin_spans(&wanted);
    }

    fn member_steps(&mut self, member: &Member<'_>, events: &[AnimationEvent]) -> Vec<Step> {
        let mut beat = Vec::new();
        self.anonymous_moves(&mut beat, member, events);
        let arrivals: BTreeSet<_> = events
            .iter()
            .filter_map(|event| match event {
                AnimationEvent::Move { id, .. } => Some(*id),
                _ => None,
            })
            .collect();
        let peek = events
            .iter()
            .find_map(|event| match event {
                AnimationEvent::Peek(ids) => Some(ids.as_slice()),
                _ => None,
            })
            .unwrap_or_default();
        for event in events {
            match event {
                AnimationEvent::Move { id, from, to } => {
                    let (zone, side) = member.zone_ref(to.zone);
                    let len = if zone == ZoneName::Stack {
                        self.stage.stack.len()
                    } else {
                        self.stage.order(zone, side).map_or(0, Vec::len)
                    };
                    let at = if reversed(to.zone) {
                        len.checked_sub(usize::from(from.zone == to.zone) + to.index)
                    } else {
                        Some(to.index)
                    };
                    self.fly_to(&mut beat, member, *id, from.zone, at, None);
                }
                AnimationEvent::HandMove {
                    from,
                    to,
                    owner_transfer: Some((_, id)),
                    ..
                } => {
                    let at = self.insertion(member, *id, *to);
                    self.fly_to(&mut beat, member, *id, *from, at, None);
                }
                AnimationEvent::Shuffle {
                    library,
                    order,
                    announce,
                } => {
                    self.shuffled_library(&mut beat, member, *library, order);
                    if *announce {
                        self.emit(
                            &mut beat,
                            Act::Announce {
                                notice: Notice::Shuffle,
                            },
                        );
                    }
                }
                AnimationEvent::Reveal { cards, .. } => {
                    for card in cards {
                        if let Some(object) = member.object(card.id) {
                            self.emit(&mut beat, Act::Showcase { object });
                        }
                    }
                }
                AnimationEvent::Phase(value) => {
                    self.emit(
                        &mut beat,
                        Act::Announce {
                            notice: Notice::Phase {
                                phase: phase(*value).0,
                            },
                        },
                    );
                }
                AnimationEvent::Learn(card) if !arrivals.contains(&card.id) => {
                    if let Some(object) = member.object(card.id)
                        && self.stage.objects.get(&object.slot) != Some(&object)
                    {
                        let visual = if peek.contains(&card.id) {
                            Act::Flip { object }
                        } else {
                            Act::Set {
                                fact: Fact::Object { object },
                            }
                        };
                        self.emit(&mut beat, visual);
                    }
                }
                AnimationEvent::Change(change) => {
                    self.change(&mut beat, member, change, &arrivals);
                }
                _ => {}
            }
        }
        self.finish_peek(&mut beat, member, peek);
        // A face edit can expose a card and change derived characteristics;
        // a changed slot can also leave a marker's source unavailable.
        self.settle_records(&mut beat, member);
        self.set_turn(&mut beat, member, events);
        self.set_library_top(&mut beat, member, events);
        beat
    }

    fn finish_peek(&mut self, beat: &mut Vec<Step>, member: &Member<'_>, peek: &[ObjectId]) {
        for &id in peek {
            if let Some(object) = member.object(id)
                && !beat.iter().any(|step| {
                    matches!(&step.act, Act::Flip { object: flipped } if flipped.slot == object.slot)
                })
            {
                self.emit(beat, Act::Flip { object });
            }
        }
    }

    fn shuffled_library(
        &mut self,
        beat: &mut Vec<Step>,
        member: &Member<'_>,
        library: Zone,
        order: &[ObjectId],
    ) {
        let Zone::Library(seat) = library else {
            unreachable!("shuffle zone");
        };
        self.emit(
            beat,
            Act::Set {
                fact: Fact::ShuffledLibrary {
                    player: member.viewer.side(seat),
                    cards: order
                        .iter()
                        .rev()
                        .filter_map(|&id| member.object(id))
                        .collect(),
                },
            },
        );
    }

    fn anonymous_moves(
        &mut self,
        beat: &mut Vec<Step>,
        member: &Member<'_>,
        events: &[AnimationEvent],
    ) {
        let lanes: BTreeSet<_> = events
            .iter()
            .filter_map(|event| match event {
                AnimationEvent::HandMove {
                    from,
                    to,
                    owner_transfer: None,
                    ..
                } => Some((*from, *to)),
                _ => None,
            })
            .collect();
        if lanes.is_empty() {
            return;
        }
        // Compare sets of departed and arrived slots. Sending a pair of old
        // and new slots per flight would let the viewer track a forgotten card.
        let mut retained = member.slots.assigned();
        // Removal has a verb of its own even if other cards in the same
        // selection lost their ids, as when returning a token to hand.
        retained.extend(events.iter().filter_map(|event| match event {
            AnimationEvent::Change(Change::Remove { object, .. }) => member.previous.get(object.id),
            _ => None,
        }));
        let removed: BTreeSet<_> = lanes
            .iter()
            .flat_map(|&(from, _)| {
                let (name, side) = member.zone_ref(from);
                self.stage
                    .order(name, side)
                    .into_iter()
                    .flatten()
                    .copied()
                    .chain(
                        self.stage
                            .stack
                            .iter()
                            .filter_map(move |item| match item.item {
                                StackItem::Spell { slot, .. } if name == ZoneName::Stack => {
                                    Some(slot)
                                }
                                _ => None,
                            }),
                    )
                    .filter(|slot| !retained.contains(slot))
            })
            .collect();
        if !removed.is_empty() {
            self.emit(
                beat,
                Act::Set {
                    fact: Fact::Forget {
                        slots: removed.into_iter().collect(),
                    },
                },
            );
        }
        // Undo can restore a vanished token alongside cards returning from a hand.
        // The token appears where it was removed, without a flight from the hand.
        let created: BTreeSet<_> = events
            .iter()
            .filter_map(|event| match event {
                AnimationEvent::Change(Change::Create { object, .. }) => Some(object.id),
                _ => None,
            })
            .collect();
        for (from, to) in lanes {
            let (zone, side) = member.zone_ref(from);
            let origin = Some(ZoneRef { zone, side });
            for &id in member.table.zone(to).into_iter().flatten() {
                if member.previous.get(id).is_none()
                    && !created.contains(&id)
                    && let Some(slot) = member.slots.get(id)
                    && !self.stage.objects.contains_key(&slot)
                {
                    let at = self.insertion(member, id, to);
                    self.fly_to(beat, member, id, from, at, origin);
                }
            }
        }
    }

    fn fly_to(
        &mut self,
        beat: &mut Vec<Step>,
        member: &Member<'_>,
        id: ObjectId,
        from: Zone,
        at: Option<usize>,
        origin: Option<ZoneRef>,
    ) {
        if let Some(object) = member.object(id) {
            let from = zone(from).0;
            let item = (object.zone == ZoneName::Stack).then(|| StackItem::spell(object.slot));
            let why = if from == ZoneName::Library && member.draw {
                Why::Draw
            } else {
                Why::Effect
            };
            self.emit(
                beat,
                Act::Fly {
                    from,
                    why,
                    object,
                    at: index(at),
                    item,
                    origin,
                },
            );
        }
    }

    fn insertion(&self, member: &Member<'_>, id: ObjectId, zone: Zone) -> Option<usize> {
        let order = member.table.zone(zone)?;
        let index = order.iter().position(|&candidate| candidate == id)?;
        let before = if reversed(zone) {
            &order[index + 1..]
        } else {
            &order[..index]
        };
        Some(
            before
                .iter()
                .filter(|&&id| {
                    if let Some(slot) = member.slots.get(id) {
                        self.stage.objects.get(&slot).is_some_and(|object| {
                            let (name, side) = member.zone_ref(zone);
                            object.zone == name
                                && (matches!(name, ZoneName::Battlefield | ZoneName::Stack)
                                    || object.zone_side.unwrap_or(object.owner) == side)
                        })
                    } else {
                        zone == Zone::Stack
                            && self.stage.stack.iter().any(|item| item.id == Some(id.0))
                    }
                })
                .count(),
        )
    }

    fn change(
        &mut self,
        beat: &mut Vec<Step>,
        member: &Member<'_>,
        change: &Change,
        arrivals: &BTreeSet<ObjectId>,
    ) {
        let act = match change {
            Change::SetObject { id, edit } if !arrivals.contains(id) => {
                let Some(object) = member.object(*id) else {
                    return;
                };
                let Some(was) = self.stage.objects.get(&object.slot) else {
                    return;
                };
                let Some(act) = object_edit(edit, was, object) else {
                    return;
                };
                act
            }
            Change::Create { object: card, .. } => {
                let Some(object) = member.object(card.id) else {
                    return;
                };
                let item = (object.zone == ZoneName::Stack).then(|| StackItem::spell(object.slot));
                let at = index(self.insertion(member, card.id, card.zone));
                Act::Appear {
                    from: ZoneName::Nowhere,
                    why: Why::Created,
                    object,
                    at,
                    item,
                }
            }
            Change::Remove { object, .. } => {
                let Some(slot) = member.removed_slot(object.id) else {
                    return;
                };
                Act::Vanish {
                    subject: Subject::Object { slot },
                    from: zone(object.zone).0,
                    why: Why::Effect,
                    object: None,
                    at: None,
                }
            }
            Change::CreateMarker { id, marker, .. } => Act::Emerge {
                id: id.0,
                item: cards::marker(*id, marker, member.slots, member.viewer),
                at: index(self.insertion(member, *id, Zone::Stack)),
            },
            Change::RemoveMarker { id, .. } => Act::Vanish {
                subject: Subject::Ability { id: id.0 },
                from: ZoneName::Stack,
                why: Why::Resolve,
                object: None,
                at: None,
            },
            Change::Roll { sides, outcomes } => Act::Announce {
                notice: Notice::Roll {
                    sides: *sides,
                    outcomes: outcomes.clone(),
                },
            },
            Change::SetTable(TableEdit::SetLife { seat, edit }) => Act::Count {
                tally: Tally::Life {
                    player: member.viewer.side(*seat),
                    life: edit.after,
                },
            },
            Change::SetTable(TableEdit::SetPlayerCounters { seat, .. }) => {
                let side = member.viewer.side(*seat);
                let (next, _) = piles(member.table, *seat, member.slots);
                let before = self.stage.piles(side);
                let counters = before.named_counters != next.named_counters;
                if before.mana != next.mana {
                    self.emit(
                        beat,
                        Act::Count {
                            tally: Tally::Pool {
                                player: side,
                                mana: next.mana,
                            },
                        },
                    );
                }
                if counters {
                    self.emit(
                        beat,
                        Act::Count {
                            tally: Tally::PlayerCounters {
                                player: side,
                                counters: next.named_counters,
                            },
                        },
                    );
                }
                return;
            }
            _ => return,
        };
        self.emit(beat, act);
    }

    fn set_turn(&mut self, beat: &mut Vec<Step>, member: &Member<'_>, events: &[AnimationEvent]) {
        let (_, next) = turn(member.table, member.viewer);
        if next != self.stage.turn {
            self.emit(
                beat,
                Act::Set {
                    fact: Fact::Turn {
                        number: next.number,
                        own_number: next.own_number,
                        active: next.active,
                        step: next.step,
                    },
                },
            );
        }
        if events.iter().any(|event| {
            matches!(
                event,
                AnimationEvent::Change(Change::SetTable(TableEdit::SetTurn { .. }))
            )
        }) {
            let name = format!("Seat {}", u16::from(member.table.active.0) + 1);
            self.emit(
                beat,
                Act::Announce {
                    notice: Notice::TableTurn { name },
                },
            );
        }
    }

    fn set_library_top(
        &mut self,
        beat: &mut Vec<Step>,
        member: &Member<'_>,
        events: &[AnimationEvent],
    ) {
        for event in events {
            if let AnimationEvent::Change(
                Change::AlwaysRevealTop { seat, .. } | Change::AlwaysLookAtTop { seat, .. },
            ) = event
            {
                let player = member.viewer.side(*seat);
                let order = &self.stage.piles(player).library;
                let fact = order
                    .last()
                    .and_then(|slot| self.stage.objects.get(slot))
                    .map_or_else(
                        || Fact::Library {
                            player,
                            order: Vec::new(),
                        },
                        |object| Fact::Object {
                            object: object.clone(),
                        },
                    );
                self.emit(beat, Act::Set { fact });
            }
        }
    }

    fn settle_records(&mut self, beat: &mut Vec<Step>, member: &Member<'_>) {
        for &id in member.table.objects.keys() {
            if let Some(object) = member.object(id)
                && self
                    .stage
                    .objects
                    .get(&object.slot)
                    .is_some_and(|was| was != &object)
            {
                self.emit(
                    beat,
                    Act::Set {
                        fact: Fact::Object { object },
                    },
                );
            }
        }
        for (&id, marker) in &member.table.markers {
            let item = cards::marker(id, marker, member.slots, member.viewer);
            if self
                .stage
                .stack
                .iter()
                .any(|entry| entry.id == Some(id.0) && entry.item != item)
            {
                self.emit(
                    beat,
                    Act::Set {
                        fact: Fact::Item {
                            subject: Subject::Ability { id: id.0 },
                            item,
                        },
                    },
                );
            }
        }
    }
}

struct Member<'a> {
    table: &'a Table,
    viewer: Viewer,
    slots: &'a Slots,
    previous: &'a Slots,
    draw: bool,
    returning_hand: bool,
}

impl Member<'_> {
    fn object(&self, id: ObjectId) -> Option<ObjectView> {
        let card = self.table.objects.get(&id)?;
        let slot = self.slots.get(id)?;
        if self.returning_hand && matches!(card.zone, Zone::Library(_)) {
            // The hand goes back face down before the shuffle. Keep its
            // temporary library records anonymous even for the drawer.
            let mut hidden = card.clone();
            hidden.card = beryl_table::state::Card::Hidden;
            hidden.note = None;
            hidden.counters.clear();
            return Some(cards::object(&hidden, slot, self.slots, self.viewer));
        }
        Some(cards::object(card, slot, self.slots, self.viewer))
    }

    fn zone_ref(&self, area: Zone) -> (ZoneName, Side) {
        let (zone, seat) = zone(area);
        (zone, seat.map_or(Side::You, |seat| self.viewer.side(seat)))
    }

    fn removed_slot(&self, id: ObjectId) -> Option<Slot> {
        self.previous.get(id)
    }
}

fn object_edit(edit: &ObjectEdit, was: &ObjectView, object: ObjectView) -> Option<Act> {
    Some(match edit {
        ObjectEdit::Tapped(_) => Act::Tap { object },
        ObjectEdit::Face(_) => Act::Flip { object },
        ObjectEdit::Stats(_) => Act::Count {
            tally: Tally::Stats { object },
        },
        ObjectEdit::Damage(_) => Act::Count {
            tally: Tally::Damage { object },
        },
        ObjectEdit::Counters(edit) => {
            let name = edit
                .before
                .iter()
                .chain(&edit.after)
                .find(|(name, _)| counter(&edit.before, name) != counter(&edit.after, name))
                .map(|(name, _)| name)?;
            let tally = if let Some(counter) = Counter::named(name) {
                Tally::Counters { counter, object }
            } else {
                Tally::NamedCounters {
                    name: name.clone(),
                    object,
                }
            };
            Act::Count { tally }
        }
        ObjectEdit::Controller(_) | ObjectEdit::AttachedTo(_) | ObjectEdit::Row(_) => Act::Fly {
            from: was.zone,
            why: Why::Effect,
            object,
            at: None,
            item: None,
            origin: None,
        },
        ObjectEdit::Note(_) | ObjectEdit::DoesntUntap(_) => Act::Set {
            fact: Fact::Object { object },
        },
    })
}

fn reversed(zone: Zone) -> bool {
    matches!(
        zone,
        Zone::Library(_) | Zone::Graveyard(_) | Zone::Exile(_) | Zone::Sideboard(_) | Zone::Stack
    )
}

fn index(at: Option<usize>) -> Option<u16> {
    at.map(|at| u16::try_from(at).expect("there are at most 65536 card slots"))
}

fn counter(counters: &[(String, i32)], name: &str) -> Option<i32> {
    counters
        .iter()
        .find(|(label, _)| label == name)
        .map(|(_, count)| *count)
}
