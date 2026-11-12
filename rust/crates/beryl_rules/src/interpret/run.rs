//! Carrying out effect data's instructions. [`Run::carry_out`] is the table of them: an
//! entry whose library function is written has its arm in that function's module, named
//! `instruction` (the crate's contract, "Modules"), and the others are still done here through
//! the [`EffectCtx`] method a Rust body would call, until their modules are written.
//!
//! An instruction acts on every object it names at once where the engine has a primitive
//! that does, and otherwise on each permanent in the order they entered the battlefield, an
//! order the game can see (slots are none: the deal puts cards in them at random).

use beryl_card::{
    CopyException, Duration, Effect, LibraryPosition, Objects, Players, Prompt, Purpose,
};

use crate::actions::{destroy, scry};
use crate::bitboard::Bitboard;
use crate::card::{AbilityIx, FaceAbility, Flow, ManaCost, ManaSet, Modification, Stop};
use crate::effect::EffectCtx;
use crate::event::{self, Entry, Reason, TokenEntry};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::scene::{Outcome, Registers, Scene};
use crate::layout::{Layout, Queue};
use crate::query::Copiable;
use crate::resolution::Resolution;
use crate::state::{
    self, LayerChange, ObjectRef, PendingTrigger, StackEntry, State, Zone, ZoneKey, placed,
};
use crate::zone::Place;
use crate::{counters, life, tokens};

/// The yes or no of an [`Effect::May`], about nothing but the effect.
const MAY: Prompt = Prompt::new(Purpose::May);

/// One resolution of an ability written as data: its context, and what it keeps between
/// instructions.
pub(crate) struct Run<'c, 'a, L: Layout> {
    ctx: &'c mut EffectCtx<'a, L>,
    subject: PendingTrigger<L>,
    registers: Registers<L>,
}

impl<'c, 'a, L: Layout> Run<'c, 'a, L> {
    /// Begins resolving the ability `ctx` was made for.
    pub(crate) fn new(ctx: &'c mut EffectCtx<'a, L>) -> Self {
        let legal = ctx.resolution.legal;
        let entry = &ctx.resolution.entry;
        let subject = PendingTrigger {
            source: entry.object,
            card: entry.card,
            face: entry.face,
            ability: match entry.ability {
                Some(FaceAbility::Own(index)) => index,
                _ => AbilityIx::default(),
            },
            controller: entry.controller,
            info: entry.info,
        };
        Run {
            ctx,
            subject,
            registers: Registers {
                legal,
                ..Registers::default()
            },
        }
    }

