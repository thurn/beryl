//! Carrying out effect data's instructions, each through the [`EffectCtx`] method a Rust
//! body would call for it, so an ability written as data makes the events its body made.
//!
//! An instruction acts on every object it names at once where the engine has a primitive
//! that does, and otherwise on each permanent in the order they entered the battlefield, an
//! order the game can see (slots are none: the deal puts cards in them at random).

use beryl_card::{CopyException, Duration, Effect, LibraryPosition, Objects, Players};

use crate::bitboard::Bitboard;
use crate::card::{CardDef, FaceAbility, Flow, ManaCost, ManaSet, Modification};
use crate::cast;
use crate::decision::Ask;
use crate::effect::EffectCtx;
use crate::event::{self, Entry, Reason, TokenEntry};
use crate::ids::Seat;
use crate::interpret::scene::{Outcome, Registers, Scene};
use crate::layout::{Layout, Queue};
use crate::state::{self, Continuous, CounterKind, ObjectRef, PendingTrigger, StackEntry, Zone};
use crate::zone::Place;

/// One resolution of an ability written as data: its context, and what it keeps between
/// instructions.
pub(super) struct Run<'c, 'a, L: Layout> {
    ctx: &'c mut EffectCtx<'a, L>,
    subject: PendingTrigger<L>,
    registers: Registers<L>,
}

impl<'c, 'a, L: Layout> Run<'c, 'a, L> {
    /// Begins resolving the ability `ctx` was made for, noting which of its targets are
    /// still legal ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "If a
    /// spell or ability specifies targets, it checks whether the targets are still legal."
    pub(super) fn new(ctx: &'c mut EffectCtx<'a, L>) -> Self {
        let entry = &ctx.entry;
        let subject = PendingTrigger {
            source: entry.object,
            card: entry.card,
            face: entry.face,
            ability: match entry.ability {
                Some(FaceAbility::Own(index)) => index,
                _ => 0,
            },
            controller: entry.controller,
            info: entry.info,
        };
        let mut legal = 0;
        for (i, &target) in entry.targets.iter().enumerate() {
            if cast::still_legal(ctx.state, ctx.table, entry, ctx.ability, target) {
                legal |= 1 << i;
            }
        }
        Run {
            ctx,
            subject,
            registers: Registers {
                legal,
                ..Registers::default()
            },
        }
    }

