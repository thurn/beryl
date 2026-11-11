//! What the Effects store implements once, whatever its entries are (the crate's contract,
//! "Stores"): the empty store, what ends its entries, whether it fits a layout and how it
//! converts to one, its invariants, and what a checkpoint would lose of it.

use beryl_card::Rules;

use crate::bitboard::Bitboard;
use crate::fit::{Misfit, room};
use crate::ids::{CardId, SlotId};
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Queue, Table};
use crate::object::ObjectRef;
use crate::relayout::{Relayout, card, queue, slot};
use crate::state::{State, Zone};
use crate::store::{Duration, Ended, Store};
use crate::table::GameTable;

use super::sets::Sets;
use super::{Affects, ContinuousEffect, EFFECTS, Effects, Kinds, Payload};

impl<L: Layout> Store<L> for Effects<L> {
    fn empty() -> Self {
        Effects {
            entries: Default::default(),
            sets: Sets::empty(),
            kinds: Kinds::default(),
            rules: Table::filled(Rules::NONE),
        }
    }

    fn keys(&self) -> Bitboard<L::Slot> {
        let objects: Bitboard<L::Slot> = (self.entries.iter())
            .filter_map(|e| e.affected.as_object())
            .map(|r| r.slot)
            .collect();
        objects | self.sets.all()
    }

    fn forget(&mut self, slot: L::Slot) -> Ended {
        // The object is a member of no set from here
        // ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): what the slot holds
        // next is "a new object with no memory of, or relation to, its previous existence").
        // An effect whose set it was the last of ends below; one with other members goes on.
        let left = self.sets.leave(slot);
        // Every entry about a slot is about its current object (the store's invariant), so
        // the slot names what ended.
        let mut done = self.end(|e| e.affected.as_object().is_none_or(|r| r.slot != slot));
        // A set's effect is a layer change, which the object that left no longer has.
        done.any |= left;
        done.layer_input |= left;
        done
    }

    #[inline]
    fn expire(&mut self, ended: Duration) -> Ended {
        if self.entries.is_empty() {
            return Ended::default();
        }
        self.end(|e| e.duration != ended)
    }

    fn fits<M: Layout>(&self) -> Result<(), Misfit> {
        room(
            "the continuous effects",
            self.entries.len(),
            <M::Queue<ContinuousEffect<M>, EFFECTS> as Queue<_>>::CAPACITY,
        )?;
        self.sets.fits::<M>()
    }

    fn check(&self, s: &State<L>, t: &GameTable) -> Result<(), Violation> {
        let mut kinds = 0;
        let mut sets = 0;
        let mut rules: L::PerPlayer<Rules> = Table::filled(Rules::NONE);
        for e in self.entries.iter() {
            kinds |= e.payload.kind();
            if let Some((player, rule)) = e.player_rule() {
                rules[player.index()] = rules[player.index()].with(rule);
            }
            let at = match e.affected {
                Affects::Object { slot, incarnation } => {
                    let at = Some(At::Slot(slot.index()));
                    check(
                        s.is_current(ObjectRef { slot, incarnation }),
                        "an effect on an object that has ended",
                        at,
                    )?;
                    at
                }
                Affects::Player(player) => {
                    check(
                        matches!(e.payload, Payload::Rule(_)),
                        "an effect on a player that is no rule change",
                        Some(At::Player(player)),
                    )?;
                    None
                }
                Affects::Set(set) => {
                    let members = self.sets.members(set);
                    check(
                        !members.is_empty(),
                        "an effect on a set with no member left",
                        None,
                    )?;
                    check(
                        matches!(e.payload, Payload::Layer(_)),
                        "an effect on a set that is no layer change",
                        None,
                    )?;
                    let named = |other: &&ContinuousEffect<L>| other.affected == e.affected;
                    check(
                        self.entries.iter().filter(named).count() == 1,
                        "two effects on one locked-in set",
                        None,
                    )?;
                    // An object that left the battlefield left the set as it did.
                    let elsewhere = members
                        .iter()
                        .find(|&slot| s.object(slot).zone() != Zone::Battlefield);
                    check(
                        elsewhere.is_none(),
                        "a locked-in set with a member off the battlefield",
                        elsewhere.map(|slot| At::Slot(slot.index())),
                    )?;
                    sets += 1;
                    None
                }
            };
            check(e.source.index() < t.len(), "an effect of no card", at)?;
        }
        check(
            sets == self.sets.live() && self.sets.is_trimmed(),
            "a locked-in set of no effect",
            None,
        )?;
        check(
            kinds == self.kinds.0,
            "the kinds of effect disagree with the effects",
            None,
        )?;
        check(
            rules == self.rules,
            "the players' rules disagree with the effects",
            None,
        )
    }

    fn lingering(&self, s: &State<L>, t: &GameTable) -> Option<String> {
        // The first entry about an object that is no permission to cast: those, and then a
        // rule change on a player, are listed after these
        // ([`State::cast_permission_lingering`], [`State::rule_change_lingering`]), where
        // and as saves have always had them.
        let (e, on) = self
            .entries
            .iter()
            .filter(|e| !matches!(e.payload, Payload::Cast(_)))
            .find_map(|e| {
                let on = match e.affected {
                    Affects::Object { slot, .. } => t.def(s.card(slot)).name.to_owned(),
                    Affects::Set(set) => {
                        format!("{} permanents", self.sets.members(set).count())
                    }
                    Affects::Player(_) => return None,
                };
                Some((e, on))
            })?;
        Some(format!(
            "an effect of {} on {on} lasting {}",
            t.def(e.source).name,
            e.duration.words()
        ))
    }
}

impl<L: Layout> Relayout for ContinuousEffect<L> {
    type In<M: Layout> = ContinuousEffect<M>;

    fn relayout<M: Layout>(&self) -> ContinuousEffect<M> {
        ContinuousEffect {
            source: card(self.source),
            timestamp: self.timestamp,
            duration: self.duration,
            affected: match self.affected {
                Affects::Object {
                    slot: affected,
                    incarnation,
                } => Affects::Object {
                    slot: slot(affected),
                    incarnation,
                },
                Affects::Player(player) => Affects::Player(player),
                Affects::Set(set) => Affects::Set(set.relayout()),
            },
            payload: self.payload,
        }
    }
}

impl<L: Layout> Relayout for Effects<L> {
    type In<M: Layout> = Effects<M>;

    fn relayout<M: Layout>(&self) -> Effects<M> {
        Effects {
            entries: queue(self.entries.as_slice(), ContinuousEffect::relayout),
            sets: self.sets.relayout(),
            kinds: self.kinds,
            rules: Table::from_fn(|player| self.rules[player]),
        }
    }
}
