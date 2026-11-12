//! Reading effect data's expressions against a game: which objects, which players, how many
//! and whether.
//!
//! A [`Scene`] is what an expression is read against: the game, the spell or ability whose
//! effect it is, and what that resolution has done so far ([`Registers`]). Reading changes
//! nothing, so the same reader serves an ability as it triggers, where there is no
//! resolution yet, and as it resolves.

use beryl_card::{Amount, CastingKind, Condition, Objects, Players, SpecIx};

use crate::PlayerSet;
use crate::bitboard::Bitboard;
use crate::card::Castings;
use crate::cast;
use crate::filter::{Evaluate, Filter};
use crate::ids::Seat;
use crate::layout::{Layout, PerPlayer, Table};
use crate::query::Copiable;
use crate::state::{
    CountersHeld, ObjectFlags, PendingTrigger, State, TARGETS, Target, TargetPlace, TargetPlaces,
    Tracker, Zone, ZoneKey, placed,
};
use crate::table::GameTable;
use crate::trigger::entered_with;

/// What the instruction before did (`beryl_card`'s `effect` module, "What the instruction
/// before did"): the objects it acted on, as they are now, how many or how much it did, and
/// whether it did anything.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Outcome<L: Layout> {
    pub(crate) objects: Bitboard<L::Slot>,
    pub(crate) number: u8,
    pub(crate) did: bool,
}

impl<L: Layout> Outcome<L> {
    /// The outcome of an instruction that did nothing.
    pub(crate) const NOTHING: Outcome<L> = Outcome {
        objects: Bitboard::EMPTY,
        number: 0,
        did: false,
    };

    /// The outcome of an instruction that acted on `objects`.
    pub(crate) fn acted_on(objects: Bitboard<L::Slot>) -> Outcome<L> {
        Outcome::of(objects, objects.count())
    }

    /// The outcome of an instruction that did `number` of something to `objects`.
    pub(crate) fn of(objects: Bitboard<L::Slot>, number: u32) -> Outcome<L> {
        Outcome {
            objects,
            number: u8::try_from(number).unwrap_or(u8::MAX),
            did: number > 0,
        }
    }

    /// This outcome and `other`'s together, as those of each player an instruction was done
    /// for.
    pub(crate) fn with(self, other: Outcome<L>) -> Outcome<L> {
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
pub(crate) struct Known<L: Layout> {
    pub(crate) controller: Seat,
    /// Its copiable values ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)),
    /// which a copy of it made after it left has.
    pub(crate) copiable: Option<Copiable<L>>,
    pub(crate) power: i8,
    pub(crate) toughness: i8,
    pub(crate) mana_value: u8,
    pub(crate) counters: CountersHeld,
}

/// What a resolution keeps between its instructions.
#[derive(Clone, Debug)]
pub(crate) struct Registers<L: Layout> {
    /// The targets that were legal as the resolution began, by their places in the entry's
    /// list
    /// ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If the spell or
    /// ability specifies targets, it checks whether the targets are still legal" as it
    /// begins to resolve, and "Illegal targets, if any, won't be affected by parts of a
    /// resolving spell's effect for which they're illegal."
    pub(crate) legal: TargetPlaces,
    /// The targets an instruction set out to move, and what was known of each, by its
    /// place, as it did. One that is no longer where it was has moved; one the instruction
    /// could not move is read where it is.
    pub(crate) noted: TargetPlaces,
    pub(crate) known: [Known<L>; TARGETS],
    pub(crate) outcome: Outcome<L>,
    /// The player an [`beryl_card::Effect::ForEachPlayer`] is being done for.
    pub(crate) that_player: Option<Seat>,
    /// The objects the last instruction to move any set out to move, by the player who
    /// controlled each as it did ([`Amount::ThoseOf`], CR 608.2h).
    pub(crate) controlled: PerPlayer<L, Bitboard<L::Slot>>,
}

impl<L: Layout> Default for Registers<L> {
    fn default() -> Self {
        Registers {
            legal: TargetPlaces::default(),
            noted: TargetPlaces::default(),
            known: [Known::default(); TARGETS],
            outcome: Outcome::default(),
            that_player: None,
            controlled: Table::filled(Bitboard::EMPTY),
        }
    }
}

/// What an expression is read against.
pub(crate) struct Scene<'s, L: Layout> {
    pub(crate) s: &'s State<L>,
    pub(crate) t: &'s GameTable,
    /// The ability, as the trigger it would be: its source, card, controller and what it
    /// triggered on. A spell's is its card.
    pub(crate) subject: &'s PendingTrigger<L>,
    /// A spell's casting options paid; `None` for an ability.
    pub(crate) costs: Option<Castings>,
    /// The announced value of X.
    pub(crate) x: u8,
    pub(crate) targets: &'s [Target<L>],
    pub(crate) registers: &'s Registers<L>,
}

impl<L: Layout> Scene<'_, L> {
    fn you(&self) -> Seat {
        self.subject.controller
    }

