//! The table: the part of a view a script accounts for (the module's "The table and replay"),
//! and what each step does to it. The session's builder keeps one per seat and the stage keeps
//! the same one: a step means here what it means on screen.

use std::collections::BTreeMap;

use serde::Serialize;

use super::{Act, Fact, Subject, Tally};
use crate::protocol::ObjectId;
use crate::protocol::{
    ManaPool, ManualPiles, NamedCounter, ObjectView, PlayerState, PlayerView, SHARED_PILE_SIDE,
    Side, StackItem, Step as TurnStep, ZoneName,
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
    /// The library and graveyard both players share, in a shared-deck game
    /// ([`PlayerView::shared`]); its `library` and `graveyard` are the piles, and its other
    /// fields are unused. When it is there, [`Table::piles`], [`Table::piles_mut`] and
    /// [`Table::order`] return it for either side, and each player's own `library` and
    /// `graveyard` stay empty.
    pub shared: Option<Piles>,
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
    pub life: i32,
    pub mana: ManaPool,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub named_counters: Vec<NamedCounter>,
    /// Bottom first.
    pub library: Vec<ObjectId>,
    /// Bottom first.
    pub graveyard: Vec<ObjectId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    manual: Option<ManualPiles>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct Turn {
    pub number: u16,
    pub own_number: u16,
    pub active: Side,
    pub step: TurnStep,
}

impl Table {
    /// A table with nothing on it and each player at `life`, in the first main phase of
    /// your first turn: what the steps a client makes itself are timed against
    /// ([`super::own_timings`]), and what tests build on.
    pub(crate) fn empty(life: i32) -> Table {
        let piles = || Piles {
            life,
            mana: ManaPool::default(),
            named_counters: Vec::new(),
            library: Vec::new(),
            graveyard: Vec::new(),
            manual: None,
        };
        Table {
            objects: BTreeMap::new(),
            battlefield: Vec::new(),
            stack: Vec::new(),
            you: piles(),
            opponent: piles(),
            shared: None,
            turn: Turn {
                number: 1,
                own_number: 1,
                active: Side::You,
                step: TurnStep::PrecombatMain,
            },
        }
    }
}

impl Piles {
    fn of(player: &PlayerState, manual: Option<&ManualPiles>) -> Piles {
        Piles {
            life: player.life,
            mana: player.mana,
            named_counters: player.named_counters.clone(),
            library: player.library.clone(),
            graveyard: player.graveyard.clone(),
            manual: manual.cloned(),
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
    /// The table of `view`, with numbers for its manual markers. The stack is the view's with two
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
    /// viewer ([`super::Heard::stack`]). Manual markers carry their number in the view.
    /// An engine ability `names` has no number for stays unnumbered.
    pub fn named(view: &PlayerView, names: &[Subject]) -> Table {
        let objects: BTreeMap<ObjectId, ObjectView> =
            view.objects.iter().map(|o| (o.slot, o.clone())).collect();
        let stack = view
            .stack_with_resolving()
            .enumerate()
            .filter(|(_, item)| match item {
                StackItem::Spell { slot, .. } => {
                    objects.get(slot).is_some_and(|o| o.zone == ZoneName::Stack)
                }
                StackItem::Ability { .. } => true,
            })
            .map(|(at, item)| Entry {
                item: item.clone(),
                id: item.marker_id().or_else(|| match (item, names.get(at)) {
                    (StackItem::Ability { .. }, Some(&Subject::Ability { id })) => Some(id),
                    _ => None,
                }),
            })
            .collect();
        let manual = view.table.as_ref().map(|table| &table.orders);
        let shared = view.shared.as_ref().map(|shared| Piles {
            life: 0,
            mana: ManaPool::default(),
            named_counters: Vec::new(),
            library: shared.library.clone(),
            graveyard: shared.graveyard.clone(),
            manual: None,
        });
        Table {
            objects,
            battlefield: view.battlefield.clone(),
            stack,
            you: Piles::of(&view.players.you, manual.map(|orders| &orders.you)),
            opponent: Piles::of(
                &view.players.opponent,
                manual.map(|orders| &orders.opponent),
            ),
            shared,
            turn: Turn {
                number: view.turn.number,
                own_number: view.turn.own_number,
                active: view.turn.active,
                step: view.turn.step,
            },
        }
    }

    /// Whether two tables show the same game. Engine ability numbers are local to their
    /// scripts. Manual marker numbers are part of their stack items.
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
            && self.shared == other.shared
            && self.turn == other.turn
    }

    /// The piles of `player`: the shared ones for either side in a shared-deck game, whose
    /// life and mana are not the player's (read them through [`Table::player`]).
    pub fn piles(&self, player: Side) -> &Piles {
        self.shared.as_ref().unwrap_or_else(|| self.player(player))
    }

    /// Whether `zone` is the shared library or graveyard of a shared-deck game.
    pub fn shares(&self, zone: ZoneName) -> bool {
        self.shared.is_some() && matches!(zone, ZoneName::Library | ZoneName::Graveyard)
    }

    /// The owner and controller a record of a card in `zone` names, where the card's owner is
    /// `owner`: [`SHARED_PILE_SIDE`] for a card in a shared library or graveyard, which no
    /// player owns, and `owner` otherwise.
    pub fn holder(&self, zone: ZoneName, owner: Side) -> Side {
        if self.shares(zone) {
            SHARED_PILE_SIDE
        } else {
            owner
        }
    }

    /// [`Table::piles`], to change.
    pub fn piles_mut(&mut self, player: Side) -> &mut Piles {
        let Table {
            shared,
            you,
            opponent,
            ..
        } = self;
        match (shared, player) {
            (Some(shared), _) => shared,
            (None, Side::You) => you,
            (None, Side::Opponent) => opponent,
        }
    }

    /// `player`'s own numbers (life, mana, counters), and their own piles in a game with no
    /// shared ones.
    pub fn player(&self, player: Side) -> &Piles {
        match player {
            Side::You => &self.you,
            Side::Opponent => &self.opponent,
        }
    }

    /// [`Table::player`], to change.
    pub fn player_mut(&mut self, player: Side) -> &mut Piles {
        match player {
            Side::You => &mut self.you,
            Side::Opponent => &mut self.opponent,
        }
    }

    /// The order of `zone` on `side`. Hand, exile and sideboard have an explicit
    /// order on manual tables. The stack has its own list of spells and abilities.
    pub fn order(&self, zone: ZoneName, side: Side) -> Option<&Vec<ObjectId>> {
        match zone {
            ZoneName::Battlefield => Some(&self.battlefield),
            ZoneName::Library => Some(&self.piles(side).library),
            ZoneName::Graveyard => Some(&self.piles(side).graveyard),
            _ => self.player(side).manual.as_ref()?.order(zone),
        }
    }

    fn order_mut(&mut self, zone: ZoneName, side: Side) -> Option<&mut Vec<ObjectId>> {
        match zone {
            ZoneName::Battlefield => Some(&mut self.battlefield),
            ZoneName::Library => Some(&mut self.piles_mut(side).library),
            ZoneName::Graveyard => Some(&mut self.piles_mut(side).graveyard),
            _ => self.player_mut(side).manual.as_mut()?.order_mut(zone),
        }
    }

    /// Does what `act` says. An act that names what the table does not have, or leaves out
    /// what the table needs, is an error that says so: the script is wrong.
    pub fn apply(&mut self, act: &Act) -> Result<(), String> {
        self.apply_noting(act, &mut |_| {})
    }

    /// [`Table::apply`], calling `note` with every slot whose object the act replaces or removes
    /// or whose place in a library's order it changes. An object it does not name keeps its
    /// record, though its place in an order may shift as another object joins or leaves it.
    /// A client that works out where objects rest after each step re-places only the parts of
    /// the table that hold the noted objects.
    pub fn apply_noting(
        &mut self,
        act: &Act,
        note: &mut dyn FnMut(ObjectId),
    ) -> Result<(), String> {
        match act {
            Act::Fly {
                object, at, item, ..
            }
            | Act::Appear {
                object, at, item, ..
            } => self.place(object, *at, item.as_ref(), note),
            Act::Vanish {
                subject,
                object,
                at,
                ..
            } => match (*subject, object) {
                (Subject::Object { slot }, Some(object)) if slot == object.slot => {
                    self.place(object, *at, None, note)
                }
                (Subject::Object { slot }, None) => self.remove(slot, note),
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
            Act::Emerge { id, item, at } => {
                if let Some(marker) = item.marker_id()
                    && marker != *id
                {
                    return Err(format!("emerge names marker a{marker} as a{id}"));
                }
                self.place_stack(item, Some(*id), *at)?;
                Ok(())
            }
            Act::Flip { object }
            | Act::Tap { object }
            | Act::Showcase { object }
            | Act::Count {
                tally:
                    Tally::Counters { object, .. }
                    | Tally::NamedCounters { object, .. }
                    | Tally::Stats { object }
                    | Tally::Damage { object },
            }
            | Act::Strike {
                object: Some(object),
                ..
            }
            | Act::Streak {
                object: Some(object),
                ..
            } => self.place(object, None, None, note),
            Act::Count {
                tally: Tally::Life { player, life },
            } => {
                self.player_mut(*player).life = *life;
                Ok(())
            }
            Act::Count {
                tally: Tally::Pool { player, mana },
            } => {
                self.player_mut(*player).mana = *mana;
                Ok(())
            }
            Act::Count {
                tally: Tally::PlayerCounters { player, counters },
            } => {
                self.player_mut(*player).named_counters.clone_from(counters);
                Ok(())
            }
            Act::Set { fact } => self.set(fact, note),
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
    fn set(&mut self, fact: &Fact, note: &mut dyn FnMut(ObjectId)) -> Result<(), String> {
        match fact {
            &Fact::Turn {
                number,
                own_number,
                active,
                step,
            } => {
                self.turn = Turn {
                    number,
                    own_number,
                    active,
                    step,
                };
            }
            Fact::Object { object } => self.place(object, None, None, note)?,
            Fact::Library { player, order } => {
                let library = &mut self.piles_mut(*player).library;
                library.iter().chain(order).for_each(|&slot| note(slot));
                library.clone_from(order);
            }
            Fact::Forget { slots } => {
                for &slot in slots {
                    self.remove(slot, note)?;
                }
            }
            Fact::ShuffledLibrary { player, cards } => {
                let old = std::mem::take(&mut self.piles_mut(*player).library);
                for slot in old {
                    self.objects.remove(&slot);
                    note(slot);
                }
                for object in cards {
                    self.place(object, None, None, note)?;
                }
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

    /// Replaces the record in `object`'s slot. A card that changes zones, changes piles
    /// within a zone, or has an explicit index leaves its old order and joins the new one at
    /// `at` or at the end. Leaving the stack takes its spell and arriving brings `item`.
    fn place(
        &mut self,
        object: &ObjectView,
        at: Option<u16>,
        item: Option<&StackItem>,
        note: &mut dyn FnMut(ObjectId),
    ) -> Result<(), String> {
        let slot = object.slot;
        let side = object.zone_side.unwrap_or(object.owner);
        let was = self
            .objects
            .get(&slot)
            .map(|o| (o.zone, o.zone_side.unwrap_or(o.owner)));
        let moved = was.is_none_or(|(zone, _)| zone != object.zone);
        let changed_pile = was.is_some_and(|(zone, old_side)| {
            zone == object.zone
                && old_side != side
                && !matches!(zone, ZoneName::Battlefield | ZoneName::Stack)
        });
        let item = match item {
            _ if (!moved && at.is_none()) || object.zone != ZoneName::Stack => None,
            Some(item) => Some(item),
            None => return Err(format!("#{slot} goes on the stack with no spell")),
        };
        if let Some(at) = at
            && let Some(order) = self.order(object.zone, side)
        {
            // An order holds each slot at most once.
            let others = order.len() - usize::from(order.contains(&slot));
            if usize::from(at) > others {
                return Err(format!("#{slot} at {at} of {others}"));
            }
        }
        if moved || changed_pile || at.is_some() {
            if let Some((zone, side)) = was {
                self.leave(slot, zone, side, moved || item.is_some());
            }
            if let Some(order) = self.order_mut(object.zone, side) {
                order.insert(at.map_or(order.len(), usize::from), slot);
            }
        }
        if let Some(item) = item {
            self.place_stack(item, None, at)?;
        }
        self.objects.insert(slot, object.clone());
        note(slot);
        Ok(())
    }

    fn place_stack(
        &mut self,
        item: &StackItem,
        id: Option<u32>,
        at: Option<u16>,
    ) -> Result<(), String> {
        let index = at.map_or(self.stack.len(), usize::from);
        if index > self.stack.len() {
            return Err(format!("stack entry at {index} of {}", self.stack.len()));
        }
        self.stack.insert(
            index,
            Entry {
                item: item.clone(),
                id,
            },
        );
        Ok(())
    }

    /// Takes `slot` out of `zone`'s order, and its spell off the stack if it leaves that.
    fn leave(&mut self, slot: ObjectId, zone: ZoneName, side: Side, moved: bool) {
        // An order holds each slot at most once, so finding it is enough: `retain` would go on
        // to visit every later slot.
        if let Some(order) = self.order_mut(zone, side)
            && let Some(at) = order.iter().position(|&s| s == slot)
        {
            order.remove(at);
            debug_assert!(!order.contains(&slot), "#{slot} is twice in one order");
        }
        if moved && zone == ZoneName::Stack {
            self.stack
                .retain(|e| !matches!(e.item, StackItem::Spell { slot: s, .. } if s == slot));
        }
    }

    /// The object in `slot` is no more.
    fn remove(&mut self, slot: ObjectId, note: &mut dyn FnMut(ObjectId)) -> Result<(), String> {
        note(slot);
        let object = self
            .objects
            .remove(&slot)
            .ok_or_else(|| format!("no object is in #{slot}"))?;
        self.leave(
            slot,
            object.zone,
            object.zone_side.unwrap_or(object.owner),
            true,
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Why;

    fn appear(object: ObjectView) -> Act {
        Act::Appear {
            from: ZoneName::Nowhere,
            why: Why::Created,
            object,
            at: None,
            item: None,
        }
    }

    /// In a shared-deck game a card of either side goes into the one shared library or
    /// graveyard, moves between them, and leaves, and no player's own pile ever holds it.
    #[test]
    fn a_shared_table_places_and_removes_a_card_for_either_side() {
        for side in [Side::You, Side::Opponent] {
            let mut table = Table::empty(20);
            table.shared = Some(table.you.clone());
            let slot = ObjectId(7);
            let card = |zone| ObjectView::card(slot, "Island", zone, side, side);
            let shared = |table: &Table| table.shared.clone().unwrap();

            table.apply(&appear(card(ZoneName::Library))).unwrap();
            assert_eq!(shared(&table).library, [slot], "{side:?}");
            assert_eq!(table.order(ZoneName::Library, Side::You), Some(&vec![slot]));
            assert_eq!(
                table.order(ZoneName::Library, Side::Opponent),
                Some(&vec![slot])
            );

            table.apply(&appear(card(ZoneName::Graveyard))).unwrap();
            assert!(shared(&table).library.is_empty(), "{side:?}");
            assert_eq!(shared(&table).graveyard, [slot], "{side:?}");

            table
                .apply(&Act::Set {
                    fact: Fact::Forget { slots: vec![slot] },
                })
                .unwrap();
            assert!(shared(&table).graveyard.is_empty(), "{side:?}");
            assert!(table.objects.is_empty());
            for own in [&table.you, &table.opponent] {
                assert!(own.library.is_empty() && own.graveyard.is_empty());
            }
        }
    }

    /// A player's life stays theirs when the piles are shared.
    #[test]
    fn a_shared_table_keeps_each_players_life() {
        let mut table = Table::empty(20);
        table.shared = Some(table.you.clone());
        table
            .apply(&Act::Count {
                tally: Tally::Life {
                    player: Side::Opponent,
                    life: 17,
                },
            })
            .unwrap();
        assert_eq!((table.you.life, table.opponent.life), (20, 17));
    }
}
