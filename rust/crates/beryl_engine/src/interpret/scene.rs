//! Reading effect data's expressions against a game: which objects, which players, how many
//! and whether.
//!
//! A [`Scene`] is what an expression is read against: the game, the spell or ability whose
//! effect it is, and what that resolution has done so far ([`Registers`]). Reading changes
//! nothing, so the same reader serves an ability as it triggers, where there is no
//! resolution yet, and as it resolves.

use beryl_card::{Amount, CastingKind, Comparison, Condition, Objects, Players};

use crate::bitboard::Bitboard;
use crate::card::Castings;
use crate::cast;
use crate::effect::entered_with;
use crate::filter::{self, Evaluate, Filter};
use crate::ids::Seat;
use crate::layout::{Conditions, Layout};
use crate::state::{Counters, ObjectFlags, PendingTrigger, State, TARGETS, Target, Zone};
use crate::table::GameTable;

/// A set of players.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct Who(u8);

impl Who {
    pub(super) const NOBODY: Who = Who(0);
    pub(super) const EVERYONE: Who = Who(0b11);

    pub(super) fn only(player: Seat) -> Who {
        Who(1 << player.get())
    }

    /// [`Players::EachOpponent`]: every opponent of `you`
    /// ([CR 102.2](https://yawgatog.com/resources/magic-rules/#R1022)): "In a two-player
    /// game, a player's opponent is the other player."
    ///
    /// The one place that entry meets the two-player game. With more players, "the player's
    /// opponents are all players not on their team"
    /// ([CR 102.3](https://yawgatog.com/resources/magic-rules/#R1023)): this function
    /// changes then, and no card does.
    pub(super) fn each_opponent(you: Seat) -> Who {
        Who::only(you.opponent())
    }

    pub(super) fn insert(&mut self, player: Seat) {
        self.0 |= 1 << player.get();
    }

    pub(super) fn contains(self, player: Seat) -> bool {
        self.0 & (1 << player.get()) != 0
    }

    /// The players in the order they act when several do
    /// ([CR 101.4](https://yawgatog.com/resources/magic-rules/#R1014)): "If multiple players
    /// would make choices and/or take actions at the same time, the active player makes any
    /// choices required, then the next player in turn order".
    pub(super) fn in_turn_order(self, active: Seat) -> impl Iterator<Item = Seat> {
        [active, active.opponent()]
            .into_iter()
            .filter(move |&player| self.contains(player))
    }
}

/// What the instruction before did (`beryl_card`'s `effect` module, "What the instruction
/// before did"): the objects it acted on, as they are now, how many or how much it did, and
/// whether it did anything.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Outcome<L: Layout> {
    pub(super) objects: Bitboard<L::Slot>,
    pub(super) number: u8,
    pub(super) did: bool,
}

impl<L: Layout> Outcome<L> {
    /// The outcome of an instruction that did nothing.
    pub(super) const NOTHING: Outcome<L> = Outcome {
        objects: Bitboard::EMPTY,
        number: 0,
        did: false,
    };

    /// The outcome of an instruction that acted on `objects`.
    pub(super) fn acted_on(objects: Bitboard<L::Slot>) -> Outcome<L> {
        Outcome::of(objects, objects.count())
    }

    /// The outcome of an instruction that did `number` of something to `objects`.
    pub(super) fn of(objects: Bitboard<L::Slot>, number: u32) -> Outcome<L> {
        Outcome {
            objects,
            number: u8::try_from(number).unwrap_or(u8::MAX),
            did: number > 0,
        }
    }

    /// This outcome and `other`'s together, as those of each player an instruction was done
    /// for.
    pub(super) fn with(self, other: Outcome<L>) -> Outcome<L> {
        Outcome {
            objects: self.objects | other.objects,
            number: self.number.saturating_add(other.number),
            did: self.did || other.did,
        }
    }
}

/// What the effect still knows of a target it has moved
/// ([CR 608.2h](https://yawgatog.com/resources/magic-rules/#R6082h)): "if it's no longer in
/// that zone, or if the effect has moved it from a public zone to a hidden zone, the effect
/// uses the object's last known information."
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Known {
    pub(super) controller: Seat,
    pub(super) power: i8,
    pub(super) toughness: i8,
    pub(super) mana_value: u8,
    pub(super) counters: Counters,
}

