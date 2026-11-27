//! What the Effects store implements once, whatever its entries are (the crate's contract,
//! "Stores"): the empty store, what ends its entries, whether it fits a layout and how it
//! converts to one, its invariants, and what a checkpoint would lose of it.
//!
//! Implements: none. Maintains effect indexes and converts the store between layouts.

use crate::bitboard::Bitboard;
use crate::fit::{Misfit, room};
use crate::ids::{CardId, SlotId};
use crate::invariants::{At, Violation, check};
use crate::layout::{Layout, Queue, Table};
use crate::object::ObjectRef;
use crate::relayout::{Relayout, card, queue, slot};
use crate::rule_changes::Rules;
use crate::state::{State, Zone};
use crate::store::{Duration, Ended, Store};
use crate::table::GameTable;

use super::sets::Sets;
use super::{Affects, ContinuousEffect, CostChange, EFFECTS, Effects, Kinds, Payload};

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

    /// Sources chosen for prevention shields. A shield distinguishes its chosen object from
    /// an otherwise interchangeable object, even when both have the same characteristics.
    #[inline]
    fn names(&self) -> Bitboard<L::Slot> {
        self.of(Kinds::PREVENTION | Kinds::EXTRA_TURN)
            .iter()
            .filter_map(|entry| {
                if let Payload::ExtraTurn(Some(trigger)) = entry.payload {
                    trigger.source
                } else {
                    entry.chosen_shield_source()
                }
            })
            .map(|source| source.slot)
            .collect()
    }

    fn forget(&mut self, slot: L::Slot) -> Ended {
        // The object is a member of no set from here
        // (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007): what the slot holds
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
        for e in self.entries.iter() {
            match e.payload {
                Payload::Copy(values) => values.fits::<M>()?,
                Payload::Layer(change) => change.fits::<M>()?,
                Payload::Move(_)
                | Payload::Rule(_)
                | Payload::Cast(_)
                | Payload::Cost(_)
                | Payload::Regeneration
                | Payload::Prevent(_)
                | Payload::Text(_)
                | Payload::Control(_)
                | Payload::ExtraTurn(_)
                | Payload::Mana(_) => {}
            }
        }
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
                        matches!(
                            e.payload,
                            Payload::Rule(_)
                                | Payload::Prevent(_)
                                | Payload::Control(_)
                                | Payload::ExtraTurn(_)
                        ),
                        "an effect on a player that is no rule change, shield, control or extra turn",
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
                Affects::Game => {
                    check(
                        matches!(e.payload, Payload::Mana(_)),
                        "a game-wide effect that is no mana change",
                        None,
                    )?;
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
        super::copy::check_copies(self, s)?;
        check(
            rules == self.rules,
            "the players' rules disagree with the effects",
            None,
        )
    }

    /// The first of: an effect on an object or a set that is no permission to cast, then
    /// a permission, then a rule change on a player. That order and these words are what
    /// saves hold for them: a permission is said by its player and card, not by what
    /// granted it, and a rule change by its player, not by the card it came from.
    fn lingering(&self, s: &State<L>, t: &GameTable) -> Option<String> {
        let name = |slot: L::Slot| t.def(s.card(slot)).name;
        let on_objects = self.entries.iter().find_map(|e| {
            let on = match (e.affected, e.payload) {
                // A save says a copy by its permanent's exceptions, and a permission by its
                // player and card.
                (Affects::Object { .. }, Payload::Cast(_) | Payload::Copy(_))
                | (Affects::Player(_) | Affects::Game, _) => {
                    return None;
                }
                (Affects::Object { slot, .. }, _) => name(slot).to_owned(),
                (Affects::Set(set), _) => {
                    format!("{} permanents", self.sets.members(set).count())
                }
            };
            Some(format!(
                "an effect of {} on {on} lasting {}",
                t.def(e.source).name,
                e.duration.words()
            ))
        });
        let permission = || {
            self.entries
                .iter()
                .find_map(|e| match (e.affected, e.payload) {
                    (Affects::Object { slot, .. }, Payload::Cast(terms)) => Some(format!(
                        "player {}'s permission to cast {}",
                        terms.player.get(),
                        name(slot)
                    )),
                    _ => None,
                })
        };
        let rule_change = || {
            self.entries.iter().find_map(|e| {
                let (player, rule) = e.player_rule()?;
                let lasting = match e.duration {
                    Duration::UntilYourNextTurn(theirs) if theirs == player => {
                        "until their next turn".to_owned()
                    }
                    other @ (Duration::EndOfTurn
                    | Duration::UntilYourNextTurn(_)
                    | Duration::Indefinite) => other.words(),
                };
                Some(format!(
                    "player {} {} {lasting}",
                    player.get(),
                    t.rule(rule).words()
                ))
            })
        };
        let extra_turn = || {
            let (_, player, _) = self.next_extra_turn()?;
            Some(format!("a pending extra turn for player {}", player.get()))
        };
        on_objects
            .or_else(permission)
            .or_else(rule_change)
            .or_else(extra_turn)
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
                Affects::Game => Affects::Game,
            },
            payload: self.payload.relayout(),
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

impl<L: Layout> State<L> {
    /// The generic mana the cost changes on the spell `object` add to its cost
    /// (see [CR 601.2f](https://yawgatog.com/resources/magic-rules/#R6012f): "plus all additional
    /// costs and cost increases"). With no cost change live on any object, one AND.
    #[inline]
    pub fn cost_increase(&self, object: ObjectRef<L>) -> u8 {
        let affected = Affects::object(object);
        self.effects
            .of(Kinds::COST)
            .iter()
            .filter(|e| e.affected == affected)
            .fold(0, |total, e| {
                if let Payload::Cost(change) = e.payload {
                    let CostChange::Increase(n) = change;
                    // Bounded by the `u8` generic cost field of `ManaCost`.
                    total.saturating_add(n)
                } else {
                    total
                }
            })
    }
}
