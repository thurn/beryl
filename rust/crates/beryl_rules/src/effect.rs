//! `EffectCtx`: how a card body written before the rules library touches the game
//! (architecture §6.3). It is a shim: a [`Game`] and a [`Resolution`], and a method for each
//! thing a body did, which the library's functions replace one module at a time (the crate's
//! contract, "The shim"). No new code calls it.
//!
//! Every primitive builds its event and sends it through the pipeline of §8.2, so triggers,
//! replacement effects, layers and the sink see everything a body does. A primitive given a
//! reference to an object that has since changed zones does nothing to it
//! ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).

use crate::actions::{destroy, scry};
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, CardDef, CastingIx, CastingKind, FaceAbility, FaceIx, Flow, ManaColor,
    ManaCost, Part, Prompt, Purpose, Stop, Types,
};
use crate::chance;
use crate::decision::{DecisionKind, Opt};
use crate::event::{self, Entry, Reason, Recipient, Sink, TokenEntry};
use crate::filter::{Evaluate, Filter};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Small, index};
use crate::mana;
use crate::profile::Payment;
use crate::query::Copiable;
use crate::resolution::Resolution;
use crate::state::{
    CounterKind, Memo, ObjectRef, PendingTrigger, StackEntry, State, Target, TargetPlace,
    TargetPlaces, TriggerInfo, Zone, ZoneKey,
};
use crate::table::GameTable;
use crate::zone::Place;
use crate::{cast, face_down};
use crate::{counters, damage, library, life, tokens};
use crate::{layers, trigger};
use beryl_card::prompt;

mod ask;
mod continuous;
mod permit;

/// What a body runs against: the game, and the spell or ability resolving in it.
pub struct EffectCtx<'a, L: Layout = Small> {
    pub(crate) game: Game<'a, L>,
    pub(crate) resolution: Resolution<'a, L>,
    /// A fixed capacity ran out in a method that returns nothing to say so; the engine stalls
    /// once the body returns (§12.3). The library's functions return [`Full`] instead.
    full: bool,
}

impl<'a, L: Layout> EffectCtx<'a, L> {
    /// The context for a triggered ability's body, with no answer source: a question pauses
    /// it.
    pub(crate) fn new(
        state: &'a mut State<L>,
        table: &'a GameTable,
        sink: &'a mut Sink<L>,
        trigger: &PendingTrigger<L>,
        ability: &'static Ability,
    ) -> Self {
        let entry = StackEntry {
            object: trigger.source,
            controller: trigger.controller,
            card: trigger.card,
            face: trigger.face,
            ability: Some(FaceAbility::Own(trigger.ability)),
            info: trigger.info,
            ..StackEntry::default()
        };
        let resolution = Resolution::new(entry, ability, TargetPlaces::NONE, None, &[]);
        Self::resolving(Game::new(state, table, sink), resolution)
    }

    /// The context for the body of the spell or ability `resolution` is of.
    pub(crate) fn resolving(game: Game<'a, L>, resolution: Resolution<'a, L>) -> Self {
        Self {
            game,
            resolution,
            full: false,
        }
    }

    /// Ends the body, which came to `flow`, with a layer refresh for what follows. Returns
    /// whether it paused at a question, or [`Full`] if a capacity ran out.
    pub(crate) fn finish(self, flow: Flow) -> Result<bool, Full> {
        layers::refresh(self.game.state, self.game.table, self.game.sink)?;
        match flow {
            Err(Stop::Full) => Err(Full),
            _ if self.full => Err(Full),
            Ok(()) => Ok(false),
            Err(Stop::Pause) => Ok(true),
        }
    }

    pub(crate) fn check(&mut self, r: Result<(), Full>) {
        if r.is_err() {
            self.full = true;
        }
    }

