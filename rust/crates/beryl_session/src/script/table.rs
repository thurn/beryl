//! The table: the part of a view a script accounts for (the module's "The table and replay"),
//! and what each step does to it. The session's builder keeps one per seat and the stage keeps
//! the same one: a step means here what it means on screen.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{Act, Fact, Subject, Tally};
use crate::protocol::ObjectId;
use crate::protocol::{
    ManaPool, ObjectView, PlayerState, PlayerView, Side, StackItem, Status, Step as TurnStep,
    ZoneName,
};

/// What a seat's screen shows of the game.
#[derive(Clone, Debug, Serialize)]
pub struct Table {
    /// Every object in a zone, by slot.
    pub objects: BTreeMap<ObjectId, ObjectView>,
    /// Permanents, in the order they entered.
    pub battlefield: Vec<ObjectId>,
    /// Bottom first.
    pub stack: Vec<Entry>,
    pub you: Piles,
    pub opponent: Piles,
    pub turn: Turn,
}

/// A spell or ability on the stack.
#[derive(Clone, Debug, Serialize)]
pub struct Entry {
    pub item: StackItem,
    /// An ability's number; none for a spell, which is its card.
    #[serde(skip)]
    pub id: Option<u32>,
}

/// One player's numbers and ordered piles.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Piles {
    pub life: i16,
    pub mana: ManaPool,
    /// Bottom first.
    pub library: Vec<ObjectId>,
    /// Bottom first.
    pub graveyard: Vec<ObjectId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Turn {
    pub number: u16,
    pub active: Side,
    pub step: TurnStep,
}

impl Piles {
    fn of(player: &PlayerState) -> Piles {
        Piles {
            life: player.life,
            mana: player.mana,
            library: player.library.clone(),
            graveyard: player.graveyard.clone(),
        }
    }
}

impl Entry {
    /// What a step calls it.
    pub fn subject(&self) -> Subject {
        match (&self.item, self.id) {
            (StackItem::Spell { slot, .. }, _) => Subject::Object { slot: *slot },
            (StackItem::Ability { .. }, Some(id)) => Subject::Ability { id },
            (StackItem::Ability { .. }, None) => unreachable!("an ability on a table is numbered"),
        }
    }
}

impl Table {
    /// The table of `view`, its abilities not yet numbered. The stack is the view's with two
    /// differences, so that a spell is on the stack exactly while its card is, for both seats.
    /// On top is the spell or ability whose resolution asks the viewer, which a view leaves
    /// out of its list ([`crate::protocol::DecisionView::resolving`]) and keeps there while
    /// the opponent is asked: always, also when the list has another that reads the same (a
    /// second trigger of one ability). And a spell whose card has left the stack is not on it, though
    /// its resolution goes on: a spell that exiles itself as it resolves.
    pub fn of(view: &PlayerView) -> Table {
        Table::named(view, &[])
    }

    /// [`Table::of`], its abilities numbered as `names` says: the name of each entry of the
    /// view's stack, bottom first, and then of the spell or ability whose resolution asks the
    /// viewer ([`super::Heard::stack`]). An ability `names` has no number for stays unnumbered.
    pub fn named(view: &PlayerView, names: &[Subject]) -> Table {
        let objects: BTreeMap<ObjectId, ObjectView> =
            view.objects.iter().map(|o| (o.slot, o.clone())).collect();
        let resolving = match &view.status {
            Status::Deciding { decision } => decision.resolving.as_ref(),
            _ => None,
        };
        let stack = view
            .stack
            .iter()
            .chain(resolving)
            .enumerate()
            .filter(|(_, item)| match item {
                StackItem::Spell { slot, .. } => {
                    objects.get(slot).is_some_and(|o| o.zone == ZoneName::Stack)
                }
                StackItem::Ability { .. } => true,
            })
            .map(|(at, item)| Entry {
                item: item.clone(),
                id: match (item, names.get(at)) {
                    (StackItem::Ability { .. }, Some(&Subject::Ability { id })) => Some(id),
                    _ => None,
                },
            })
            .collect();
        Table {
            objects,
            battlefield: view.battlefield.clone(),
            stack,
            you: Piles::of(&view.players.you),
            opponent: Piles::of(&view.players.opponent),
            turn: Turn {
                number: view.turn.number,
                active: view.turn.active,
                step: view.turn.step,
            },
        }
    }

    /// Whether two tables show the same game: their abilities' numbers aside, which a view
    /// does not have.
    pub fn shows(&self, other: &Table) -> bool {
        self.objects == other.objects
            && self.battlefield == other.battlefield
            && self.stack.len() == other.stack.len()
            && self
                .stack
                .iter()
                .zip(&other.stack)
                .all(|(a, b)| a.item == b.item)
            && self.you == other.you
            && self.opponent == other.opponent
            && self.turn == other.turn
    }

    pub fn piles(&self, player: Side) -> &Piles {
        match player {
            Side::You => &self.you,
            Side::Opponent => &self.opponent,
        }
    }

    pub fn piles_mut(&mut self, player: Side) -> &mut Piles {
        match player {
            Side::You => &mut self.you,
            Side::Opponent => &mut self.opponent,
        }
    }

    /// The order `zone` keeps its cards in, for the cards `owner` owns: none for a hand, exile
    /// and the stack, whose cards say where they are.
    pub fn order(&self, zone: ZoneName, owner: Side) -> Option<&Vec<ObjectId>> {
        match zone {
            ZoneName::Battlefield => Some(&self.battlefield),
            ZoneName::Library => Some(&self.piles(owner).library),
            ZoneName::Graveyard => Some(&self.piles(owner).graveyard),
            _ => None,
        }
    }

