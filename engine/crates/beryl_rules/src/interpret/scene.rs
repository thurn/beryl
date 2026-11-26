//! Reading effect data's expressions against a game: which objects, which players, how many
//! and whether.
//!
//! A [`Scene`] is what an expression is read against: the game, the spell or ability whose
//! effect it is, and what that resolution has done so far ([`Registers`]). Reading changes
//! nothing, so the same reader serves an ability as it triggers, where there is no
//! resolution yet, and as it resolves.
//!
//! Implements: CR 107.1a, CR 107.1b, CR 109.5, CR 608.2h

use beryl_card::{Amount, CastingKind, Condition, Objects, Players, SpecIx, Types, Words};
use beryl_kernel::memos::LinkKind;
use beryl_kernel::reference::Current;

use crate::PlayerSet;
use crate::bitboard::Bitboard;
use crate::card::Castings;
use crate::casting;
use crate::filter::{Evaluate, Filter};
use crate::ids::Seat;
use crate::layout::{Layout, PerPlayer, Table};
use crate::query::Copiable;
use crate::stack::trigger::entered_with;
use crate::state::{
    AbilityRef, AbilityTarget, Count, CountersHeld, ObjectRef, PendingTrigger, State, TARGETS,
    Target, TargetPlace, TargetPlaces, Tracker, Zone, ZoneKey, placed,
};
use crate::table::GameTable;

/// What the instruction before did (`beryl_card`'s `effect` module, "What the instruction
/// before did"): the objects it acted on, as they are now, how many or how much it did, and
/// whether it did anything.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Outcome<L: Layout> {
    pub(crate) objects: Bitboard<L::Slot>,
    pub(crate) number: u8,
    pub(crate) did: bool,
    pub(crate) subgame_nonwinners: PlayerSet,
}