    fn scene(&self) -> Scene<'_, L> {
        let entry = &self.ctx.entry;
        Scene {
            s: self.ctx.state,
            t: self.ctx.table,
            subject: &self.subject,
            costs: entry.is_spell().then_some(entry.costs),
            x: entry.x,
            targets: entry.targets.as_slice(),
            registers: &self.registers,
        }
    }

    /// Carries out one instruction, or each of several in order
    /// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of
    /// the spell or ability follows its instructions in the order written."
    pub(super) fn carry_out(&mut self, effect: &'static Effect) -> Flow {
        self.registers.outcome = match *effect {
            Effect::Sequence(effects) => {
                for effect in effects {
                    self.carry_out(effect)?;
                }
                return Ok(());
            }
            // CR 608.2h: "the answer is determined only once, when the effect is applied."
            Effect::If {
                condition,
                then,
                otherwise,
            } => {
                let branch = if self.scene().holds(&condition) {
                    Some(then)
                } else {
                    otherwise
                };
                return self.carry_out_or_nothing(branch);
            }
            // CR 608.2d: "If an effect of a spell or ability offers any choices ..., the
            // player announces these while applying the effect."
            Effect::May { player, effect } => {
                let agreed = match self.one(player) {
                    Some(player) => self.ctx.choose_yes_no(player, Ask::Plain)?,
                    None => false,
                };
                return self.carry_out_or_nothing(agreed.then_some(effect));
            }
            // CR 118.12a: "[A player may do something else]. If [that player doesn't], [do
            // something]." With no such player, no one pays.
            Effect::Unless {
                effect,
                player,
                cost,
            } => {
                let paid = self.unless_pays(player, cost)?;
                return self.carry_out_or_nothing((!paid).then_some(effect));
            }
            Effect::ForEachPlayer { players, effect } => {
                return self.for_each_player(players, effect);
            }
            Effect::Counter(objects) => self.counter(objects),
            // CR 701.8a: "To destroy a permanent, move it from the battlefield to its owner's
            // graveyard."
            Effect::Destroy(objects) => self.remove(objects, EffectCtx::destroy_all),
            Effect::Exile(objects) => self.exile(objects),
            Effect::ExileUntilThisLeaves(objects) => {
                let permanents = self.permanents(objects);
                self.remember(permanents);
                self.each_permanent(permanents, EffectCtx::exile_until_source_leaves);
                self.left_the_battlefield(permanents)
            }
            // CR 701.21a: "To sacrifice a permanent, its controller moves it from the
            // battlefield directly to its owner's graveyard."
            Effect::Sacrifice(objects) => self.remove(objects, |ctx, permanents| {
                ctx.move_all(permanents, Zone::Graveyard, Reason::Sacrifice);
            }),
            Effect::ReturnToHand(objects) => self.put(objects, Zone::Hand, Place::Top),
            Effect::PutIntoLibrary { objects, position } => {
                let place = match position {
                    LibraryPosition::Top | LibraryPosition::FromTop(0 | 1) => Place::Top,
                    LibraryPosition::Bottom => Place::Bottom,
                    // CR 401.7: with "fewer than N cards in it, the player puts that card on
                    // the bottom of that library."
                    LibraryPosition::FromTop(n) => Place::UnderTop(n - 1),
                };
                self.put(objects, Zone::Library, place)
            }
            Effect::Draw { player, count } => self.draw(player, self.scene().amount(count)),
            Effect::Discard { player, count } => {
                self.discard(player, self.scene().amount(count))?
            }
            Effect::Mill { player, count } => self.mill(player, self.scene().amount(count)),
            Effect::Scry { player, count } => self.scry(player, self.scene().amount(count))?,
            Effect::GainLife { player, amount } => {
                self.change_life(player, self.scene().amount(amount), EffectCtx::gain_life)
            }
            Effect::LoseLife { player, amount } => {
                self.change_life(player, self.scene().amount(amount), EffectCtx::lose_life)
            }
            Effect::CreateTokens {
                player,
                token,
                count,
            } => self.create_tokens(player, token, self.scene().amount(count)),
            Effect::CreateCopies {
                player,
                of,
                count,
                except,
            } => self.create_copies(player, of, self.scene().amount(count), except),
            Effect::AddCounters {
                objects,
                kind,
                count,
            } => self.change_counters(objects, kind, self.scene().amount(count), true),
            Effect::RemoveCounters {
                objects,
                kind,
                count,
            } => self.change_counters(objects, kind, self.scene().amount(count), false),
            Effect::AddMana { player, mana } => self.add_mana(player, mana),
            Effect::Modify {
                objects,
                modification,
                duration,
            } => self.modify(objects, modification, duration),
        };
        Ok(())
    }

    /// Carries out the effect an [`Effect::If`], [`Effect::May`] or [`Effect::Unless`] came
    /// to, whose outcome is then its own; one that came to nothing did nothing.
    fn carry_out_or_nothing(&mut self, effect: Option<&'static Effect>) -> Flow {
        let Some(effect) = effect else {
            self.registers.outcome = Outcome::NOTHING;
            return Ok(());
        };
        self.carry_out(effect)
    }

    /// Carries out `effect` for each of `players` in turn order (CR 101.4), each of them
    /// "that player" in it. Each begins from what the instruction before this one did, and
    /// the outcome is every player's together.
    fn for_each_player(&mut self, players: Players, effect: &'static Effect) -> Flow {
        let (before, outer) = (self.registers.outcome, self.registers.that_player);
        let mut together = Outcome::NOTHING;
        for player in self.in_turn_order(players) {
            self.registers.that_player = Some(player);
            self.registers.outcome = before;
            self.carry_out(effect)?;
            together = together.with(self.registers.outcome);
        }
        self.registers.that_player = outer;
        self.registers.outcome = together;
        Ok(())
    }

    /// The players `players` names, in the order they act (CR 101.4).
    fn in_turn_order(&self, players: Players) -> impl Iterator<Item = Seat> + use<L> {
        let who = self.scene().players(players);
        who.in_turn_order(self.ctx.state.turn.active)
    }

    /// The one player who makes a choice the effect offers `players`.
    fn one(&self, players: Players) -> Option<Seat> {
        self.in_turn_order(players).next()
    }

    /// "Unless [player] pays [cost]" (CR 118.12a): whether they paid.
    fn unless_pays(&mut self, player: Players, cost: ManaCost) -> Result<bool, crate::card::Pause> {
        match self.one(player) {
            Some(player) => self.ctx.unless_pays(player, cost),
            None => Ok(false),
        }
    }

    /// The permanents among the objects `objects` names
    /// ([CR 110.1](https://yawgatog.com/resources/magic-rules/#R1101)): "A permanent is a
    /// card or token on the battlefield."
    fn permanents(&self, objects: Objects) -> Bitboard<L::Slot> {
        self.elsewhere_than(objects, None)
            .iter()
            .filter(|&slot| self.ctx.state.object(slot).zone == Zone::Battlefield)
            .collect()
    }

    /// The objects `objects` names that are not in `zone` already.
    fn elsewhere_than(&self, objects: Objects, zone: Option<Zone>) -> Bitboard<L::Slot> {
        let named = self.scene().objects(objects);
        match zone {
            Some(zone) => named
                .iter()
                .filter(|&slot| self.ctx.state.object(slot).zone != zone)
                .collect(),
            None => named,
        }
    }

    /// Notes what is known of the targets among `leaving` before the effect moves them, for
    /// the instructions after (CR 608.2h): "the effect uses the object's last known
    /// information."
    fn remember(&mut self, leaving: Bitboard<L::Slot>) {
        for i in 0..self.ctx.entry.targets.len() {
            let target = self.ctx.entry.targets.as_slice()[i];
            let Some(object) = target.as_object() else {
                continue;
            };
            if self.registers.legal & (1 << i) != 0
                && self.ctx.state.is_current(object)
                && leaving.contains(object.slot)
            {
                self.registers.known[i] = self.scene().known(object.slot);
                self.registers.moved |= 1 << i;
            }
        }
    }

    /// The outcome of an instruction that moved `permanents` off the battlefield: those that
    /// left it, as they are now
    /// ([CR 400.7j](https://yawgatog.com/resources/magic-rules/#R4007j)): "If an effect
    /// causes an object to move to a public zone, other parts of that effect can find that
    /// object."
    fn left_the_battlefield(&self, permanents: Bitboard<L::Slot>) -> Outcome<L> {
        let s = &*self.ctx.state;
        Outcome::acted_on(
            permanents
                .iter()
                .filter(|&slot| s.object(slot).zone != Zone::Battlefield)
                .collect(),
        )
    }

    /// Moves the permanents `objects` names off the battlefield together with `act`.
    fn remove(
        &mut self,
        objects: Objects,
        act: impl FnOnce(&mut EffectCtx<'a, L>, Bitboard<L::Slot>),
    ) -> Outcome<L> {
        let permanents = self.permanents(objects);
        if permanents.is_empty() {
            return Outcome::NOTHING;
        }
        self.remember(permanents);
        act(self.ctx, permanents);
        self.left_the_battlefield(permanents)
    }

    /// Does `act` to each of `permanents` that is still one, in the order they entered the
    /// battlefield.
    fn each_permanent(
        &mut self,
        mut permanents: Bitboard<L::Slot>,
        mut act: impl FnMut(&mut EffectCtx<'a, L>, ObjectRef<L>),
    ) {
        loop {
            let s = &*self.ctx.state;
            let next = s
                .battlefield()
                .iter()
                .find(|&&slot| permanents.contains(slot));
            let Some(&slot) = next else {
                break;
            };
            permanents.remove(slot);
            let object = s.object_ref(slot);
            act(self.ctx, object);
        }
    }

    /// The topmost spell on the stack among `spells`.
    fn top_spell(&self, spells: Bitboard<L::Slot>) -> Option<ObjectRef<L>> {
        let stack = self.ctx.state.stack().iter().rev();
        stack
            .filter_map(StackEntry::spell)
            .find(|spell| spells.contains(spell.slot))
    }

    /// "Counter [spell]" (CR 701.6a): "A countered spell is put into its owner's graveyard."
    /// Of several, the topmost first.
    fn counter(&mut self, objects: Objects) -> Outcome<L> {
        let mut spells = self.scene().objects(objects);
        self.remember(spells);
        let mut countered = Bitboard::EMPTY;
        while let Some(spell) = self.top_spell(spells) {
            spells.remove(spell.slot);
            if self.ctx.counter(spell, false) {
                countered.insert(spell.slot);
            }
        }
        Outcome::acted_on(countered)
    }

    /// Takes the spells among `objects` off the stack, for an effect that moves their cards
    /// elsewhere without their resolving. One cast with flashback is exiled instead
    /// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "If the flashback
    /// cost was paid, exile this card instead of putting it anywhere else any time it would
    /// leave the stack." Returns the objects still to move.
    fn unstack(&mut self, objects: Bitboard<L::Slot>) -> Bitboard<L::Slot> {
        let mut flashback = Bitboard::EMPTY;
        self.ctx.state.stack.retain(|entry| match entry.spell() {
            Some(spell) if objects.contains(spell.slot) => {
                if entry.flashback {
                    flashback.insert(spell.slot);
                }
                false
            }
            _ => true,
        });
        if !flashback.is_empty() {
            self.ctx.move_all(flashback, Zone::Exile, Reason::Effect);
        }
        objects & !flashback
    }

    /// "Exile [objects]" (CR 701.13a): "move it to the exile zone from wherever it is."
    fn exile(&mut self, objects: Objects) -> Outcome<L> {
        let objects = self.elsewhere_than(objects, Some(Zone::Exile));
        if objects.is_empty() {
            return Outcome::NOTHING;
        }
        self.remember(objects);
        let rest = self.unstack(objects);
        self.ctx.move_all(rest, Zone::Exile, Reason::Exile);
        self.now_in(objects, Zone::Exile)
    }

    /// Puts the objects `objects` names into their owners' hands or libraries, at `place`
    /// in a library (CR 400.3): "If an object would go to any library, graveyard, or hand
    /// other than its owner's, it goes to its owner's corresponding zone."
    fn put(&mut self, objects: Objects, to: Zone, place: Place) -> Outcome<L> {
        let objects = self.elsewhere_than(objects, Some(to));
        if objects.is_empty() {
            return Outcome::NOTHING;
        }
        self.remember(objects);
        let rest = self.unstack(objects);
        let ctx = &mut *self.ctx;
        let entry = Entry::default();
        let moved = event::move_batch_to(
            ctx.state,
            ctx.table,
            ctx.sink,
            rest,
            to,
            place,
            Reason::Effect,
            entry,
        );
        ctx.check(moved);
        self.now_in(objects, to)
    }

    /// The outcome of an instruction that moved `objects` to `zone`: those that are there.
    fn now_in(&self, objects: Bitboard<L::Slot>, zone: Zone) -> Outcome<L> {
        let s = &*self.ctx.state;
        Outcome::acted_on(
            objects
                .iter()
                .filter(|&slot| s.object(slot).zone == zone)
                .collect(),
        )
    }

    /// "[Player] draws [count] cards" (CR 121.1). The outcome is the cards drawn.
    fn draw(&mut self, players: Players, count: u8) -> Outcome<L> {
        let mut drawn = Bitboard::EMPTY;
        for player in self.in_turn_order(players) {
            let hand = self.ctx.state.zone(player, Zone::Hand);
            self.ctx.draw(player, count);
            drawn |= self.ctx.state.zone(player, Zone::Hand) & !hand;
        }
        Outcome::acted_on(drawn)
    }

    /// "[Player] discards [count] cards", each of their choice (CR 701.9b), or their whole
    /// hand if it has fewer (CR 609.3).
    fn discard(&mut self, players: Players, count: u8) -> Result<Outcome<L>, crate::card::Pause> {
        let mut discarded = Bitboard::EMPTY;
        for player in self.in_turn_order(players) {
            for _ in 0..count {
                let Some(card) = self.ctx.discard_a_card(player)? else {
                    break;
                };
                discarded.insert(card.slot);
            }
        }
        Ok(Outcome::acted_on(discarded))
    }

    /// "[Player] mills [count] cards" (CR 701.17a).
    fn mill(&mut self, players: Players, count: u8) -> Outcome<L> {
        let mut milled = Bitboard::EMPTY;
        for player in self.in_turn_order(players) {
            let top = self.ctx.top_of_library(player, count);
            self.ctx.mill(player, count);
            milled |= top & !self.ctx.state.zone(player, Zone::Library);
        }
        Outcome::acted_on(milled)
    }

    /// "[Player] scries [count]" (CR 701.22a). CR 701.22b: "If a player is instructed to
    /// scry 0, no scry event occurs."
    fn scry(&mut self, players: Players, count: u8) -> Result<Outcome<L>, crate::card::Pause> {
        let mut looked = 0;
        for player in self.in_turn_order(players) {
            looked += self.ctx.top_of_library(player, count).count();
            self.ctx.scry(player, count)?;
        }
        Ok(Outcome::of(Bitboard::EMPTY, looked))
    }

    /// "[Player] gains [amount] life" or loses it, with `change`
    /// ([CR 119.3](https://yawgatog.com/resources/magic-rules/#R1193)): "that player's life
    /// total is adjusted accordingly." None of it is no event. The outcome is the life gained
    /// or lost.
    fn change_life(
        &mut self,
        players: Players,
        amount: u8,
        change: impl Fn(&mut EffectCtx<'a, L>, Seat, u8),
    ) -> Outcome<L> {
        if amount == 0 {
            return Outcome::NOTHING;
        }
        let mut total = 0;
        for player in self.in_turn_order(players) {
            let before = self.ctx.state.player(player).life;
            change(self.ctx, player, amount);
            total += u32::from(self.ctx.state.player(player).life.abs_diff(before));
        }
        Outcome::of(Bitboard::EMPTY, total)
    }

    /// "[Player] creates [count] [token] tokens" (CR 701.7a). The outcome is the tokens.
    fn create_tokens(
        &mut self,
        players: Players,
        token: &'static CardDef,
        count: u8,
    ) -> Outcome<L> {
        if count == 0 {
            return Outcome::NOTHING;
        }
        let mut tokens = Bitboard::EMPTY;
        for player in self.in_turn_order(players) {
            tokens |= self
                .ctx
                .create_tokens_for(player, token, count, TokenEntry::default());
        }
        Outcome::acted_on(tokens)
    }

    /// "[Player] creates [count] tokens that are copies of [object]"
    /// ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "the copy acquires
    /// the copiable values of the original object's characteristics". Those of the ability's
    /// own source are its card's once it has left (CR 608.2h), as offspring's "a copy of it"
    /// (CR 702.175a).
    fn create_copies(
        &mut self,
        players: Players,
        of: Objects,
        count: u8,
        except: CopyException,
    ) -> Outcome<L> {
        // CR 707.9b: "The final set of values for that characteristic becomes part of the
        // copiable values of the copy."
        let one_one = match except {
            CopyException::None => false,
            CopyException::PowerToughness(1, 1) => true,
            CopyException::PowerToughness(..) => no_primitive(&except),
        };
        if count == 0 {
            return Outcome::NOTHING;
        }
        let who = self.scene().players(players);
        let active = self.ctx.state.turn.active;
        let mut tokens = Bitboard::EMPTY;
        let mut copy = |ctx: &mut EffectCtx<'a, L>, card, how: TokenEntry| {
            let how = TokenEntry {
                one_one: how.one_one || one_one,
                ..how
            };
            for player in who.in_turn_order(active) {
                tokens |= ctx.create_tokens_of_for(player, card, count, how);
            }
        };
        let originals = self.permanents(of);
        if originals.is_empty() && matches!(of, Objects::This) {
            copy(self.ctx, self.ctx.entry.card, TokenEntry::default());
        }
        self.each_permanent(originals, |ctx, object| {
            let (card, how) = ctx.copiable_values(object);
            copy(ctx, card, how);
        });
        Outcome::acted_on(tokens)
    }

    /// "Put [count] [kind] counters on [objects]" (CR 122.1), or with `add` false, remove
    /// them, as many as each has (CR 609.3). The outcome is the counters put or removed.
    fn change_counters(
        &mut self,
        objects: Objects,
        kind: CounterKind,
        count: u8,
        add: bool,
    ) -> Outcome<L> {
        let permanents = self.permanents(objects);
        let counters = |run: &Self| -> u32 {
            let s = &*run.ctx.state;
            permanents
                .iter()
                .map(|slot| u32::from(s.object(slot).counters.get(kind)))
                .sum()
        };
        let before = counters(self);
        self.each_permanent(permanents, |ctx, object| {
            if add {
                ctx.add_counters(object, kind, count);
            } else {
                ctx.remove_counters(object, kind, count);
            }
        });
        Outcome::of(permanents, counters(self).abs_diff(before))
    }

    /// "[Player] adds [mana]" (CR 106.3): "A spell or ability that produces mana instructs a
    /// player to add that mana", of the kind its activator chose among those it produces.
    fn add_mana(&mut self, players: Players, mana: ManaSet) -> Outcome<L> {
        debug_assert!(mana.includes(self.ctx.mana));
        let mut added = 0;
        for player in self.in_turn_order(players) {
            self.ctx.add_mana(player, self.ctx.mana, mana.amount);
            added += u32::from(mana.amount);
        }
        Outcome::of(Bitboard::EMPTY, added)
    }

    /// "[Objects] get, gain or become [modification] [duration]"
    /// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)): a continuous effect
    /// from a resolving ability "lasts as long as stated by the spell or ability creating
    /// it". It affects the permanents that are there as it begins (CR 611.2c): "After that
    /// point, the set won't change."
    fn modify(
        &mut self,
        objects: Objects,
        modification: Modification,
        duration: Duration,
    ) -> Outcome<L> {
        let duration = match duration {
            Duration::EndOfTurn => state::Duration::EndOfTurn,
            Duration::UntilYourNextTurn => {
                state::Duration::UntilYourNextTurn(self.ctx.controller())
            }
            Duration::Indefinite => state::Duration::Indefinite,
        };
        let permanents = self.permanents(objects);
        self.each_permanent(permanents, |ctx, object| {
            ctx.add_effect(object, Continuous::Layer(modification), duration);
        });
        Outcome::acted_on(permanents)
    }
}

/// Stops on an entry the engine has no primitive for: no card can come to depend on one
/// before the engine can do it.
#[cold]
fn no_primitive(entry: &dyn std::fmt::Debug) -> ! {
    unimplemented!("the engine cannot do {entry:?} yet")
}