/// What a resolution keeps between its instructions.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Registers<L: Layout> {
    /// The targets that were legal as the resolution began, a bit for each by its place in
    /// the entry's list
    /// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If a spell or
    /// ability specifies targets, it checks whether the targets are still legal" as it
    /// begins to resolve, and "Illegal targets, if any, won't be affected by parts of a
    /// resolving spell's effect for which they're illegal."
    pub(super) legal: u8,
    /// The targets an instruction set out to move, a bit for each, and what was known of
    /// each as it did. One that is no longer where it was has moved; one the instruction
    /// could not move is read where it is.
    pub(super) noted: u8,
    pub(super) known: [Known; TARGETS],
    pub(super) outcome: Outcome<L>,
    /// The player an [`beryl_card::Effect::ForEachPlayer`] is being done for.
    pub(super) that_player: Option<Seat>,
}

/// What an expression is read against.
pub(super) struct Scene<'s, L: Layout> {
    pub(super) s: &'s State<L>,
    pub(super) t: &'s GameTable,
    /// The ability, as the trigger it would be: its source, card, controller and what it
    /// triggered on. A spell's is its card.
    pub(super) subject: &'s PendingTrigger<L>,
    /// A spell's casting options paid; `None` for an ability.
    pub(super) costs: Option<Castings>,
    /// The announced value of X.
    pub(super) x: u8,
    pub(super) targets: &'s [Target<L>],
    pub(super) registers: &'s Registers<L>,
}

