//! What the engine says of its events beyond the events (plan K23): what each came from, and
//! which spell or ability on the stack that is. The engine tags every event it records with
//! its cause and names a stack entry by its place ([`StackIx`]); [`Names`] follows the stack
//! through the events and gives each entry the name scripts call it by, and a seat's screen is
//! handed, beside each view, what was said of that view's events ([`Heard`]). None of it is
//! serialized: a view carries its script, and the stage reads no tag.

use beryl_engine::event::{Cause as Tag, Event, StackIx, Tagged};
use beryl_engine::state::StackEntry;
use beryl_engine::{Layout, Seat, SlotId as _};

use super::Subject;
use crate::protocol::{Side, StackItem};

/// What an event came from, as the engine's tag says it
/// ([`beryl_engine::event::Cause`]), with the spell or ability it names by the name scripts
/// give it. A seat's screen hears its players by side.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Cause<P = Side> {
    /// The game itself: a turn-based or state-based action.
    #[default]
    Rules,
    /// A player's own action, and the spell or ability it casts, activates, puts on the
    /// stack or pays for.
    Player { player: P, entry: Option<Subject> },
    /// The effect of the resolving spell or ability `entry`: none when the stack was not
    /// followed as it arrived.
    Effect { entry: Option<Subject> },
    /// The effect of an ability of the object in `slot` that never was on the stack, and the
    /// spell or ability whose payment it was activated in: what its mana pays for.
    Object { slot: u16, entry: Option<Subject> },
}

impl<P> Cause<P> {
    /// The spell or ability on the stack the cause names.
    pub fn entry(self) -> Option<Subject> {
        match self {
            Cause::Rules => None,
            Cause::Player { entry, .. } | Cause::Effect { entry } | Cause::Object { entry, .. } => {
                entry
            }
        }
    }

    /// The cause with its player as `name` names them.
    pub fn of<Q>(self, name: impl FnOnce(P) -> Q) -> Cause<Q> {
        match self {
            Cause::Rules => Cause::Rules,
            Cause::Player { player, entry } => Cause::Player {
                player: name(player),
                entry,
            },
            Cause::Effect { entry } => Cause::Effect { entry },
            Cause::Object { slot, entry } => Cause::Object { slot, entry },
        }
    }
}

impl Cause {
    /// What a step it causes names as its cause.
    pub fn subject(self) -> Option<Subject> {
        match self {
            Cause::Rules => None,
            Cause::Player { player, .. } => Some(Subject::Player { player }),
            Cause::Effect { entry } => entry,
            Cause::Object { slot, .. } => Some(Subject::Object { slot }),
        }
    }
}

/// An ability arriving on the stack or leaving it. A spell does both as its card moves.
#[derive(Clone, Debug, PartialEq)]
pub enum Move {
    /// As it stood then: before its targets are chosen.
    Arrival {
        id: u32,
        item: StackItem,
        cause: Cause,
    },
    Departure {
        id: u32,
        cause: Cause,
    },
}

/// What the engine said of one view's events, for the view's seat.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Heard {
    /// What each of the view's events came from, in their order.
    pub causes: Vec<Cause>,
    /// The abilities that arrived on the stack and left it, in order, each with how many of
    /// the view's events came before it.
    pub moves: Vec<(usize, Move)>,
    /// The name of each entry of the view's stack, bottom first, and then of the spell or
    /// ability whose resolution asks the viewer, which their view's stack leaves out.
    pub stack: Vec<Subject>,
}

/// What [`Names`] says of one engine event: its cause with the entry named, and the entry
/// that arrives or leaves if the event is a stack's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Said {
    pub cause: Cause<Seat>,
    /// The entry a [`Event::StackArrival`] puts on the stack or a [`Event::StackDeparture`]
    /// takes off it.
    pub moved: Option<Subject>,
}

/// The stack as the engine's events tell it, each entry by the name scripts give it: a
/// spell's is its card's slot, and an ability's a number from 1 that is never used again. A
/// game keeps one for both seats, so both seats' scripts call an ability the same.
#[derive(Clone, Debug, Default)]
pub struct Names {
    /// Bottom first: the entries that have arrived and not left
    /// ([`StackIx`]: a resolving one keeps its place until its departure).
    stack: Vec<Subject>,
    /// The number of the latest ability named.
    abilities: u32,
}

impl Names {
    fn name<L: Layout>(&mut self, entry: &StackEntry<L>) -> Subject {
        if let Some(spell) = entry.spell() {
            return Subject::Object {
                slot: spell.slot.num(),
            };
        }
        self.abilities += 1;
        Subject::Ability { id: self.abilities }
    }

    /// The entries by name, bottom first.
    pub fn stack(&self) -> &[Subject] {
        &self.stack
    }