    /// The game the ability resolves in, to change.
    pub(crate) fn game(&mut self) -> &mut Game<'a, L> {
        &mut self.ctx.game
    }

    /// The game and the resolution, for a library function that asks.
    pub(crate) fn resolving(&mut self) -> (&mut Game<'a, L>, &mut Resolution<'a, L>) {
        (&mut self.ctx.game, &mut self.ctx.resolution)
    }

    /// The state as it is now, to read.
    pub(crate) fn state(&self) -> &State<L> {
        self.ctx.game.state
    }

    /// What the ability's expressions are read against, as the game is now.
    pub(crate) fn scene(&self) -> Scene<'_, L> {
        let entry = &self.ctx.resolution.entry;
        Scene {
            s: self.ctx.game.state,
            t: self.ctx.game.table,
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
    pub(crate) fn carry_out(&mut self, effect: &'static Effect) -> Flow {
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
                    Some(player) => self.ctx.choose_yes_no(player, MAY, Bitboard::EMPTY)?,
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
            Effect::Destroy(objects) => destroy::instruction(self, objects)?,
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
            Effect::Scry { player, count } => scry::instruction(self, player, count)?,
            Effect::GainLife { player, amount } => life::gain_instruction(self, player, amount)?,
            Effect::LoseLife { player, amount } => life::lose_instruction(self, player, amount)?,
            Effect::CreateTokens {
                player,
                token,
                count,
            } => tokens::create_instruction(self, player, token, count)?,
            Effect::CreateCopies {
                player,
                of,
                count,
                except,
            } => self.create_copies(player, of, self.scene().amount(count), except)?,
            Effect::AddCounters {
                objects,
                kind,
                count,
            } => counters::add_instruction(self, objects, kind, count)?,
            Effect::RemoveCounters {
                objects,
                kind,
                count,
            } => counters::remove_instruction(self, objects, kind, count)?,
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
    pub(crate) fn in_turn_order(&self, players: Players) -> impl Iterator<Item = Seat> + use<L> {
        let who = self.scene().players(players);
        self.ctx.game.state.players_in_turn_order(who)
    }

    /// The one player who makes a choice the effect offers `players`.
    fn one(&self, players: Players) -> Option<Seat> {
        self.in_turn_order(players).next()
    }

    /// "Unless [player] pays [cost]" (CR 118.12a): whether they paid.
    fn unless_pays(&mut self, player: Players, cost: ManaCost) -> Result<bool, Stop> {
        match self.one(player) {
            Some(player) => self.ctx.unless_pays(player, cost),
            None => Ok(false),
        }
    }

    /// The permanents among the objects `objects` names
    /// ([CR 110.1](https://yawgatog.com/resources/magic-rules/#R1101)): "A permanent is a
    /// card or token on the battlefield."
    pub(crate) fn permanents(&self, objects: Objects) -> Bitboard<L::Slot> {
        let s = &*self.ctx.game.state;
        (self.scene().objects(objects)).filter(|slot| s.object(slot).zone() == Zone::Battlefield)
    }

    /// The objects `objects` names that are not in `zone` already.
    fn elsewhere_than(&self, objects: Objects, zone: Zone) -> Bitboard<L::Slot> {
        let s = &*self.ctx.game.state;
        (self.scene().objects(objects)).filter(|slot| s.object(slot).zone() != zone)
    }

    /// Notes what is known of the targets among `leaving` before the effect moves them, for
    /// the instructions after (CR 608.2h): "the effect uses the object's last known
    /// information."
    pub(crate) fn remember(&mut self, leaving: Bitboard<L::Slot>) {
        for (place, target) in placed(self.ctx.resolution.entry.targets.as_slice()) {
            let Some(object) = target.as_object() else {
                continue;
            };
            if self.registers.legal.contains(place)
                && self.ctx.game.state.is_current(object)
                && leaving.contains(object.slot)
            {
                self.registers.known[place.index()] = self.scene().known(object.slot);
                self.registers.noted.insert(place);
            }
        }
    }

    /// The outcome of an instruction that moved `permanents` off the battlefield: those that
    /// left it, as they are now
    /// ([CR 400.7j](https://yawgatog.com/resources/magic-rules/#R4007j)): "If an effect
    /// causes an object to move to a public zone, other parts of that effect can find that
    /// object."
    pub(crate) fn left_the_battlefield(&self, permanents: Bitboard<L::Slot>) -> Outcome<L> {
        let s = &*self.ctx.game.state;
        Outcome::acted_on(permanents.filter(|slot| s.object(slot).zone() != Zone::Battlefield))
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

    /// The first of `permanents`, in the order they entered the battlefield, that is still
    /// one, taken out of the set: how an instruction acts on each of several in an order the
    /// game can see, whatever the ones before did.
    pub(crate) fn next_permanent(
        &self,
        permanents: &mut Bitboard<L::Slot>,
    ) -> Option<ObjectRef<L>> {
        let s = self.state();
        let &slot = (s.battlefield().iter()).find(|&&slot| permanents.contains(slot))?;
        permanents.remove(slot);
        Some(s.object_ref(slot))
    }

    /// Does `act` to each of `permanents` that is still one, in the order they entered the
    /// battlefield.
    fn each_permanent(
        &mut self,
        mut permanents: Bitboard<L::Slot>,
        mut act: impl FnMut(&mut EffectCtx<'a, L>, ObjectRef<L>),
    ) {
        while let Some(object) = self.next_permanent(&mut permanents) {
            act(self.ctx, object);
        }
    }

    /// The topmost spell on the stack among `spells`.
    fn top_spell(&self, spells: Bitboard<L::Slot>) -> Option<ObjectRef<L>> {
        let stack = self.ctx.game.state.stack().iter().rev();
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
    /// to `to` without their resolving, and exiles those that are exiled instead
    /// ([`crate::event::exiled_instead`]). Returns the objects still to move.
    fn unstack(&mut self, objects: Bitboard<L::Slot>, to: Zone) -> Bitboard<L::Slot> {
        let mut flashback = Bitboard::EMPTY;
        let spells = objects & crate::cast::spells(self.ctx.game.state);
        // Each spell's entry comes off on its own, with no event, and the spells left on the
        // stack keep their order, so the order these come off in is nothing.
        for slot in self.ctx.game.state.in_candidate_order(spells) {
            let spell = self.ctx.game.state.object_ref(slot);
            let taken = self.ctx.game.state.take_off_stack(spell).is_some();
            if taken && crate::event::exiled_instead(self.ctx.game.state, spell, to) {
                flashback.insert(slot);
            }
        }
        if !flashback.is_empty() {
            self.ctx.move_all(flashback, Zone::Exile, Reason::Effect);
        }
        objects & !flashback
    }

    /// "Exile [objects]" (CR 701.13a): "move it to the exile zone from wherever it is."
    fn exile(&mut self, objects: Objects) -> Outcome<L> {
        let objects = self.elsewhere_than(objects, Zone::Exile);
        if objects.is_empty() {
            return Outcome::NOTHING;
        }
        self.remember(objects);
        let rest = self.unstack(objects, Zone::Exile);
        self.ctx.move_all(rest, Zone::Exile, Reason::Exile);
        self.now_in(objects, Zone::Exile)
    }

    /// Puts the objects `objects` names into their owners' hands or libraries, at `place`
    /// in a library (CR 400.3): "If an object would go to any library, graveyard, or hand
    /// other than its owner's, it goes to its owner's corresponding zone."
    fn put(&mut self, objects: Objects, to: Zone, place: Place) -> Outcome<L> {
        let objects = self.elsewhere_than(objects, to);
        if objects.is_empty() {
            return Outcome::NOTHING;
        }
        self.remember(objects);
        let rest = self.unstack(objects, to);
        let ctx = &mut *self.ctx;
        let entry = Entry::default();
        let moved = event::move_batch_to(
            ctx.game.state,
            ctx.game.table,
            ctx.game.sink,
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
        let s = &*self.ctx.game.state;
        Outcome::acted_on(objects.filter(|slot| s.object(slot).zone() == zone))
    }

    /// "[Player] draws [count] cards" (CR 121.1). The outcome is the cards drawn.
    fn draw(&mut self, players: Players, count: u8) -> Outcome<L> {
        let mut drawn = Bitboard::EMPTY;
        for player in self.in_turn_order(players) {
            let hand = self.ctx.game.state.zone(ZoneKey::Of(player, Zone::Hand));
            self.ctx.draw(player, count);
            drawn |= self.ctx.game.state.zone(ZoneKey::Of(player, Zone::Hand)) & !hand;
        }
        Outcome::acted_on(drawn)
    }

    /// "[Player] discards [count] cards", each of their choice (CR 701.9b), or their whole
    /// hand if it has fewer (CR 609.3).
    fn discard(&mut self, players: Players, count: u8) -> Result<Outcome<L>, Stop> {
        let mut discarded = Bitboard::EMPTY;
        for player in self.in_turn_order(players) {
            discarded |= self.ctx.discard_cards(player, count)?;
        }
        Ok(Outcome::acted_on(discarded))
    }

    /// "[Player] mills [count] cards" (CR 701.17a).
    fn mill(&mut self, players: Players, count: u8) -> Outcome<L> {
        let mut milled = Bitboard::EMPTY;
        for player in self.in_turn_order(players) {
            let top = self.ctx.top_of_library(player, count);
            self.ctx.mill(player, count);
            milled |= top & !self.ctx.game.state.zone(ZoneKey::Of(player, Zone::Library));
        }
        Outcome::acted_on(milled)
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
    ) -> Result<Outcome<L>, Full> {
        // CR 707.9b: "The final set of values for that characteristic becomes part of the
        // copiable values of the copy."
        let excepted = |what: Copiable<L>| match except {
            CopyException::None => what,
            CopyException::PowerToughness(1, 1) => what.except_one_one(),
            CopyException::PowerToughness(..) => no_primitive(&except),
        };
        if count == 0 {
            return Ok(Outcome::NOTHING);
        }
        let who = self.scene().players(players);
        let mut created = Bitboard::EMPTY;
        let mut copy = |game: &mut Game<'a, L>, what: Copiable<L>| -> Result<(), Full> {
            for player in game.state.players_in_turn_order(who) {
                let how = TokenEntry::default();
                created |= tokens::create(game, player, excepted(what), count, how)?;
            }
            Ok(())
        };
        let mut originals = self.permanents(of);
        if originals.is_empty() && matches!(of, Objects::This) {
            let card = self.ctx.resolution.entry.card;
            copy(self.game(), Copiable::of_card(card))?;
        }
        while let Some(object) = self.next_permanent(&mut originals) {
            let what = self.state().copiable(object.slot);
            copy(self.game(), what)?;
        }
        Ok(Outcome::acted_on(created))
    }

    /// "[Player] adds [mana]" (CR 106.3): "A spell or ability that produces mana instructs a
    /// player to add that mana", of the kind its activator chose among those it produces.
    fn add_mana(&mut self, players: Players, mana: ManaSet) -> Outcome<L> {
        debug_assert!(mana.includes(self.ctx.resolution.mana));
        let mut added = 0;
        for player in self.in_turn_order(players) {
            self.ctx
                .add_mana(player, self.ctx.resolution.mana, mana.amount);
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
        // "You" in what it says is the ability's controller as it resolves.
        let change = LayerChange::resolve(modification, self.ctx.controller());
        // One effect, with one timestamp, on the set as it is now
        // ([CR 613.7](https://yawgatog.com/resources/magic-rules/#R6137): "An effect with an
        // earlier timestamp is applied before an effect with a later timestamp.").
        let permanents = self.permanents(objects);
        self.ctx.add_effect_on_set(permanents, change, duration);
        Outcome::acted_on(permanents)
    }
}

/// Stops on an entry the engine has no primitive for: no card can come to depend on one
/// before the engine can do it.
#[cold]
fn no_primitive(entry: &dyn std::fmt::Debug) -> ! {
    unimplemented!("the engine cannot do {entry:?} yet")
}