impl<L: Layout> Scene<'_, L> {
    fn you(&self) -> Seat {
        self.subject.controller
    }

    /// The targets chosen for the `spec`th instance of the word "target" that were legal as
    /// the resolution began, with their places in the entry's list.
    fn targets(&self, spec: u8) -> impl Iterator<Item = (usize, Target<L>)> + '_ {
        self.targets
            .iter()
            .copied()
            .enumerate()
            .filter(move |&(i, target)| target.spec == spec && self.registers.legal & (1 << i) != 0)
    }

    /// The targets among [`Scene::targets`] the effect has moved, as they were last known.
    /// One an instruction set out to move and could not, as when what would have exiled it
    /// had left first (CR 610.3b), is still what [`Scene::objects`] names, and is not here.
    fn moved(&self, objects: Objects) -> impl Iterator<Item = &Known> + '_ {
        let spec = match objects {
            Objects::Target(spec) => Some(spec),
            Objects::This | Objects::Those | Objects::All { .. } => None,
        };
        spec.into_iter()
            .flat_map(|spec| self.targets(spec))
            .filter(|&(i, target)| {
                self.registers.noted & (1 << i) != 0
                    && target
                        .as_object()
                        .is_some_and(|object| !self.s.is_current(object))
            })
            .map(|(i, _)| &self.registers.known[i])
    }

    /// The objects `objects` names that are where they were: one that has changed zones is
    /// a new object ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which
    /// only [`Objects::Those`] follows.
    pub(super) fn objects(&self, objects: Objects) -> Bitboard<L::Slot> {
        let s = self.s;
        match objects {
            Objects::This => self
                .subject
                .source
                .filter(|&source| s.is_current(source))
                .map_or(Bitboard::EMPTY, |source| Bitboard::single(source.slot)),
            Objects::Target(spec) => self
                .targets(spec)
                .filter_map(|(_, target)| target.as_object())
                .filter(|&object| s.is_current(object))
                .map(|object| object.slot)
                .collect(),
            Objects::Those => self.registers.outcome.objects,
            Objects::All { zone, filter } => {
                let within = match zone {
                    Zone::Battlefield => s.battlefield().iter().copied().collect(),
                    Zone::Stack => cast::spells(s),
                    _ => cast::in_zone(s, zone),
                };
                filter.select_x(s, self.t, within, self.you(), self.x)
            }
        }
    }

    /// Who controls the object in `slot` now: a permanent's or a spell's controller, and
    /// anywhere else its owner
    /// ([CR 108.4a](https://yawgatog.com/resources/magic-rules/#R1084a)): "If anything asks
    /// for the controller of a card that doesn't have one (because it's not a permanent or
    /// spell), use its owner instead."
    fn controller(&self, slot: L::Slot) -> Seat {
        let s = self.s;
        match s.object(slot).zone {
            Zone::Battlefield => s.controller(slot),
            Zone::Stack => s
                .stack()
                .iter()
                .find(|entry| entry.spell().is_some_and(|spell| spell.slot == slot))
                .map_or(s.object(slot).owner, |entry| entry.controller),
            _ => s.object(slot).owner,
        }
    }

    /// The players `players` names.
    pub(super) fn players(&self, players: Players) -> Who {
        match players {
            // CR 109.5: "The words 'you' and 'your' on an object refer to the object's
            // controller".
            Players::You => Who::only(self.you()),
            Players::EachOpponent => Who::each_opponent(self.you()),
            Players::EachPlayer => Who::EVERYONE,
            Players::Target(spec) => {
                let mut who = Who::NOBODY;
                for player in self.targets(spec).filter_map(|(_, t)| t.as_player()) {
                    who.insert(player);
                }
                who
            }
            // A target the effect moved is known as it last was. What `Those` and `All`
            // name are read where they are now, so their owners once they have left.
            Players::ControllerOf(objects) => {
                let mut who = Who::NOBODY;
                for slot in self.objects(objects).iter() {
                    who.insert(self.controller(slot));
                }
                for known in self.moved(objects) {
                    who.insert(known.controller);
                }
                who
            }
            // CR 108.3: "The owner of a card in the game is the player who started the game
            // with it in their deck." A slot keeps its owner through every move.
            Players::OwnerOf(objects) => {
                let mut who = Who::NOBODY;
                for slot in self.objects(objects).iter() {
                    who.insert(self.s.object(slot).owner);
                }
                if let Objects::Target(spec) = objects {
                    for object in self.targets(spec).filter_map(|(_, t)| t.as_object()) {
                        who.insert(self.s.object(object.slot).owner);
                    }
                }
                who
            }
            Players::ThatPlayer => {
                debug_assert!(
                    self.registers.that_player.is_some(),
                    "\"that player\" outside \"each player\""
                );
                self.registers.that_player.map_or(Who::NOBODY, Who::only)
            }
        }
    }

    /// What is known of the object in `slot` as it is now, for an effect about to move it.
    pub(super) fn known(&self, slot: L::Slot) -> Known {
        Known {
            controller: self.controller(slot),
            power: self.power(slot),
            toughness: self.toughness(slot),
            mana_value: self.mana_value(slot),
            counters: self.s.object(slot).counters,
        }
    }

    /// A permanent's power after continuous effects, and elsewhere what its card says
    /// ([CR 208.1](https://yawgatog.com/resources/magic-rules/#R2081)): "A creature card has
    /// two numbers separated by a slash printed in its lower right corner. The first number
    /// is its power".
    fn power(&self, slot: L::Slot) -> i8 {
        let o = self.s.object(slot);
        if o.zone == Zone::Battlefield {
            self.s.derived(slot).power
        } else {
            self.t.def(o.card).face(o.face).power.unwrap_or(0)
        }
    }

    /// [`Scene::power`] for toughness (CR 208.1): "the second is its toughness".
    fn toughness(&self, slot: L::Slot) -> i8 {
        let o = self.s.object(slot);
        if o.zone == Zone::Battlefield {
            self.s.derived(slot).toughness
        } else {
            self.t.def(o.card).face(o.face).toughness.unwrap_or(0)
        }
    }

    /// An object's mana value, a spell's with its X
    /// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as
    /// the number chosen for it while the object is on the stack."
    fn mana_value(&self, slot: L::Slot) -> u8 {
        let s = self.s;
        if s.object(slot).zone != Zone::Stack {
            return filter::mana_value(s, self.t, slot);
        }
        let spell = s
            .stack()
            .iter()
            .find(|entry| entry.spell().is_some_and(|spell| spell.slot == slot));
        match spell {
            Some(entry) => {
                let cost = self.t.def(entry.card).face(entry.face).cost;
                cost.mana_value()
                    .saturating_add(cost.x.saturating_mul(entry.x))
            }
            None => filter::mana_value(s, self.t, slot),
        }
    }

    /// The total of `of` over the objects `objects` names, the targets the effect has moved
    /// among them (CR 608.2h).
    fn total(
        &self,
        objects: Objects,
        of: impl Fn(&Self, L::Slot) -> i32,
        known: impl Fn(&Known) -> i32,
    ) -> i32 {
        let here: i32 = self
            .objects(objects)
            .iter()
            .map(|slot| of(self, slot))
            .sum();
        here + self.moved(objects).map(known).sum::<i32>()
    }

    /// The value of `amount` before it is used, which may be below zero.
    fn value(&self, amount: Amount) -> i32 {
        match amount {
            Amount::Fixed(n) => i32::from(n),
            Amount::X => i32::from(self.x),
            Amount::ThatMany => i32::from(self.registers.outcome.number),
            Amount::Count(objects) => self.objects(*objects).count().cast_signed(),
            Amount::Times(n, amount) => i32::from(n) * self.value(*amount),
            Amount::Power(objects) => self.total(
                objects,
                |scene, slot| i32::from(scene.power(slot)),
                |known| i32::from(known.power),
            ),
            Amount::Toughness(objects) => self.total(
                objects,
                |scene, slot| i32::from(scene.toughness(slot)),
                |known| i32::from(known.toughness),
            ),
            Amount::ManaValue(objects) => self.total(
                objects,
                |scene, slot| i32::from(scene.mana_value(slot)),
                |known| i32::from(known.mana_value),
            ),
            Amount::Counters(objects, kind) => self.total(
                objects,
                |scene, slot| i32::from(scene.s.object(slot).counters.get(kind)),
                |known| i32::from(known.counters.get(kind)),
            ),
        }
    }

    /// The value of `amount` as its instruction is reached (CR 608.2h), never less than zero
    /// ([CR 107.1b](https://yawgatog.com/resources/magic-rules/#R1071b)): "If a calculation
    /// that would determine the result of an effect yields a negative number, zero is used
    /// instead".
    pub(super) fn amount(&self, amount: Amount) -> u8 {
        u8::try_from(self.value(amount).max(0)).unwrap_or(u8::MAX)
    }

    /// Whether `condition` holds.
    pub(super) fn holds(&self, condition: &Condition) -> bool {
        match *condition {
            Condition::Did => self.registers.outcome.did,
            Condition::Paid(kind) => self.paid(kind),
            Condition::Compare(left, comparison, right) => {
                let (left, right) = (self.amount(left), self.amount(right));
                match comparison {
                    Comparison::AtLeast => left >= right,
                    Comparison::AtMost => left <= right,
                    Comparison::Equal => left == right,
                    Comparison::MoreThan => left > right,
                    Comparison::LessThan => left < right,
                }
            }
            Condition::Is(names, filter) => {
                // What a target was is not kept once the effect has moved it (`Known` holds
                // no characteristics), so "if it was a [filter]" cannot be asked of one yet.
                debug_assert!(
                    self.moved(names).next().is_none(),
                    "the last known characteristics of a target the effect moved"
                );
                let objects = self.objects(names);
                !objects.is_empty() && self.passing(objects, filter) == objects
            }
            Condition::Not(condition) => !self.holds(condition),
            Condition::All(conditions) => conditions.iter().all(|c| self.holds(c)),
            Condition::Any(conditions) => conditions.iter().any(|c| self.holds(c)),
            Condition::Custom(custom) => {
                let conditions = custom
                    .get::<Conditions>()
                    .expect("a condition made by conditions!");
                L::condition(conditions)(self.s, self.t, self.subject)
            }
        }
    }

    fn passing(&self, objects: Bitboard<L::Slot>, filter: Filter) -> Bitboard<L::Slot> {
        filter.select_x(self.s, self.t, objects, self.you(), self.x)
    }

    /// Whether the spell was cast with this kind of cost, or the permanent's spell was
    /// ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that spell
    /// has been 'kicked.'" A permanent carries a mark for each kind it can enter with
    /// ([`entered_with`]); none is overloaded, as only an instant or sorcery has overload
    /// ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)).
    fn paid(&self, kind: CastingKind) -> bool {
        if let Some(costs) = self.costs {
            let face = self.t.def(self.subject.card).face(self.subject.face);
            return costs.paid(face).any(|option| option.kind() == kind);
        }
        let flag = match kind {
            CastingKind::Kicker => ObjectFlags::KICKED,
            CastingKind::Offspring => ObjectFlags::OFFSPRING,
            CastingKind::Impending => ObjectFlags::IMPENDING,
            CastingKind::Evoke => ObjectFlags::EVOKED,
            CastingKind::Overload => return false,
        };
        entered_with(self.s, self.subject, flag)
    }
}