impl<L: Layout> Outcome<L> {
    /// The outcome of an instruction that did nothing.
    pub(crate) const NOTHING: Outcome<L> = Outcome {
        objects: Bitboard::EMPTY,
        number: 0,
        did: false,
        subgame_nonwinners: PlayerSet::NOBODY,
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
            ..Self::NOTHING
        }
    }

    /// This outcome and `other`'s together, as those of each player an instruction was done
    /// for.
    pub(crate) fn with(self, other: Outcome<L>) -> Outcome<L> {
        Outcome {
            objects: self.objects | other.objects,
            number: self.number.saturating_add(other.number),
            did: self.did || other.did,
            subgame_nonwinners: self.subgame_nonwinners.union(other.subgame_nonwinners),
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
    /// Its copiable values (see [CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)),
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
    /// (see [CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If the spell or
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
    /// controlled each as it did ([`Amount::ThoseOf`], [`Players::ControllerOf`] of
    /// [`Objects::Those`], CR 608.2h). Under an [`beryl_card::Effect::ForEachPlayer`], what
    /// each player's turn at the instruction moved, together.
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
    /// Which text changes of the object the spell or ability is printed on its color words
    /// and subtypes are read through, as a [`crate::state::StackEntry::words`] names them:
    /// `READ_NOW` for a spell or for an ability not on the stack, its source's as they are now;
    /// for an ability on the stack, its source's as it went there ([`State::words_by`]).
    /// Read only where a word is.
    pub(crate) words: u8,
}

impl<L: Layout> Scene<'_, L> {
    pub(crate) fn you(&self) -> Seat {
        self.subject.of.controller.seat()
    }

    /// The ability's source if it is still the object it was (see CR 400.7): a source that
    /// has changed zones since is a new object, which "this" no longer names.
    fn source(&self) -> Option<ObjectRef<L>> {
        let source = self.subject.of.source?;
        self.s.current(source).map(Current::reference)
    }

    /// What the ability triggered on, or the object a delayed ability carries, if it is still
    /// the object it was (see CR 400.7).
    fn triggered_on(&self) -> Option<ObjectRef<L>> {
        let object = self.subject.info.object?;
        self.s.current(object).map(Current::reference)
    }

    fn words(&self) -> Words {
        (self.s).words_by(self.subject.of.source.map(|source| source.slot), self.words)
    }

    /// The targets chosen for the `spec`th instance of the word "target" that were legal as
    /// the resolution began, with their places in the entry's list.
    fn targets(&self, spec: SpecIx) -> impl Iterator<Item = (TargetPlace, Target<L>)> + '_ {
        placed(self.targets).filter(move |&(place, target)| {
            target.spec() == spec && self.registers.legal.contains(place)
        })
    }

    /// The legal targets of one target spec in the order they were chosen.
    pub(crate) fn legal_targets(&self, spec: SpecIx) -> impl Iterator<Item = Target<L>> + '_ {
        self.targets(spec).map(|(_, target)| target)
    }

    /// The targets among [`Scene::targets`] the effect has moved, as they were last known.
    /// One an instruction set out to move and could not, as when what would have exiled it
    /// had left first (see CR 610.3b), is still what [`Scene::objects`] names, and is not here.
    pub(crate) fn moved(&self, objects: Objects) -> impl Iterator<Item = &Known<L>> + '_ {
        let spec = match objects {
            Objects::Target(spec) => Some(spec),
            Objects::This
            | Objects::AttachedTo
            | Objects::Those
            | Objects::TriggeredOn
            | Objects::TriggeredOnIn(_)
            | Objects::Carried
            | Objects::ExiledWithThis
            | Objects::LibraryTop { .. }
            | Objects::ExceptThose(_)
            | Objects::PutOntoBattlefieldWithThis
            | Objects::All { .. }
            | Objects::InZoneOf { .. }
            | Objects::CardsIn { .. }
            | Objects::Everywhere(_)
            | Objects::Paid(_)
            | Objects::Other(_) => None,
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

    /// The abilities on the stack `objects` names, a bit for each place counted from the
    /// bottom: the targets of an ability target that were legal as the resolution began and
    /// are still on the stack. Nothing names an ability but a target.
    pub(crate) fn abilities(&self, objects: Objects) -> u64 {
        let Objects::Target(spec) = objects else {
            return 0;
        };
        let mut places = 0;
        for (_, target) in self.targets(spec) {
            if let Some(AbilityTarget::At(place)) = target.as_ability() {
                places |= 1 << place;
            }
        }
        places
    }

    /// The linked cards in exile that pass `filter`.
    pub(crate) fn linked_exiles(&self, filter: Filter) -> Bitboard<L::Slot> {
        let exiled = self.objects(Objects::ExiledWithThis);
        filter.select_x(self.s, self.t, exiled, self.you(), &self.words(), self.x)
    }

    /// The objects `objects` names that are where they were: one that has changed zones is
    /// a new object (see [CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), which
    /// only [`Objects::Those`] follows.
    pub(crate) fn objects(&self, objects: Objects) -> Bitboard<L::Slot> {
        let s = self.s;
        match objects {
            Objects::This => self
                .source()
                .map_or(Bitboard::EMPTY, |source| Bitboard::single(source.slot)),
            Objects::Target(spec) => self
                .targets(spec)
                .filter_map(|(_, target)| s.current(target.as_object()?))
                .map(|object| object.reference().slot)
                .collect(),
            // See CR 303.4b: "The object or player an Aura is attached to is called enchanted."
            // Asked of the source where it is now: once it has left the battlefield it is
            // attached to nothing.
            Objects::AttachedTo => self
                .source()
                .and_then(|source| s.object(source.slot).attached_to())
                .map_or(Bitboard::EMPTY, Bitboard::single),
            Objects::Those => self.registers.outcome.objects,
            Objects::ExceptThose(objects) => {
                self.objects(*objects) & !self.registers.outcome.objects
            }
            Objects::LibraryTop { players, count } => self.library_top(*players, count),
            Objects::TriggeredOn => self
                .triggered_on()
                .map_or(Bitboard::EMPTY, |object| Bitboard::single(object.slot)),
            // See CR 603.7c: "if that object is no longer in the zone it's expected to be in at
            // the time the delayed triggered ability resolves, the ability won't affect it."
            // A delayed ability carries its object in the place a trigger keeps what its
            // event was about, and one that has changed zones is no longer current.
            // The cards a merged or melded permanent became in exile are found together
            // (see CR 713.8c).
            Objects::Carried => self
                .triggered_on()
                .map_or(Bitboard::EMPTY, |object| s.exiled_together(object.slot)),
            Objects::TriggeredOnIn(zone) => self
                .triggered_on()
                .filter(|&object| s.object(object.slot).zone() == zone)
                .map_or(Bitboard::EMPTY, |object| Bitboard::single(object.slot)),
            // See CR 607.2a: "The second ability refers only to cards in the exile zone that
            // were put there as a result of an instruction to exile them in the first
            // ability." An ability of an object that has left still refers to the cards
            // that object exiled, whether or not it is current, and never to those of a new
            // object in its slot (see CR 400.7).
            Objects::ExiledWithThis => {
                let link = LinkKind::ExiledWith;
                (self.subject.of.source).map_or(Bitboard::EMPTY, |source| s.linked(link, source))
            }
            // "That creature": the permanents an ability of the source put onto the
            // battlefield and that are still there, whether or not the source is.
            Objects::PutOntoBattlefieldWithThis => {
                let link = LinkKind::PutOntoBattlefieldWith;
                (self.subject.of.source).map_or(Bitboard::EMPTY, |source| s.linked(link, source))
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
            // See CR 400.1: "Each player has their own library, hand, and graveyard."
            Objects::InZoneOf {
                zone,
                players,
                filter,
            } => {
                let within = self.zone_of(*players, zone);
                filter.select_x(s, self.t, within, self.you(), &self.words(), self.x)
            }
            // See CR 400.1: "Each player has their own library, hand, and graveyard."
            Objects::CardsIn { players, zones } => {
                let mut cards = Bitboard::EMPTY;
                for player in self.players(*players).in_seat_order() {
                    for &zone in zones {
                        cards |= s.zone(ZoneKey::Of(player, zone));
                    }
                }
                cards
            }
            Objects::All { zone, filter } => {
                let within = match zone {
                    Zone::Battlefield => s.battlefield().iter().copied().collect(),
                    Zone::Stack => casting::spells(s),
                    Zone::Library
                    | Zone::Hand
                    | Zone::Graveyard
                    | Zone::Exile
                    | Zone::Nowhere
                    | Zone::Merged => s.in_zone(zone),
                };
                filter.select_x(s, self.t, within, self.you(), &self.words(), self.x)
            }
            // See CR 611.3b: the cards in every zone, the spells and the permanents.
            Objects::Everywhere(filter) => {
                let mut within: Bitboard<L::Slot> = s.battlefield().iter().copied().collect();
                within |= casting::spells(s);
                for zone in [Zone::Library, Zone::Hand, Zone::Graveyard, Zone::Exile] {
                    within |= s.in_zone(zone);
                }
                filter.select_x(s, self.t, within, self.you(), &self.words(), self.x)
            }
            // "Each other creature": the set without the ability's source. A source that has
            // changed zones since is a new object (see CR 400.7), which is "other".
            Objects::Other(objects) => {
                let mut others = self.objects(*objects);
                if let Some(source) = self.source() {
                    others.remove(source.slot);
                }
                others
            }
        }
    }

    /// The top cards of each player's library, in their original zone. See [CR 401.2]:
    /// "Each library must be kept in a single face-down pile."
    ///
    /// [CR 401.2]: https://yawgatog.com/resources/magic-rules/#R4012
    fn library_top(&self, players: Players, count: u8) -> Bitboard<L::Slot> {
        let mut cards = Bitboard::EMPTY;
        for player in self.players(players).in_seat_order() {
            cards |= self
                .s
                .library_cards_from_top(player)
                .take(usize::from(count))
                .collect();
        }
        cards
    }

    /// The permanents these players control, or the cards in their own zones.
    ///
    /// See [CR 110.2]: "Every permanent has a controller." The controller can differ
    /// from the owner, so battlefield membership is read by controller.
    ///
    /// [CR 110.2]: https://yawgatog.com/resources/magic-rules/#R1102
    #[inline]
    fn zone_of(&self, players: Players, zone: Zone) -> Bitboard<L::Slot> {
        let players = self.players(players);
        if zone == Zone::Battlefield {
            return self
                .s
                .permanents_with_derived()
                .filter_map(|(slot, derived)| players.contains(derived.controller).then_some(slot))
                .collect();
        }
        let mut within = Bitboard::EMPTY;
        for player in players.in_seat_order() {
            within |= self.s.zone(ZoneKey::Of(player, zone));
        }
        within
    }

    /// The players `players` names.
    pub(crate) fn players(&self, players: Players) -> PlayerSet {
        match players {
            // CR 109.5: "The words 'you' and 'your' on an object refer to the object's
            // controller".
            Players::You => PlayerSet::only(self.you()),
            // See CR 102.2: "In a two-player game, a player's opponent is the other player."
            Players::EachOpponent => self.s.opponents_of(self.you()),
            // See CR 102.1: "A player is one of the people in the game."
            Players::EachPlayer => self.s.players(),
            // See CR 303.4b: "The object or player an Aura is attached to is called enchanted."
            Players::Enchanted => self
                .source()
                .and_then(|source| self.s.object(source.slot).enchanted_player())
                .map_or(PlayerSet::NOBODY, PlayerSet::only),
            Players::Target(spec) => {
                let mut who = PlayerSet::NOBODY;
                for player in self.targets(spec).filter_map(|(_, t)| t.as_player()) {
                    who.insert(player);
                }
                who
            }
            // CR 608.2h: "the effect uses the object's last known information." A target the
            // effect moved is known as it last was, and so is one of `Those` the instruction
            // moved ([`Scene::controller`]). What any other name names is read where it is.
            Players::ControllerOf(objects) => {
                let moved = objects == Objects::Those;
                let mut who = self
                    .objects(objects)
                    .fold(PlayerSet::NOBODY, |mut who, slot| {
                        who.insert(if moved {
                            self.controller(slot)
                        } else {
                            self.s.controller(slot)
                        });
                        who
                    });
                // CR 608.2h: "if it’s no longer in that zone ... the effect uses the
                // object’s last known information." A becomes-target-of-a-spell trigger
                // remembers that spell's controller even if the spell has been countered.
                if who == PlayerSet::NOBODY
                    && objects == Objects::TriggeredOn
                    && self.subject.info.object.is_some()
                {
                    let ability = crate::stack::trigger::ability(self.t, &self.subject.of);
                    if ability.trigger().is_some_and(|trigger| {
                        matches!(
                            trigger.event,
                            beryl_card::TriggerEvent::BecomesTargetOfSpell
                        )
                    }) {
                        who.insert(self.subject.info.player);
                    }
                }
                for known in self.moved(objects) {
                    who.insert(known.controller);
                }
                who
            }
            // See CR 108.3: "The owner of a card in the game is the player who started the game
            // with it in their deck." A slot keeps its owner through every move.
            Players::OwnerOf(objects) => {
                let mut who = self
                    .objects(objects)
                    .fold(PlayerSet::NOBODY, |mut who, slot| {
                        who.insert(self.s.object(slot).owner());
                        who
                    });
                if let Objects::Target(spec) = objects {
                    for object in self.targets(spec).filter_map(|(_, t)| t.as_object()) {
                        who.insert(self.s.object(object.slot).owner());
                    }
                }
                who
            }
            // See CR 102.1: everyone in the game, but them.
            Players::EachOther(players) => {
                let mut who = self.s.players();
                for player in self.players(*players).in_seat_order() {
                    who = who.without(player);
                }
                who
            }
            Players::SubgameNonwinners => self.registers.outcome.subgame_nonwinners,
            // An instruction under "each player" uses that player. Otherwise a cast
            // trigger refers to the player who cast the spell (see CR 601.2a).
            Players::ThatPlayer => PlayerSet::only(
                self.registers
                    .that_player
                    .unwrap_or(self.subject.info.player),
            ),
        }
    }

    /// The controller of one of [`Objects::Those`]: of a permanent or a spell, its controller
    /// (see [CR 108.4](https://yawgatog.com/resources/magic-rules/#R1084)): "A card doesn't have
    /// a controller unless that card represents a permanent or spell". Of a card anywhere
    /// else that the last instruction to move any set out to move, the player who controlled
    /// it as that instruction did ([`Registers::controlled`], CR 608.2h), and of any other
    /// its owner (see CR 108.4a).
    fn controller(&self, slot: L::Slot) -> Seat {
        let s = self.s;
        let now = s.controller(slot);
        if matches!(s.object(slot).zone(), Zone::Battlefield | Zone::Stack) {
            return now;
        }
        s.players()
            .in_seat_order()
            .find(|player| self.registers.controlled[player.index()].contains(slot))
            .unwrap_or(now)
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
            of: AbilityRef {
                controller: player.into(),
                ..self.subject.of
            },
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
            words: self.words,
        };
        theirs.value(amount)
    }

    /// The value of `amount` before it is used, which may be below zero.
    pub(crate) fn value(&self, amount: Amount) -> i32 {
        match amount {
            Amount::Fixed(n) => i32::from(n),
            Amount::X => i32::from(self.x),
            Amount::ThatMany => i32::from(self.registers.outcome.number),
            // CR 608.2h: settled once, as the effect that made the delayed ability was
            // applied.
            // What a delayed trigger carries, or what a "when you cast" ability kept as it
            // triggered (`trigger::fires`), which the card table allows in no other ability.
            Amount::Carried | Amount::SpellsCastBefore => i32::from(self.subject.info.amount),
            // Of those the instruction acted on, the ones each of the players controlled
            // as it set out to (CR 608.2h).
            Amount::ThoseOf(players) => {
                let (who, those) = (self.players(players), self.registers.outcome.objects);
                let theirs = |player: Seat| self.registers.controlled[player.index()] & those;
                let each = who.in_seat_order();
                each.map(|player| theirs(player).count().cast_signed())
                    .sum()
            }
            Amount::Count(objects) => self.objects(*objects).count().cast_signed(),
            Amount::Times(n, amount) => i32::from(n) * self.value(*amount),
            Amount::Plus(n, amount) => self.value(*amount) + i32::from(n),
            Amount::Half { of, round_up } => round_half(self.value(*of), round_up),
            // See CR 300.1: each card type once, among the cards: a token is none (see CR 108.2).
            // A union of types, the same in any order the cards are read.
            Amount::CardTypesAmong(objects) => {
                let types = self
                    .objects(*objects)
                    .filter(|slot| !self.s.object(slot).is_token())
                    .fold(Types::NONE, |types, slot| {
                        types.union(self.s.types(self.t, slot))
                    });
                types.0.count_ones().cast_signed()
            }
            // See CR 119.1: a player's life total, which may be below zero.
            Amount::Life(players) => {
                let who = self.players(players);
                let players = who.in_seat_order();
                players.map(|p| i32::from(self.s.player(p).life())).sum()
            }
            // Counted for each player this turn, in a game with a card that reads the count
            // (`beryl_kernel`'s `Capability::SpellsCast` and `Capability::CardsDrawn`).
            Amount::SpellsCast(players) => self.counted(players, Count::SpellsCast),
            Amount::CardsDrawn(players) => self.counted(players, Count::CardsDrawn),
            Amount::Most(players, amount) => {
                let who = self.players(players);
                let each = who.in_seat_order();
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
            // See CR 608.2h: "if it’s no longer in that zone ... the effect uses the
            // object’s last known information." The original spell's chosen face and X
            // survive when an ability still needs them after it has left the stack.
            Amount::ManaValue(Objects::This) if self.objects(Objects::This).is_empty() => self
                .subject
                .of
                .source
                .and_then(|source| self.s.departed_spell(source))
                .map_or(0, |spell| {
                    i32::from(beryl_kernel::query::spell_mana_value(self.t, &spell.entry))
                }),
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

    /// The total of `count` this turn over the players `players` names.
    fn counted(&self, players: Players, count: Count) -> i32 {
        let who = self.players(players);
        let each = who.in_seat_order();
        each.map(|player| i32::from(self.s.counted(count, player)))
            .sum()
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
            // "If it's on the battlefield": of the ability's source, still the object it was
            // (see CR 400.7).
            Condition::OnBattlefield(names) => {
                let objects = self.objects(names);
                !objects.is_empty() && objects.all(|slot| self.s.object(slot).on_battlefield())
            }
            // "If you cast it": a permanent remembers it was a spell its controller cast, in
            // a game with a card that asks (`beryl_kernel`'s `Capability::RemembersCast`).
            Condition::Cast(names) => {
                let objects = self.objects(names);
                !objects.is_empty() && objects.all(|slot| self.s.was_cast(slot))
            }
            Condition::Is(names, filter) => {
                // What a target was is not kept once the effect has moved it (`Known` holds
                // no characteristics), so "if it was a [filter]" cannot be asked of one: a
                // definition that would is refused as it compiles (`beryl_card`'s
                // `Malformed::LastKnown`).
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
            // See CR 504.1: whether each of the players has drawn a card in their own draw step
            // this turn, counted in a game with a card that asks
            // (`beryl_kernel`'s `Capability::DrewInDrawStep`).
            Condition::DrewInDrawStep(players) => {
                let who = self.players(players);
                let mut each = who.in_seat_order().peekable();
                each.peek().is_some()
                    && each.all(|player| self.s.counted(Count::DrewInDrawStep, player) > 0)
            }
            // See CR 701.42a (`actions::meld`).
            Condition::MeldPair(partner) => self.meld_pair(partner).is_some(),
            Condition::Not(condition) => !self.holds(condition),
            Condition::All(conditions) => conditions.iter().all(|c| self.holds(c)),
            Condition::Any(conditions) => conditions.iter().any(|c| self.holds(c)),
        }
    }

    /// Whether there is an object `names` names and every one is a token. The ability's own
    /// source that has left the battlefield is read as it last was there, from the last-known
    /// information an ability that triggered on its leaving kept ([`State::last_known`])
    /// (see [CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)): "Some zone-change
    /// triggers look back in time."
    fn is_a_token(&self, names: Objects) -> bool {
        let objects = self.objects(names);
        if objects.is_empty() && names == Objects::This {
            let source = self.subject.of.source;
            return source
                .and_then(|r| self.s.last_known(r))
                .is_some_and(|row| row.token);
        }
        !objects.is_empty() && objects.all(|slot| self.s.object(slot).is_token())
    }

    pub(crate) fn passing(&self, objects: Bitboard<L::Slot>, filter: Filter) -> Bitboard<L::Slot> {
        filter.select_x(self.s, self.t, objects, self.you(), &self.words(), self.x)
    }

    /// Whether the spell was cast with this kind of cost, or the permanent's spell was
    /// (see [CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)): "If a spell's
    /// controller declares the intention to pay any of that spell's kicker costs, that spell
    /// has been 'kicked.'" A permanent remembers each kind its spell paid
    /// ([`entered_with`]); none is overloaded, as only an instant or sorcery has overload
    /// (see [CR 702.96a](https://yawgatog.com/resources/magic-rules/#R70296a)).
    #[allow(
        clippy::disallowed_methods,
        reason = "reads the casting options printed on the face the spell was cast as"
    )]
    fn paid(&self, kind: CastingKind) -> bool {
        if let Some(costs) = self.costs {
            let face = self.t.def(self.subject.of.card).face(self.subject.of.face);
            return costs.paid(face).any(|option| option.kind() == Some(kind));
        }
        kind != CastingKind::Overload && entered_with(self.s, self.subject, kind)
    }
}

/// Under [CR 107.1a], "If a spell or ability could generate a fractional number, the spell
/// or ability will tell you whether to round up or down."
///
/// [CR 107.1a]: https://yawgatog.com/resources/magic-rules/#R1071a
fn round_half(value: i32, round_up: bool) -> i32 {
    let half = value.div_euclid(2);
    half + i32::from(round_up && value.rem_euclid(2) != 0)
}