    /// The spell's card or the ability's source.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn source(&self) -> ObjectRef<L> {
        self.resolution.source()
    }

    pub fn controller(&self) -> Seat {
        self.resolution.controller()
    }

    /// The state as it is now, to read.
    pub fn state(&self) -> &State<L> {
        self.game.state()
    }

    pub fn table(&self) -> &GameTable {
        self.game.table()
    }

    /// What a triggered ability triggered on, with last-known information for a look-back
    /// trigger.
    pub fn trigger(&self) -> TriggerInfo<L> {
        self.resolution.trigger()
    }

    /// The announced value of X.
    pub fn x(&self) -> u8 {
        self.resolution.x()
    }

    /// Whether the casting option at `option` in the face's list was paid.
    pub fn paid(&self, option: CastingIx) -> bool {
        self.resolution.paid(option)
    }

    /// [`Resolution::target`].
    pub fn target(&self, place: TargetPlace) -> Option<Target<L>> {
        self.resolution.target(&self.game, place)
    }

    /// [`Resolution::target_object`].
    pub fn target_object(&self, place: TargetPlace) -> Option<ObjectRef<L>> {
        self.resolution.target_object(&self.game, place)
    }

    /// [`Resolution::target_player`].
    pub fn target_player(&self, place: TargetPlace) -> Option<Seat> {
        self.resolution.target_player(&self.game, place)
    }

    /// The game index of a card or token the game's table holds.
    ///
    /// # Panics
    ///
    /// If the table lacks it: a card's tokens must be listed in its `tokens`.
    pub fn game_index(&self, def: &'static CardDef) -> L::Card {
        self.game
            .table
            .index_of(def)
            .expect("a token missing from its card's tokens")
    }

    /// Notes a capacity that ran out, so the engine stalls once the body returns (§12.3).
    fn checked<T: Default>(&mut self, r: Result<T, Full>) -> T {
        r.unwrap_or_else(|Full| {
            self.full = true;
            T::default()
        })
    }

    /// Counters a spell ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)):
    /// "To counter a spell or ability means to cancel it, removing it from the stack. It
    /// doesn't resolve and none of its effects occur. A countered spell is put into its
    /// owner's graveyard." With `exile`, it is exiled instead. Returns whether it was
    /// countered.
    pub fn counter(&mut self, spell: ObjectRef<L>, exile: bool) -> bool {
        let r = cast::counter(
            self.game.state,
            self.game.table,
            self.game.sink,
            spell,
            exile,
        );
        self.checked(r)
    }

    /// The controller of a spell on the stack.
    pub fn spell_controller(&self, spell: ObjectRef<L>) -> Option<Seat> {
        self.spell_entry(spell).map(|e| e.controller)
    }

    fn spell_entry(&self, spell: ObjectRef<L>) -> Option<&StackEntry<L>> {
        self.game
            .state
            .stack()
            .iter()
            .find(|e| e.spell() == Some(spell))
    }

    /// The mana value of a spell on the stack, X included
    /// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as the
    /// number chosen for it while the object is on the stack."
    pub fn spell_mana_value(&self, spell: ObjectRef<L>) -> Option<u8> {
        let e = self.spell_entry(spell)?;
        let cost = self.game.table.def(e.card).face(e.face).cost;
        Some(cost.mana_value().saturating_add(cost.x.saturating_mul(e.x)))
    }

    /// "[Effect] unless [player] pays [cost]"
    /// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "This means the
    /// same thing as '[A player may do something else]. If [that player doesn't], [do
    /// something].'" Asks `player` whether to pay if they can, and pays if they do. Returns
    /// whether they paid.
    ///
    /// [CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g): "If an effect gives a
    /// player the option to pay mana, they may activate mana abilities before taking that
    /// action."
    pub fn unless_pays(&mut self, player: Seat, cost: ManaCost) -> Result<bool, Stop> {
        let r = mana::payable(
            self.game.state,
            self.game.table,
            player,
            cost,
            Bitboard::EMPTY,
        );
        if !self.checked(r) {
            return Ok(false);
        }
        let prompt = Prompt::new(Purpose::UnlessPays).cost(cost);
        if !self.choose_yes_no(player, prompt, Bitboard::EMPTY)? {
            return Ok(false);
        }
        self.pay(player, cost)?;
        self.game.sink.note(event::Event::ResolutionPayment {
            player,
            item: self.resolution.entry.clone(),
        });
        Ok(true)
    }

    /// Pays a mana cost `player` can pay, under the profile's payment rules (§10).
    fn pay(&mut self, player: Seat, cost: ManaCost) -> Result<(), Stop> {
        if self.game.state.profile().payment == Payment::Manual {
            let mut remaining = cost;
            while !remaining.is_zero() {
                let r = mana::manual_options(
                    self.game.state,
                    self.game.table,
                    player,
                    remaining,
                    Bitboard::EMPTY,
                );
                let options = self.checked(r);
                let option = self.ask_options(
                    player,
                    DecisionKind::ManaPayment,
                    options.as_slice(),
                    Prompt::new(Purpose::Mana).cost(remaining),
                    Bitboard::EMPTY,
                )?;
                let r = mana::manual_pay(
                    self.game.state,
                    self.game.table,
                    self.game.sink,
                    player,
                    remaining,
                    option,
                );
                remaining = self.checked(r);
            }
            return Ok(());
        }
        let greedy = self.game.state.profile().payment == Payment::Greedy;
        let r = mana::plans(
            self.game.state,
            self.game.table,
            self.game.sink,
            player,
            cost,
            Bitboard::EMPTY,
            greedy,
        );
        let plans = self.checked(r);
        let mut opts: L::Queue<Opt<L>, 64> = Default::default();
        for i in 0..plans.len() {
            self.check(opts.push(Opt::Plan(index::<L>(i))));
        }
        let Opt::Plan(i) = self.ask_options(
            player,
            DecisionKind::PaymentPlan,
            opts.as_slice(),
            Prompt::new(Purpose::Mana).cost(cost).part(Part::Plan),
            Bitboard::EMPTY,
        )?
        else {
            unreachable!("a plan");
        };
        let r = mana::pay(
            self.game.state,
            self.game.table,
            self.game.sink,
            player,
            cost,
            Bitboard::EMPTY,
            &plans.as_slice()[Into::<usize>::into(i)],
        );
        self.checked(r);
        Ok(())
    }

    /// Lets `player` cast the card in `object`'s slot during this resolution, without paying
    /// its mana cost when `free`. The body's last action: the card is offered as it finishes
    /// resolving, which is when a spell cast during resolution would otherwise have
    /// mattered, since "no player receives priority after it's cast"
    /// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)).
    pub fn offer_cast(&mut self, object: ObjectRef<L>, player: Seat, free: bool) {
        self.resolution.offer = Some((object, player, free));
    }

    /// Adds mana to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool."
    pub fn add_mana(&mut self, player: Seat, color: ManaColor, amount: u8) {
        let source = self.resolution.entry.object;
        crate::interpret::add_mana(
            self.game.state,
            self.game.sink,
            player,
            source,
            color,
            amount,
        );
    }

    /// Deals noncombat damage from this ability's source.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn deal_damage(&mut self, to: Recipient<L>, amount: u8) {
        let source = self.source();
        let r = damage::deal(&mut self.game, source, to, amount, false);
        self.check(r);
    }

    pub fn gain_life(&mut self, player: Seat, amount: u8) {
        let r = life::gain(&mut self.game, player, amount);
        self.check(r);
    }

    pub fn lose_life(&mut self, player: Seat, amount: u8) {
        let r = life::lose(&mut self.game, player, amount);
        self.check(r);
    }

    pub fn draw(&mut self, player: Seat, cards: u8) {
        let r = event::draw(
            self.game.state,
            self.game.table,
            self.game.sink,
            player,
            cards,
        );
        self.check(r);
    }

    pub fn add_counters(&mut self, object: ObjectRef<L>, kind: CounterKind, amount: u8) {
        let r = counters::add(&mut self.game, object, kind, amount);
        self.check(r);
    }

    pub fn remove_counters(&mut self, object: ObjectRef<L>, kind: CounterKind, amount: u8) {
        let r = counters::remove(&mut self.game, object, kind, amount);
        self.check(r);
    }

    pub fn tap(&mut self, object: ObjectRef<L>) {
        let r = event::set_tapped(
            self.game.state,
            self.game.table,
            self.game.sink,
            object,
            true,
        );
        self.check(r);
    }

    pub fn untap(&mut self, object: ObjectRef<L>) {
        let r = event::set_tapped(
            self.game.state,
            self.game.table,
            self.game.sink,
            object,
            false,
        );
        self.check(r);
    }

    /// Moves an object to a zone, to the top of an ordered one.
    pub fn move_to(&mut self, object: ObjectRef<L>, to: Zone, reason: Reason) {
        if !self.game.state.is_current(object) {
            return;
        }
        let r = event::move_one(
            self.game.state,
            self.game.table,
            self.game.sink,
            object.slot,
            to,
            reason,
        );
        self.check(r);
    }

    /// Returns a spell or a permanent to its owner's hand
    /// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go
    /// to any library, graveyard, or hand other than its owner's, it goes to its owner's
    /// corresponding zone." A spell leaves the stack without resolving, or is exiled instead
    /// ([`event::exiled_instead`]).
    pub fn return_to_hand(&mut self, object: ObjectRef<L>) {
        if !self.game.state.is_current(object) {
            return;
        }
        let to = match self.game.state.object(object.slot).zone() {
            Zone::Battlefield => Some(Zone::Hand),
            Zone::Stack => cast::unstack(self.game.state, object, Zone::Hand),
            _ => None,
        };
        if let Some(to) = to {
            self.move_to(object, to, Reason::Effect);
        }
    }

    /// Puts a spell on top of its owner's library, or on the bottom unless `top`. It leaves
    /// the stack without resolving, or is exiled instead ([`event::exiled_instead`]).
    pub fn put_spell_into_library(&mut self, spell: ObjectRef<L>, top: bool) {
        if !self.game.state.is_current(spell) {
            return;
        }
        match cast::unstack(self.game.state, spell, Zone::Library) {
            Some(Zone::Library) => {}
            Some(to) => return self.move_to(spell, to, Reason::Effect),
            None => return,
        }
        let r = event::move_batch_to(
            self.game.state,
            self.game.table,
            self.game.sink,
            Bitboard::single(spell.slot),
            Zone::Library,
            if top { Place::Top } else { Place::Bottom },
            Reason::Effect,
            Entry::default(),
        );
        self.check(r);
    }

    /// Puts a permanent into its owner's library under `under` cards from the top, or at the
    /// bottom of a library with fewer, as Oust's "second from the top" is under one.
    pub fn put_into_library(&mut self, object: ObjectRef<L>, under: u8) {
        if !self.on_battlefield(object) {
            return;
        }
        let r = event::move_batch_to(
            self.game.state,
            self.game.table,
            self.game.sink,
            Bitboard::single(object.slot),
            Zone::Library,
            Place::UnderTop(under),
            Reason::Effect,
            Entry::default(),
        );
        self.check(r);
    }

    /// Moves a set of objects to one zone simultaneously, each to its owner's (CR 400.3).
    pub fn move_all(&mut self, objects: Bitboard<L::Slot>, to: Zone, reason: Reason) {
        let r = event::move_batch(
            self.game.state,
            self.game.table,
            self.game.sink,
            objects,
            to,
            reason,
            Entry::default(),
        );
        self.check(r);
    }

    /// Shuffles a player's library
    /// ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To shuffle a
    /// library or a face-down pile of cards, randomize the cards within it so that no player
    /// knows their order."
    pub fn shuffle(&mut self, player: Seat) {
        self.game.state.shuffle_library(player);
        self.game
            .sink
            .note(event::Event::LibraryShuffled { player });
    }

    /// Reveals a card to every player
    /// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
    /// show that card to all players for a brief time." Each player knows it from then on,
    /// until it is shuffled or otherwise hidden anew.
    pub fn reveal(&mut self, slot: L::Slot) {
        self.game.state.reveal(self.game.sink, slot);
    }

    /// A permanent explores ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)):
    /// "that permanent's controller reveals the top card of their library. If a land card is
    /// revealed this way, that player puts that card into their hand. Otherwise, that player
    /// puts a +1/+1 counter on the exploring permanent and may put the revealed card into
    /// their graveyard."
    ///
    /// [CR 701.44b](https://yawgatog.com/resources/magic-rules/#R70144b): it "explores ... even
    /// if some or all of those actions were impossible": with an empty library no land is
    /// revealed, so it gets the counter. [CR 701.44c](https://yawgatog.com/resources/magic-rules/#R70144c):
    /// "If a permanent is no longer on the battlefield as an effect instructs it to explore,
    /// its last known information is used to determine which object explored and who
    /// controlled it." The only explorer here is a target, which this ability's controller
    /// controls, so that is the controller once it is gone.
    pub fn explore(&mut self, object: ObjectRef<L>) -> Flow {
        let player = if self.on_battlefield(object) {
            self.game.state.controller(object.slot)
        } else {
            self.resolution.entry.controller
        };
        let top = self.game.state.library(player).last().copied();
        if let Some(slot) = top {
            self.reveal(slot);
            // A card in a library has its front face's characteristics (CR 712.6a).
            if self
                .game
                .state
                .copiable(slot)
                .face(self.game.table)
                .types
                .contains(Types::LAND)
            {
                self.move_to(self.game.state.object_ref(slot), Zone::Hand, Reason::Effect);
                return Ok(());
            }
        }
        if self.on_battlefield(object) {
            self.add_counters(object, CounterKind::PlusOne, 1);
        }
        // The card is revealed, on top of the library: a yes puts it into the graveyard.
        let prompt = Prompt::new(Purpose::Explore)
            .count(1)
            .places(prompt::Place::Graveyard, prompt::Place::Top);
        if let Some(slot) = top
            && self.choose_yes_no(player, prompt, Bitboard::single(slot))?
        {
            self.move_to(
                self.game.state.object_ref(slot),
                Zone::Graveyard,
                Reason::Effect,
            );
        }
        Ok(())
    }

    /// The top `n` cards of a player's library, or all of it if it has fewer.
    pub fn top_of_library(&self, player: Seat, n: u8) -> Bitboard<L::Slot> {
        library::top(self.game.state, player, n)
    }

    /// `player` looks at the cards in `cards` and from then on knows them (architecture §13):
    /// a library and a hand are hidden zones
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)), "zones in which not
    /// all players can be expected to see the cards' faces."
    pub fn look_at(&mut self, player: Seat, cards: Bitboard<L::Slot>) {
        self.game.state.look(self.game.sink, player, cards);
    }

    /// Discards a card from its owner's hand
    /// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
    /// move it from its owner's hand to that player's graveyard."
    pub fn discard(&mut self, card: ObjectRef<L>) {
        if self.game.state.is_current(card)
            && self.game.state.object(card.slot).zone() == Zone::Hand
        {
            self.move_to(card, Zone::Graveyard, Reason::Discard);
        }
    }

    /// Puts cards on the bottom of their owner's library in a random order. Their identities
    /// are dealt anew among the slots they land in, so no player knows which is where
    /// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "That library's owner
    /// doesn't reveal the order in which the cards go into the library."
    pub fn put_on_bottom_in_random_order(&mut self, cards: Bitboard<L::Slot>) {
        let r = event::move_batch_to(
            self.game.state,
            self.game.table,
            self.game.sink,
            cards,
            Zone::Library,
            Place::Bottom,
            Reason::Effect,
            Entry::default(),
        );
        self.check(r);
        // Rebound within each owner's library, among the slots that went there. Each takes
        // from the one random stream, by seat.
        for owner in self
            .game
            .state
            .players_in_seat_order(self.game.state.players())
        {
            let mut moved: L::Queue<L::Slot, 64> = Default::default();
            for &slot in self.game.state.library(owner) {
                if cards.contains(slot) && moved.push(slot).is_err() {
                    self.full = true;
                }
            }
            self.game.state.rebind_at_random(moved.as_slice());
        }
    }

    /// Puts a card in a library on the bottom of that library, as Jace, the Mind Sculptor's
    /// "You may put that card on the bottom of that player's library." It stays in the
    /// library, so it is still the same object.
    pub fn put_library_card_on_bottom(&mut self, object: ObjectRef<L>) {
        if self.game.state.is_current(object)
            && self.game.state.object(object.slot).zone() == Zone::Library
        {
            let r = self.game.state.reorder_library(object.slot, Place::Bottom);
            self.check(r);
        }
    }

    /// `player` scries `n`
    /// ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "To 'scry N' means
    /// to look at the top N cards of your library, then put any number of them on the bottom
    /// of your library in any order and the rest on top of your library in any order."
    ///
    /// The cards for the bottom are chosen one at a time, each going under the ones before,
    /// until the player is done; then those left are chosen one at a time from the top down.
    /// Every arrangement is one sequence of answers. [CR 701.22b](https://yawgatog.com/resources/magic-rules/#R70122b):
    /// "If a player is instructed to scry 0, no scry event occurs."
    pub fn scry(&mut self, player: Seat, n: u8) -> Flow {
        scry::scry(&mut self.game, &mut self.resolution, player, n)
    }

    /// `player` manifests dread
    /// ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): "'Manifest dread'
    /// means 'Look at the top two cards of your library. Manifest one of them, then put the
    /// cards you looked at that were not manifested this way into your graveyard.'" With one
    /// card in the library, that one is manifested; with none, nothing happens.
    pub fn manifest_dread(&mut self, player: Seat) -> Flow {
        let top = self.top_of_library(player, 2);
        if top.is_empty() {
            return Ok(());
        }
        self.look_at(player, top);
        let prompt = Prompt::new(Purpose::ManifestDread)
            .count(2)
            .most(1)
            .places(prompt::Place::Battlefield, prompt::Place::Graveyard);
        let chosen = self.choose_object(player, top, prompt)?;
        let r = face_down::manifest(
            self.game.state,
            self.game.table,
            self.game.sink,
            chosen,
            player,
        );
        self.check(r);
        let mut rest = top;
        rest.remove(chosen);
        self.move_all(rest, Zone::Graveyard, Reason::Effect);
        Ok(())
    }

    /// `player` mills `n` cards
    /// ([CR 701.17a](https://yawgatog.com/resources/magic-rules/#R70117a)): "For a player to
    /// mill a number of cards, that player puts that many cards from the top of their library
    /// into their graveyard." [CR 701.17b](https://yawgatog.com/resources/magic-rules/#R70117b):
    /// "A player can't mill a number of cards greater than the number of cards in their
    /// library."
    pub fn mill(&mut self, player: Seat, n: u8) {
        let top = self.top_of_library(player, n);
        self.move_all(top, Zone::Graveyard, Reason::Effect);
    }

    /// Exiles a permanent, then returns it to the battlefield transformed under its owner's
    /// control, as Jace, Vryn's Prodigy does. It returns as a new object
    /// ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)) with its back face up
    /// ([CR 712.11a](https://yawgatog.com/resources/magic-rules/#R71211a)): "If a spell or
    /// ability puts a double-faced card onto the battlefield 'transformed' or 'converted,' it
    /// enters the battlefield with its back face up." If it already left the battlefield, it
    /// isn't exiled, so it doesn't return.
    pub fn exile_and_return_transformed(&mut self, object: ObjectRef<L>) {
        if !self.on_battlefield(object) {
            return;
        }
        self.move_to(object, Zone::Exile, Reason::Exile);
        let o = *self.game.state.object(object.slot);
        if o.zone() != Zone::Exile {
            return;
        }
        let r = event::move_batch(
            self.game.state,
            self.game.table,
            self.game.sink,
            Bitboard::single(object.slot),
            Zone::Battlefield,
            Reason::Effect,
            Entry {
                controller: Some(o.owner()),
                face: FaceIx::BACK,
                ..Entry::default()
            },
        );
        self.check(r);
    }

    /// Destroys a permanent ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)):
    /// "To destroy a permanent, move it from the battlefield to its owner's graveyard."
    pub fn destroy(&mut self, object: ObjectRef<L>) {
        let r = destroy::destroy(&mut self.game, object);
        self.check(r);
    }

    /// Destroys a set of permanents simultaneously (CR 701.8a), as "Destroy each creature"
    /// does: one event, so "whenever one or more" abilities see them together.
    pub fn destroy_all(&mut self, objects: Bitboard<L::Slot>) {
        let r = destroy::destroy_all(&mut self.game, objects);
        self.check(r);
    }

    /// [`EffectCtx::search_library`] for up to `n` cards, as Winds of Abandon's "a basic land
    /// card" for each creature exiled: those found, still in the library, one
    /// [`DecisionKind::ChooseUpTo`] at a time. The caller shuffles once they have moved.
    pub fn search_library_up_to(
        &mut self,
        player: Seat,
        filter: Filter,
        n: u8,
    ) -> Result<Bitboard<L::Slot>, Stop> {
        let library = self.game.state.zone(ZoneKey::Of(player, Zone::Library));
        self.look_at(player, library);
        let found = filter.select(self.game.state, self.game.table, library, player);
        self.choose_up_to(player, found, n, Prompt::new(Purpose::Search).most(n))
    }

    /// Puts cards onto the battlefield tapped, each under its owner's control, simultaneously:
    /// "Those cards enter tapped."
    pub fn put_onto_battlefield_tapped(&mut self, cards: Bitboard<L::Slot>) {
        let r = event::move_batch(
            self.game.state,
            self.game.table,
            self.game.sink,
            cards,
            Zone::Battlefield,
            Reason::Effect,
            Entry {
                tapped: true,
                ..Entry::default()
            },
        );
        self.check(r);
    }

    /// Sacrifices a permanent ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)):
    /// "To sacrifice a permanent, its controller moves it from the battlefield directly to its
    /// owner's graveyard."
    pub fn sacrifice(&mut self, object: ObjectRef<L>) {
        if self.on_battlefield(object) {
            self.move_to(object, Zone::Graveyard, Reason::Sacrifice);
        }
    }

    fn on_battlefield(&self, object: ObjectRef<L>) -> bool {
        self.game.on_battlefield(object)
    }

    /// Exiles an object until this ability's source leaves the battlefield
    /// ([CR 610.3](https://yawgatog.com/resources/magic-rules/#R6103)): "Some one-shot effects
    /// cause an object to change zones 'until' a specified event occurs. A second one-shot
    /// effect is created immediately after the specified event. This second one-shot effect
    /// returns the object to its previous zone."
    ///
    /// [CR 610.3b](https://yawgatog.com/resources/magic-rules/#R6103b): "If a resolving
    /// triggered ability creates the initial one-shot effect that causes the object to change
    /// zones, and the specified event has already occurred before that one-shot effect would
    /// occur but after that ability triggered, the object doesn't move."
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn exile_until_source_leaves(&mut self, object: ObjectRef<L>) {
        let source = self.source();
        if !self.on_battlefield(source) || !self.on_battlefield(object) {
            return;
        }
        self.move_to(object, Zone::Exile, Reason::Exile);
        if self.game.state.object(object.slot).zone() == Zone::Exile {
            let r = self
                .game
                .state
                .remember(object.slot, Memo::ExiledUntil(source.slot));
            self.check(r);
        }
    }

    /// Exiles a card, linked to this ability's source so that the source's other abilities
    /// can find it among the cards "exiled with" it
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "If an object has an
    /// activated or triggered ability printed on it that instructs a player to exile one or
    /// more cards and an ability printed on it that refers either to "the exiled cards" or to
    /// cards "exiled with [this object]," these abilities are linked." A source that has left
    /// its zone since is gone, and so is the link: nothing can refer to it any more.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn exile_linked(&mut self, object: ObjectRef<L>) {
        if !self.game.state.is_current(object) {
            return;
        }
        let source = self.source();
        self.move_to(object, Zone::Exile, Reason::Exile);
        if self.game.state.is_current(source)
            && self.game.state.object(object.slot).zone() == Zone::Exile
        {
            let r = self
                .game
                .state
                .remember(object.slot, Memo::ExiledWith(source.slot));
            self.check(r);
        }
    }

    /// The cards exiled with this ability's source by [`Self::exile_linked`] (CR 607.2a),
    /// whether or not the source is still there: an ability of an object that has left the
    /// battlefield still refers to the cards that object exiled.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn exiled_with_source(&self) -> Bitboard<L::Slot> {
        self.game.state.exiled_with(self.source().slot)
    }

    /// `player` searches their library for a card that passes `filter`, and finds one of the
    /// cards that do, or none ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)):
    /// "To search for a card in a zone, look at all cards in that zone (even if it's a hidden
    /// zone) and find a card that matches the given description."
    /// [CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b): "If a player is
    /// searching a hidden zone for cards with a stated quality ... that player isn't required
    /// to find some or all of those cards even if they're present in that zone." Returns the
    /// card found, still in the library: the body says where it goes, whether it is revealed
    /// ([CR 701.23e](https://yawgatog.com/resources/magic-rules/#R70123e)), and shuffles.
    /// The question is a [`DecisionKind::ChooseUpTo`] whose [`Opt::Done`] finds nothing.
    pub fn search_library(
        &mut self,
        player: Seat,
        filter: Filter,
    ) -> Result<Option<L::Slot>, Stop> {
        Ok(self.search_library_up_to(player, filter, 1)?.sole())
    }

    /// Creates `count` tokens of a predefined kind under this ability's controller
    /// ([CR 111.10](https://yawgatog.com/resources/magic-rules/#R11110)).
    pub fn create_tokens(
        &mut self,
        def: &'static CardDef,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        self.create_tokens_for(self.resolution.entry.controller, def, count, how)
    }

    /// Creates `count` tokens of a predefined kind under `player`'s control, as in Get Lost's
    /// "Its controller creates two Map tokens."
    /// ([CR 111.2](https://yawgatog.com/resources/magic-rules/#R1112)): "The player who
    /// creates a token is its owner."
    pub fn create_tokens_for(
        &mut self,
        player: Seat,
        def: &'static CardDef,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        let what = tokens::predefined(self.game.table, def);
        self.tokens(player, what, count, how)
    }

    /// Creates `count` tokens that are copies of what has the copiable values `what`: a
    /// permanent's ([`State::copiable`]), or those of the card a last-known-information
    /// record names ([`Copiable::of_card`]). CR 707.2.
    pub fn create_tokens_of(
        &mut self,
        what: Copiable<L>,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        self.tokens(self.resolution.entry.controller, what, count, how)
    }

    /// Creates `count` tokens that are copies of what has the copiable values `what` under
    /// `player`'s control, as Fractured Identity's "Each player other than its controller
    /// creates a token that's a copy of it" (CR 111.2, 707.2).
    pub fn create_tokens_of_for(
        &mut self,
        player: Seat,
        what: Copiable<L>,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        self.tokens(player, what, count, how)
    }

    fn tokens(
        &mut self,
        player: Seat,
        what: Copiable<L>,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        let created = tokens::create(&mut self.game, player, what, count, how);
        self.checked(created)
    }

    /// Creates `count` tokens of a predefined kind under this ability's controller, tapped
    /// and attacking. As they enter, their controller chooses what they attack
    /// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): "If a creature is put
    /// onto the battlefield attacking, its controller chooses which defending player,
    /// planeswalker a defending player controls, or battle a defending player protects it's
    /// attacking as it enters the battlefield". Only the attacking player's creatures can be
    /// ([CR 506.3b](https://yawgatog.com/resources/magic-rules/#R5063b)): "If an effect would
    /// put a creature onto the battlefield attacking under the control of any player except an
    /// attacking player, that creature does enter the battlefield, but it's never considered to
    /// be an attacking creature." [CR 506.2](https://yawgatog.com/resources/magic-rules/#R5062):
    /// "During the combat phase, the active player is the attacking player".
    pub fn create_attacking_tokens(
        &mut self,
        def: &'static CardDef,
        count: u8,
    ) -> Result<Bitboard<L::Slot>, Stop> {
        let you = self.resolution.entry.controller;
        let attacking = crate::combat::in_combat(self.game.state.turn().step())
            && self.game.state.turn().active() == you;
        let tokens = self.create_tokens(
            def,
            count,
            TokenEntry {
                tapped: true,
                attacking,
            },
        );
        if attacking {
            // The choice is the controller's "as it enters", yet asked once the tokens are on
            // the battlefield, attacking the player, so they see what they choose for. Nothing
            // sees them in between: it is one step of the resolution, in which no player gets
            // priority and no ability triggers or state-based action is checked
            // ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c): "The
            // controller of the spell or ability follows its instructions in the order
            // written").
            let opts = self.checked(crate::combat::attack_targets(self.game.state));
            let Opt::Defender(walker) = self.ask_options(
                you,
                DecisionKind::AttackTarget,
                opts.as_slice(),
                Prompt::new(Purpose::Attack).part(Part::Aim),
                tokens,
            )?
            else {
                unreachable!("a defender")
            };
            if let Some(walker) = walker {
                // Each token is told what it attacks, with no event: the order is nothing.
                for slot in self.game.state.in_candidate_order(tokens) {
                    self.game.state.choose_attacked(slot, Some(walker));
                }
            }
        }
        Ok(tokens)
    }

    /// Gives a player an emblem ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)):
    /// "'[Player] gets an emblem with [ability].' This means that [player] puts an emblem with
    /// [ability] into the command zone. The emblem is both owned and controlled by that
    /// player."
    pub fn create_emblem(&mut self, player: Seat, def: &'static CardDef) {
        let card = self.game_index(def);
        let r = self.game.state.add_emblem(player, card);
        self.check(r);
    }

    /// Creates a delayed triggered ability: the ability at `ability` on this ability's card,
    /// triggering once at the step its delayed [`crate::card::Trigger`] names, controlled by
    /// this ability's controller, carrying `amount`
    /// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
    pub fn delay(&mut self, ability: AbilityIx, amount: u8) {
        let trigger = PendingTrigger {
            source: self.resolution.entry.object,
            card: self.resolution.entry.card,
            face: self.resolution.entry.face,
            ability,
            controller: self.resolution.entry.controller,
            info: TriggerInfo {
                amount,
                ..TriggerInfo::default()
            },
        };
        let r = trigger::delay(self.game.state, trigger);
        self.check(r);
    }

    /// `player` wins the game ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)):
    /// "An effect may state that a player wins the game." The game ends at once; the body
    /// asks nothing more.
    pub fn win_game(&mut self, player: Seat) {
        if self.game.state.outcome().is_none() {
            let cause = crate::event::LossCause::Effect {
                source: self.resolution.entry.object,
                card: self.resolution.entry.card,
                face: self.resolution.entry.face,
                ability: self.resolution.entry.ability,
            };
            crate::sba::win_game(self.game.state, self.game.sink, player, cause);
        }
    }

    /// This ability's controller untaps up to `n` lands of their choice, whoever controls
    /// them, as Teferi, Hero of Dominaria's "untap up to two lands". Untapping an untapped
    /// land does nothing ([CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b)):
    /// "Only tapped permanents can be untapped", so only tapped lands are offered.
    pub fn untap_up_to_lands(&mut self, n: u8) -> Flow {
        let you = self.resolution.entry.controller;
        let tapped = Filter::All(&[Filter::Types(Types::LAND), Filter::Tapped]);
        let s = &*self.game.state;
        let lands = tapped.select(s, self.game.table, s.in_zone(Zone::Battlefield), you);
        let chosen = self.choose_up_to(you, lands, n, Prompt::new(Purpose::Untap).most(n))?;
        // In battlefield order, never slot order: the untaps' events are seen in this order.
        let lands = self.game.state.in_game_order(self.game.table, chosen);
        for &slot in self.checked(lands).as_slice() {
            self.untap(self.game.state.object_ref(slot));
        }
        Ok(())
    }

    /// Flips a coin and returns whether the player won the flip
    /// ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "the player that flips
    /// the coin calls 'heads' or 'tails.' If the call matches the result, the player wins the
    /// flip." A fair coin against any call is a fair win, so one draw from the game's
    /// randomness decides it (architecture §14), or while a save replays, its record
    /// ([`chance`]).
    pub fn flip_coin(&mut self, player: Seat) -> bool {
        let won = chance::flip(self.game.state, self.game.sink, player);
        let source = self.source();
        let r = event::settle(
            self.game.state,
            self.game.table,
            self.game.sink,
            event::Event::CoinFlip {
                player,
                won,
                source,
            },
        );
        self.check(r);
        won
    }

    /// Puts a permanent card onto the battlefield under this ability's controller, with the
    /// face up that it enters with (a double-faced card returned "transformed" enters with
    /// its back face up) and any counters it enters with.
    pub fn put_onto_battlefield(
        &mut self,
        object: ObjectRef<L>,
        face: FaceIx,
        counters: Option<(CounterKind, u8)>,
    ) {
        if !self.game.state.is_current(object) {
            return;
        }
        let r = event::move_batch(
            self.game.state,
            self.game.table,
            self.game.sink,
            Bitboard::single(object.slot),
            Zone::Battlefield,
            Reason::Effect,
            Entry {
                controller: Some(self.resolution.entry.controller),
                face,
                counters,
                ..Entry::default()
            },
        );
        self.check(r);
    }
}

/// "If it was kicked" ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)):
/// "If a spell's controller declares the intention to pay any of that spell's kicker costs,
/// that spell has been 'kicked.'"
pub fn kicked<L: Layout>(s: &State<L>, _: &GameTable, tr: &PendingTrigger<L>) -> bool {
    trigger::entered_with(s, tr, CastingKind::Kicker)
}