    /// The targets chosen for the `spec`th instance of the word "target" that were legal as
    /// the resolution began, with their places in the entry's list.
    fn targets(&self, spec: SpecIx) -> impl Iterator<Item = (TargetPlace, Target<L>)> + '_ {
        placed(self.targets).filter(move |&(place, target)| {
            target.spec() == spec && self.registers.legal.contains(place)
        })
    }

    /// The targets among [`Scene::targets`] the effect has moved, as they were last known.
    /// One an instruction set out to move and could not, as when what would have exiled it
    /// had left first (CR 610.3b), is still what [`Scene::objects`] names, and is not here.
    pub(crate) fn moved(&self, objects: Objects) -> impl Iterator<Item = &Known<L>> + '_ {
        let spec = match objects {
            Objects::Target(spec) => Some(spec),
            Objects::This
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::ExiledWithThis
            | Objects::All { .. }
            | Objects::InZoneOf { .. }
            | Objects::CardsIn { .. }
            | Objects::Paid(_) => None,
        };
        spec.into_iter()
            .flat_map(|spec| self.targets(spec))
            .filter(|&(place, target)| {
                self.registers.noted.contains(place)
                    && target
                        .as_object()
                        .is_some_and(|object| !self.s.is_current(object))
            })
            .map(|(place, _)| &self.registers.known[place.index()])
    }

    /// The objects `objects` names that are where they were: one that has changed zones is
    /// a new object ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which
    /// only [`Objects::Those`] follows.
    pub(crate) fn objects(&self, objects: Objects) -> Bitboard<L::Slot> {
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
            // CR 303.4b: "The object or player an Aura is attached to is called enchanted."
            // Asked of the source where it is now: once it has left the battlefield it is
            // attached to nothing.
            Objects::AttachedTo => self
                .subject
                .source
                .filter(|&source| s.is_current(source))
                .and_then(|source| s.object(source.slot).attached_to())
                .map_or(Bitboard::EMPTY, Bitboard::single),
            Objects::Those => self.registers.outcome.objects,
            Objects::TriggeredOn => (self.subject.info.object)
                .filter(|&object| s.is_current(object))
                .map_or(Bitboard::EMPTY, |object| Bitboard::single(object.slot)),
            // CR 607.2a: "The second ability refers only to cards in the exile zone that
            // were put there as a result of an instruction to exile them in the first
            // ability." An ability of an object that has left still refers to the cards
            // that object exiled, so its slot is asked whether or not it is current.
            Objects::ExiledWithThis => {
                (self.subject.source).map_or(Bitboard::EMPTY, |source| s.exiled_with(source.slot))
            }
            Objects::Paid(_) => {
                unreachable!("the card table refuses an effect that names what a cost took")
            }
            // "The cards in your library": a zone each player has one of holds its owner's
            // cards, which is the whole of what the filter asks.
            Objects::All {
                zone: zone @ (Zone::Library | Zone::Hand | Zone::Graveyard),
                filter: Filter::Whose(&Players::You),
            } => s.zone(ZoneKey::Of(self.you(), zone)),
            // CR 400.1: "Each player has their own library, hand, and graveyard."
            Objects::InZoneOf {
                zone,
                players,
                filter,
            } => {
                let mut within = Bitboard::EMPTY;
                // A set is built, whose order is nothing.
                for player in s.players_in_seat_order(self.players(*players)) {
                    within |= s.zone(ZoneKey::Of(player, zone));
                }
                filter.select_x(s, self.t, within, self.you(), self.x)
            }
            // CR 400.1: "Each player has their own library, hand, and graveyard."
            Objects::CardsIn { players, zones } => {
                let mut cards = Bitboard::EMPTY;
                for player in s.players_in_seat_order(self.players(*players)) {
                    for &zone in zones {
                        cards |= s.zone(ZoneKey::Of(player, zone));
                    }
                }
                cards
            }
            Objects::All { zone, filter } => {
                let within = match zone {
                    Zone::Battlefield => s.battlefield().iter().copied().collect(),
                    Zone::Stack => cast::spells(s),
                    _ => s.in_zone(zone),
                };
                filter.select_x(s, self.t, within, self.you(), self.x)
            }
        }
    }

    /// The players `players` names.
    pub(crate) fn players(&self, players: Players) -> PlayerSet {
        match players {
            // CR 109.5: "The words 'you' and 'your' on an object refer to the object's
            // controller".
            Players::You => PlayerSet::only(self.you()),
            // CR 102.2: "In a two-player game, a player's opponent is the other player."
            Players::EachOpponent => self.s.opponents_of(self.you()),
            // CR 102.1: "A player is one of the people in the game."
            Players::EachPlayer => self.s.players(),
            Players::Target(spec) => {
                let mut who = PlayerSet::NOBODY;
                for player in self.targets(spec).filter_map(|(_, t)| t.as_player()) {
                    who.insert(player);
                }
                who
            }
            // A target the effect moved is known as it last was. What `Those` and `All`
            // name are read where they are now, so their owners once they have left.
            Players::ControllerOf(objects) => {
                let mut who = PlayerSet::NOBODY;
                // The candidate order, here and for the owners: a set is built, whose order
                // is nothing.
                for slot in self.s.in_candidate_order(self.objects(objects)) {
                    who.insert(self.s.controller(slot));
                }
                for known in self.moved(objects) {
                    who.insert(known.controller);
                }
                who
            }
            // CR 108.3: "The owner of a card in the game is the player who started the game
            // with it in their deck." A slot keeps its owner through every move.
            Players::OwnerOf(objects) => {
                let mut who = PlayerSet::NOBODY;
                for slot in self.s.in_candidate_order(self.objects(objects)) {
                    who.insert(self.s.object(slot).owner());
                }
                if let Objects::Target(spec) = objects {
                    for object in self.targets(spec).filter_map(|(_, t)| t.as_object()) {
                        who.insert(self.s.object(object.slot).owner());
                    }
                }
                who
            }
            // CR 102.1: everyone in the game, but them.
            Players::EachOther(players) => {
                let mut who = self.s.players();
                for player in self.s.players_in_seat_order(self.players(*players)) {
                    who = who.without(player);
                }
                who
            }
            Players::ThatPlayer => {
                debug_assert!(
                    self.registers.that_player.is_some(),
                    "\"that player\" outside \"each player\""
                );
                self.registers
                    .that_player
                    .map_or(PlayerSet::NOBODY, PlayerSet::only)
            }
        }
    }

    /// What is known of the object in `slot` as it is now, for an effect about to move it.
    pub(crate) fn known(&self, slot: L::Slot) -> Known<L> {
        Known {
            controller: self.s.controller(slot),
            copiable: Some(self.s.copiable(slot)),
            power: self.s.power(self.t, slot),
            toughness: self.s.toughness(self.t, slot),
            mana_value: self.s.mana_value(self.t, slot),
            counters: self.s.counters(slot).held(),
        }
    }

    /// The total of `of` over the objects `objects` names, the targets the effect has moved
    /// among them (CR 608.2h).
    fn total(
        &self,
        objects: Objects,
        of: impl Fn(&Self, L::Slot) -> i32,
        known: impl Fn(&Known<L>) -> i32,
    ) -> i32 {
        let here: i32 = self.objects(objects).sum(|slot| of(self, slot));
        here + self.moved(objects).map(known).sum::<i32>()
    }

    /// The value of `amount` read with `player` in the place of "you" ([`Amount::Most`]):
    /// against this scene, as that player's.
    fn value_for(&self, player: Seat, amount: Amount) -> i32 {
        let subject = PendingTrigger {
            controller: player,
            ..*self.subject
        };
        let theirs = Scene {
            s: self.s,
            t: self.t,
            subject: &subject,
            costs: self.costs,
            x: self.x,
            targets: self.targets,
            registers: self.registers,
        };
        theirs.value(amount)
    }

    /// The value of `amount` before it is used, which may be below zero.
    fn value(&self, amount: Amount) -> i32 {
        match amount {
            Amount::Fixed(n) => i32::from(n),
            Amount::X => i32::from(self.x),
            Amount::ThatMany => i32::from(self.registers.outcome.number),
            // CR 608.2h: settled once, as the effect that made the delayed ability was
            // applied.
            Amount::Carried => i32::from(self.subject.info.amount),
            // Of those the instruction acted on, the ones each of the players controlled
            // as it set out to (CR 608.2h).
            Amount::ThoseOf(players) => {
                let (who, those) = (self.players(players), self.registers.outcome.objects);
                let theirs = |player: Seat| self.registers.controlled[player.index()] & those;
                let each = self.s.players_in_seat_order(who);
                each.map(|player| theirs(player).count().cast_signed())
                    .sum()
            }
            Amount::Count(objects) => self.objects(*objects).count().cast_signed(),
            Amount::Times(n, amount) => i32::from(n) * self.value(*amount),
            // CR 119.1: a player's life total, which may be below zero.
            Amount::Life(players) => {
                let who = self.players(players);
                let players = self.s.players_in_seat_order(who);
                players.map(|p| i32::from(self.s.player(p).life())).sum()
            }
            Amount::Most(players, amount) => {
                let who = self.players(players);
                let each = self.s.players_in_seat_order(who);
                each.map(|player| self.value_for(player, *amount))
                    .max()
                    .unwrap_or(0)
            }
            Amount::Power(objects) => self.total(
                objects,
                |scene, slot| i32::from(scene.s.power(scene.t, slot)),
                |known| i32::from(known.power),
            ),
            Amount::Toughness(objects) => self.total(
                objects,
                |scene, slot| i32::from(scene.s.toughness(scene.t, slot)),
                |known| i32::from(known.toughness),
            ),
            Amount::ManaValue(objects) => self.total(
                objects,
                |scene, slot| i32::from(scene.s.mana_value(scene.t, slot)),
                |known| i32::from(known.mana_value),
            ),
            Amount::Counters(objects, kind) => self.total(
                objects,
                |scene, slot| i32::from(scene.s.counters_of(slot, kind)),
                |known| i32::from(known.counters.get(kind)),
            ),
        }
    }

    /// The value of `amount` as its instruction is reached (CR 608.2h), never less than zero
    /// ([CR 107.1b](https://yawgatog.com/resources/magic-rules/#R1071b)): "If a calculation
    /// that would determine the result of an effect yields a negative number, zero is used
    /// instead".
    pub(crate) fn amount(&self, amount: Amount) -> u8 {
        u8::try_from(self.value(amount).max(0)).unwrap_or(u8::MAX)
    }

    /// Whether `condition` holds.
    pub(crate) fn holds(&self, condition: &Condition) -> bool {
        match *condition {
            // A coin flip did something when its player won it (`coins::instruction`).
            Condition::Did | Condition::WonFlip => self.registers.outcome.did,
            Condition::Paid(kind) => self.paid(kind),
            // Each side is never less than zero (CR 107.1b).
            Condition::Compare(left, comparison, right) => {
                comparison.holds(self.value(left).max(0), self.value(right).max(0))
            }
            Condition::Token(names) => self.is_a_token(names),
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
            // A permanent is tracked from the turn it enters to that turn's end, in a game
            // with a card that asks (`beryl_kernel`'s `Capability::EnteredThisTurn`).
            Condition::EnteredThisTurn(names) => {
                let objects = self.objects(names);
                !objects.is_empty()
                    && objects.all(|slot| self.s.tracked(Tracker::EnteredThisTurn, slot))
            }
            Condition::Not(condition) => !self.holds(condition),
            Condition::All(conditions) => conditions.iter().all(|c| self.holds(c)),
            Condition::Any(conditions) => conditions.iter().any(|c| self.holds(c)),
        }
    }

    /// Whether there is an object `names` names and every one is a token. The ability's own
    /// source that has left the battlefield is read as it last was there, which an ability
    /// that triggered on its leaving kept
    /// ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)): "Some zone-change
    /// triggers look back in time."
    fn is_a_token(&self, names: Objects) -> bool {
        let objects = self.objects(names);
        if objects.is_empty() && names == Objects::This {
            return self.subject.info.lki.token;
        }
        !objects.is_empty()
            && objects.all(|slot| self.s.object(slot).flags().has(ObjectFlags::TOKEN))
    }

    fn passing(&self, objects: Bitboard<L::Slot>, filter: Filter) -> Bitboard<L::Slot> {
        filter.select_x(self.s, self.t, objects, self.you(), self.x)
    }

    /// Whether the spell was cast with this kind of cost, or the permanent's spell was
    /// ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that spell
    /// has been 'kicked.'" A permanent remembers each kind its spell paid
    /// ([`entered_with`]); none is overloaded, as only an instant or sorcery has overload
    /// ([CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)).
    fn paid(&self, kind: CastingKind) -> bool {
        if let Some(costs) = self.costs {
            let face = self.t.def(self.subject.card).face(self.subject.face);
            return costs.paid(face).any(|option| option.kind() == kind);
        }
        kind != CastingKind::Overload && entered_with(self.s, self.subject, kind)
    }
}