    /// Follows the stack through `events`, and says of each what it came from.
    pub fn follow<L: Layout>(&mut self, events: &[Tagged<L>]) -> Vec<Said> {
        let mut said = Vec::with_capacity(events.len());
        for (i, tagged) in events.iter().enumerate() {
            let arrives = match &tagged.event {
                Event::StackArrival { entry } => {
                    let name = self.name(entry);
                    self.stack.push(name);
                    Some(name)
                }
                _ => None,
            };
            let entry = tagged.tag.cause.entry().and_then(|place| {
                self.at(place)
                    .or_else(|| about_to_arrive(&events[i..], place, self.stack.len()))
            });
            let leaves = match tagged.event {
                Event::StackDeparture { entry } if entry.index() < self.stack.len() => {
                    Some(self.stack.remove(entry.index()))
                }
                _ => None,
            };
            let cause = match tagged.tag.cause {
                Tag::Rules => Cause::Rules,
                Tag::Player { player, .. } => Cause::Player { player, entry },
                Tag::Effect { .. } => Cause::Effect { entry },
                Tag::Object { source, .. } => Cause::Object {
                    slot: source.slot.num(),
                    entry,
                },
            };
            said.push(Said {
                cause,
                moved: arrives.or(leaves),
            });
        }
        said
    }

    fn at(&self, place: StackIx) -> Option<Subject> {
        self.stack.get(place.index()).copied()
    }

    /// Makes the names those of `stack`, the engine's entries bottom first with a resolving
    /// one in its place: the entries followed keep their names, and whatever the events did
    /// not tell (a game loaded or taken back, a cast cancelled) is named anew or forgotten.
    pub fn agree<'a, L: Layout>(&mut self, stack: impl IntoIterator<Item = &'a StackEntry<L>>) {
        let mut kept = 0;
        for entry in stack {
            let spell = entry
                .spell()
                .map(|s| Subject::Object { slot: s.slot.num() });
            let same = match (self.stack.get(kept), spell) {
                (Some(&name), Some(spell)) => name == spell,
                (Some(Subject::Ability { .. }), None) => true,
                _ => false,
            };
            if !same {
                self.stack.truncate(kept);
                let name = self.name(entry);
                self.stack.push(name);
            }
            kept += 1;
        }
        self.stack.truncate(kept);
    }
}

/// The spell whose card the first of `events` moves to the stack, to the place `place` just
/// above its top: the one cause that names an entry ahead of its arrival
/// ([CR 601.2a](https://yawgatog.com/resources/magic-rules/#R6012a): "a player first moves
/// that card (or that copy of a card) from where it is to the stack. It becomes the topmost
/// object on the stack.").
fn about_to_arrive<L: Layout>(events: &[Tagged<L>], place: StackIx, top: usize) -> Option<Subject> {
    if place.index() != top {
        return None;
    }
    events[1..].iter().find_map(|later| match &later.event {
        Event::StackArrival { entry } => Some(
            entry
                .spell()
                .map(|s| Subject::Object { slot: s.slot.num() }),
        ),
        _ => None,
    })?
}

#[cfg(test)]
mod tests {
    use beryl_engine::card::FaceAbility;

    use super::*;

    fn ability() -> StackEntry {
        StackEntry {
            ability: Some(FaceAbility::own(0)),
            ..StackEntry::default()
        }
    }

    fn tagged(event: Event, cause: Tag) -> Tagged {
        let together = false;
        let tag = beryl_engine::event::Tag { cause, together };
        Tagged { event, tag }
    }

    const A1: Subject = Subject::Ability { id: 1 };
    const A2: Subject = Subject::Ability { id: 2 };
    const A3: Subject = Subject::Ability { id: 3 };

    /// A cause's place is read in the stack as it stands at its event: the entries above one
    /// that leaves fall by a place, and the one resolving is named until it has left.
    #[test]
    fn a_place_names_the_entry_there_as_its_event_happens() {
        let mut names = Names::default();
        let arrives = |cause| {
            let entry = ability();
            tagged(Event::StackArrival { entry }, cause)
        };
        let effect = |place| Tag::Effect {
            entry: StackIx(place),
        };
        let leaves = |place, cause| {
            let entry = StackIx(place);
            tagged(Event::StackDeparture { entry }, cause)
        };
        let said = names.follow(&[
            arrives(Tag::Rules),
            arrives(Tag::Rules),
            // The lower one leaves from under the upper, which takes its place.
            leaves(0, effect(1)),
            leaves(0, effect(0)),
        ]);
        let told: Vec<_> = said.iter().map(|s| (s.cause.entry(), s.moved)).collect();
        assert_eq!(
            told,
            [
                (None, Some(A1)),
                (None, Some(A2)),
                (Some(A2), Some(A1)),
                (Some(A2), Some(A2)),
            ]
        );
        assert!(names.stack().is_empty());
    }

    /// What the events did not tell is taken from the engine's stack: an entry gone with no
    /// departure is forgotten, and one there with no arrival gets a number never used before.
    #[test]
    fn names_agree_with_a_stack_the_events_did_not_tell() {
        let mut names = Names::default();
        let stack = [ability(), ability()];
        names.agree(&stack);
        assert_eq!(names.stack(), [A1, A2]);
        // The same stack again changes nothing; a cast taken back leaves the first alone.
        names.agree(&stack);
        names.agree(&stack[..1]);
        assert_eq!(names.stack(), [A1]);
        names.agree(&stack);
        assert_eq!(names.stack(), [A1, A3]);
    }
}