    fn order_mut(&mut self, zone: ZoneName, owner: Side) -> Option<&mut Vec<ObjectId>> {
        match zone {
            ZoneName::Battlefield => Some(&mut self.battlefield),
            ZoneName::Library => Some(&mut self.piles_mut(owner).library),
            ZoneName::Graveyard => Some(&mut self.piles_mut(owner).graveyard),
            _ => None,
        }
    }

    /// Does what `act` says. An act that names what the table does not have, or leaves out
    /// what the table needs, is an error that says so: the script is wrong.
    pub fn apply(&mut self, act: &Act) -> Result<(), String> {
        match act {
            Act::Fly {
                object, at, item, ..
            }
            | Act::Appear {
                object, at, item, ..
            } => self.place(object, *at, item.as_ref()),
            Act::Vanish {
                subject,
                object,
                at,
                ..
            } => match (*subject, object) {
                (Subject::Object { slot }, Some(object)) if slot == object.slot => {
                    self.place(object, *at, None)
                }
                (Subject::Object { slot }, None) => self.remove(slot),
                (Subject::Ability { id }, None) => {
                    let at = self
                        .stack
                        .iter()
                        .position(|e| e.id == Some(id))
                        .ok_or_else(|| format!("no ability a{id} is on the stack"))?;
                    self.stack.remove(at);
                    Ok(())
                }
                (subject, _) => Err(format!("{subject} cannot vanish so")),
            },
            Act::Emerge { id, item } => {
                self.stack.push(Entry {
                    item: item.clone(),
                    id: Some(*id),
                });
                Ok(())
            }
            Act::Flip { object }
            | Act::Tap { object }
            | Act::Showcase { object }
            | Act::Count {
                tally: Tally::Counters { object, .. } | Tally::Stats { object },
            }
            | Act::Strike {
                object: Some(object),
                ..
            }
            | Act::Streak {
                object: Some(object),
                ..
            } => self.place(object, None, None),
            Act::Count {
                tally: Tally::Life { player, life },
            } => {
                self.piles_mut(*player).life = *life;
                Ok(())
            }
            Act::Count {
                tally: Tally::Pool { player, mana },
            } => {
                self.piles_mut(*player).mana = *mana;
                Ok(())
            }
            Act::Set { fact } => self.set(fact),
            Act::Strike { object: None, .. }
            | Act::Streak { object: None, .. }
            | Act::Dwell { .. }
            | Act::Announce { .. }
            | Act::Point { .. }
            | Act::Mark { .. }
            | Act::Lift { .. } => Ok(()),
        }
    }

    /// A part of the table is as `fact` says.
    fn set(&mut self, fact: &Fact) -> Result<(), String> {
        match fact {
            &Fact::Turn {
                number,
                active,
                step,
            } => {
                self.turn = Turn {
                    number,
                    active,
                    step,
                };
            }
            Fact::Object { object } => self.place(object, None, None)?,
            Fact::Library { player, order } => {
                self.piles_mut(*player).library.clone_from(order);
            }
            Fact::Item { subject, item } => {
                let entry = self
                    .stack
                    .iter_mut()
                    .find(|e| e.subject() == *subject)
                    .ok_or_else(|| format!("{subject} is not on the stack"))?;
                entry.item = item.clone();
            }
        }
        Ok(())
    }

    /// Puts `object`'s record in place of the one its slot had. In another zone than it was,
    /// or with a place given, it leaves its old zone's order and joins the new one's at `at`,
    /// or at the end; leaving the stack takes its spell, and arriving there brings `item`.
    fn place(
        &mut self,
        object: &ObjectView,
        at: Option<u16>,
        item: Option<&StackItem>,
    ) -> Result<(), String> {
        let slot = object.slot;
        let was = self.objects.get(&slot).map(|o| (o.zone, o.owner));
        let moved = was.is_none_or(|(zone, _)| zone != object.zone);
        let item = match item {
            _ if !moved || object.zone != ZoneName::Stack => None,
            Some(item) => Some(item),
            None => return Err(format!("#{slot} goes on the stack with no spell")),
        };
        if let Some(at) = at
            && let Some(order) = self.order(object.zone, object.owner)
        {
            let others = order.iter().filter(|&&s| s != slot).count();
            if usize::from(at) > others {
                return Err(format!("#{slot} at {at} of {others}"));
            }
        }
        if moved || at.is_some() {
            if let Some((zone, owner)) = was {
                self.leave(slot, zone, owner, moved);
            }
            if let Some(order) = self.order_mut(object.zone, object.owner) {
                order.insert(at.map_or(order.len(), usize::from), slot);
            }
        }
        if let Some(item) = item {
            self.stack.push(Entry {
                item: item.clone(),
                id: None,
            });
        }
        self.objects.insert(slot, object.clone());
        Ok(())
    }

    /// Takes `slot` out of `zone`'s order, and its spell off the stack if it leaves that.
    fn leave(&mut self, slot: ObjectId, zone: ZoneName, owner: Side, moved: bool) {
        if let Some(order) = self.order_mut(zone, owner) {
            order.retain(|&s| s != slot);
        }
        if moved && zone == ZoneName::Stack {
            self.stack
                .retain(|e| !matches!(e.item, StackItem::Spell { slot: s, .. } if s == slot));
        }
    }

    /// The object in `slot` is no more.
    fn remove(&mut self, slot: ObjectId) -> Result<(), String> {
        let object = self
            .objects
            .remove(&slot)
            .ok_or_else(|| format!("no object is in #{slot}"))?;
        self.leave(slot, object.zone, object.owner, true);
        Ok(())
    }
}
