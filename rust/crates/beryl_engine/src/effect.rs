//! `EffectCtx`: the only way a card body touches the state (architecture §6.3).
//!
//! Every primitive builds its event and sends it through the pipeline of §8.2, so triggers,
//! replacement effects, layers and the sink see everything a body does. A primitive given a
//! reference to an object that has since changed zones does nothing to it
//! ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).

use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, FaceAbility, FaceIx, Flow, ManaColor, ManaCost, Pause, Types};
use crate::chance;
use crate::decision::{Answers, Ask, Candidates, Choice, Decision, DecisionKind, Opt, StallReason};
use crate::event::{self, Entry, Reason, Recipient, Sink, TokenEntry};
use crate::filter::{Evaluate, Filter};
use crate::fixed::Full;
use crate::ids::{CardIx, Seat};
use crate::layout::{Layout, Queue, Small, index};
use crate::mana;
use crate::observation::Observation;
use crate::profile::{Payment, Stops};
use crate::state::{
    CastPermission, Continuous, CounterKind, Duration, FloatingEffect, ObjectFlags, ObjectRef,
    PendingTrigger, StackEntry, State, Target, TriggerInfo, Zone,
};
use crate::table::GameTable;
use crate::zone::{self, Place};
use crate::{cast, face_down};
use crate::{layers, trigger};

/// What a body runs against: the working state and tables, the event sink, the stack entry
/// resolving (its source, controller, targets, X and what triggered it), and where its
/// questions are answered (§3, §7.3).
pub struct EffectCtx<'a, L: Layout = Small> {
    pub(crate) state: &'a mut State<L>,
    pub(crate) table: &'a GameTable,
    pub(crate) sink: &'a mut Sink<L>,
    pub(crate) entry: StackEntry<L>,
    pub(crate) ability: &'static Ability,
    /// Where questions go: an inline source answers them, anything else pauses the body.
    answers: Option<&'a mut dyn Answers<L>>,
    /// The answers given so far to this resolution's questions, and how many this run of
    /// the body has used.
    record: &'a [L::Index],
    used: usize,
    /// The question that paused this run.
    pub(crate) question: Option<Decision<L>>,
    /// For a mana ability that can produce more than one kind of mana, the kind its
    /// activator chose.
    pub(crate) mana: ManaColor,
    /// A card the body lets a player cast as it finishes resolving (CR 608.2g).
    pub(crate) offer: Option<(ObjectRef<L>, Seat, bool)>,
    /// A fixed capacity ran out; the engine stalls once the body returns (§12.3).
    pub(crate) full: bool,
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
        Self::resolving(state, table, sink, None, &[], entry, ability)
    }

    /// The context for the body of the spell or ability `entry`, answering from `record`
    /// and then `answers`.
    pub(crate) fn resolving(
        state: &'a mut State<L>,
        table: &'a GameTable,
        sink: &'a mut Sink<L>,
        answers: Option<&'a mut dyn Answers<L>>,
        record: &'a [L::Index],
        entry: StackEntry<L>,
        ability: &'static Ability,
    ) -> Self {
        Self {
            state,
            table,
            sink,
            entry,
            ability,
            answers,
            record,
            used: 0,
            question: None,
            mana: ManaColor::Colorless,
            offer: None,
            full: false,
        }
    }

    /// Ends the body: a stall if a capacity ran out, and a layer refresh for what follows.
    pub(crate) fn finish(self) -> Result<(), Full> {
        layers::refresh(self.state, self.table, self.sink);
        if self.full { Err(Full) } else { Ok(()) }
    }

    fn check(&mut self, r: Result<(), Full>) {
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
        self.entry.object.expect("an ability with a source")
    }

    pub fn controller(&self) -> Seat {
        self.entry.controller
    }

    /// The state as it is now, to read.
    pub fn state(&self) -> &State<L> {
        self.state
    }

    pub fn table(&self) -> &GameTable {
        self.table
    }

    /// What a triggered ability triggered on, with last-known information for a look-back
    /// trigger.
    pub fn trigger(&self) -> TriggerInfo<L> {
        self.entry.info
    }

    /// The announced value of X.
    pub fn x(&self) -> u8 {
        self.entry.x
    }

    /// Whether the casting option at `index` in the face's list was paid.
    pub fn paid(&self, index: u8) -> bool {
        self.entry.costs.contains(usize::from(index))
    }

    /// The `i`th chosen target if it is still legal, `None` if it was not chosen or is
    /// illegal now ([CR 608.2b](https://yawgatog.com/resources/magic-rules/#R6082b)): "Illegal
    /// targets, if any, won't be affected by parts of a resolving spell's effect for which
    /// they're illegal."
    pub fn target(&self, i: usize) -> Option<Target<L>> {
        let target = *self.entry.targets.as_slice().get(i)?;
        cast::still_legal(self.state, self.table, &self.entry, self.ability, target)
            .then_some(target)
    }

    /// The `i`th target as an object, if it is one and still legal.
    pub fn target_object(&self, i: usize) -> Option<ObjectRef<L>> {
        self.target(i).and_then(Target::as_object)
    }

    /// The `i`th target as a player, if it is one and still legal.
    pub fn target_player(&self, i: usize) -> Option<Seat> {
        self.target(i).and_then(Target::as_player)
    }

    /// The game index of a card or token the game's table holds.
    ///
    /// # Panics
    ///
    /// If the table lacks it: a card's tokens must be listed in its `tokens`.
    pub fn game_index(&self, def: &'static CardDef) -> CardIx {
        self.table
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

    fn stall(&mut self, reason: StallReason) {
        self.state.outcome = Some(crate::decision::Outcome::Stalled(reason));
    }

    /// Counters a spell ([CR 701.6a](https://yawgatog.com/resources/magic-rules/#R7016a)):
    /// "To counter a spell or ability means to cancel it, removing it from the stack. It
    /// doesn't resolve and none of its effects occur. A countered spell is put into its
    /// owner's graveyard." With `exile`, it is exiled instead. Returns whether it was
    /// countered.
    pub fn counter(&mut self, spell: ObjectRef<L>, exile: bool) -> bool {
        let r = cast::counter(self.state, self.table, self.sink, spell, exile);
        self.checked(r)
    }

    /// The controller of a spell on the stack.
    pub fn spell_controller(&self, spell: ObjectRef<L>) -> Option<Seat> {
        self.spell_entry(spell).map(|e| e.controller)
    }

    fn spell_entry(&self, spell: ObjectRef<L>) -> Option<&StackEntry<L>> {
        self.state.stack().iter().find(|e| e.spell() == Some(spell))
    }

    /// The mana value of a spell on the stack, X included
    /// ([CR 202.3e](https://yawgatog.com/resources/magic-rules/#R2023e)): "X is treated as the
    /// number chosen for it while the object is on the stack."
    pub fn spell_mana_value(&self, spell: ObjectRef<L>) -> Option<u8> {
        let e = self.spell_entry(spell)?;
        let cost = self.table.def(e.card).face(e.face).cost;
        Some(cost.mana_value().saturating_add(cost.x.saturating_mul(e.x)))
    }

    /// Asks a question (§7.3). A question with one answer is answered silently, except under
    /// [`Stops::Forced`]. Otherwise the choice record answers it, or an inline source does, or
    /// the body pauses here. Once the game is over, as when a player won it mid-resolution,
    /// the body asks nothing more.
    fn ask(&mut self, d: &Decision<L>) -> Result<Choice, Pause> {
        debug_assert!(
            self.ability.may_pause,
            "a body asked a question without declaring may_pause"
        );
        debug_assert!(!d.is_empty(), "a question with no answer");
        if self.state.outcome.is_some() {
            return Err(Pause);
        }
        if d.len() == 1 && self.state.profile.stops != Stops::Forced {
            return Ok(Choice(0));
        }
        if let Some(&c) = self.record.get(self.used) {
            self.used += 1;
            return Ok(Choice(c.into()));
        }
        if let Some(answers) = self.answers.as_deref_mut()
            && answers.inline()
        {
            // An inline answer is a decision like any other, so it counts toward the budget
            // as a paused question would when `next` returns it.
            if self.state.budgets.decisions >= self.state.profile.limits.decisions_per_turn {
                self.stall(StallReason::DecisionsPerTurn);
                return Err(Pause);
            }
            self.state.budgets.decisions += 1;
            let answer = answers.answer(Observation::new(self.state, d.player), d);
            debug_assert!(
                answer.is_some(),
                "an inline answer source declined to answer"
            );
            if let Some(c) = answer {
                return Ok(c);
            }
        }
        self.question = Some(d.clone());
        Err(Pause)
    }

    fn ask_options(
        &mut self,
        player: Seat,
        kind: DecisionKind,
        opts: &[Opt<L>],
        ask: Ask<L>,
    ) -> Result<Opt<L>, Pause> {
        let mut list: L::Queue<Opt<L>, 64> = Default::default();
        for &o in opts {
            if list.push(o).is_err() {
                self.full = true;
                return Err(Pause);
            }
        }
        let d = Decision {
            player,
            kind,
            candidates: Candidates::Options(list),
            ask,
        };
        let c = self.ask(&d)?;
        Ok(d.candidates.option(c).expect("an answer among the options"))
    }

    /// Asks `player` yes or no, for the purpose `ask`.
    pub fn choose_yes_no(&mut self, player: Seat, ask: Ask<L>) -> Result<bool, Pause> {
        Ok(self.ask_options(player, DecisionKind::YesNo, &[Opt::Yes, Opt::No], ask)? == Opt::Yes)
    }

    /// Asks `player` to choose up to `n` of `candidates`, one at a time, each answer either one
    /// more object or [`Opt::Done`], for the purpose `ask`. Interchangeable candidates are one
    /// answer (§7.2).
    pub fn choose_up_to(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        n: u8,
        ask: Ask<L>,
    ) -> Result<Bitboard<L::Slot>, Pause> {
        let mut chosen = Bitboard::EMPTY;
        while chosen.count() < u32::from(n) && !(candidates & !chosen).is_empty() {
            match self.choose_or_done(player, candidates & !chosen, ask)? {
                Some(slot) => chosen.insert(slot),
                None => break,
            }
        }
        Ok(chosen)
    }

    /// Asks `player` for one more of `candidates`, as an [`Opt::Object`], or [`Opt::Done`].
    /// Interchangeable candidates are one answer (§7.2).
    fn choose_or_done(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        ask: Ask<L>,
    ) -> Result<Option<L::Slot>, Pause> {
        let mut opts: L::Queue<Opt<L>, 64> = Default::default();
        let left = crate::engine::collapse(self.state, candidates);
        for o in core::iter::once(Opt::Done).chain(left.iter().map(Opt::Object)) {
            if opts.push(o).is_err() {
                self.full = true;
                return Err(Pause);
            }
        }
        Ok(
            match self.ask_options(player, DecisionKind::ChooseUpTo, opts.as_slice(), ask)? {
                Opt::Object(slot) => Some(slot),
                _ => None,
            },
        )
    }

    /// Asks `player` to choose one of `candidates`, which must not be empty, for the purpose
    /// `ask`.
    pub fn choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        ask: Ask<L>,
    ) -> Result<L::Slot, Pause> {
        self.ask_object(player, DecisionKind::ChooseObject, candidates, ask)
    }

    fn ask_object(
        &mut self,
        player: Seat,
        kind: DecisionKind,
        candidates: Bitboard<L::Slot>,
        ask: Ask<L>,
    ) -> Result<L::Slot, Pause> {
        let d = Decision {
            player,
            kind,
            candidates: Candidates::Objects(crate::engine::collapse(self.state, candidates)),
            ask,
        };
        let c = self.ask(&d)?;
        Ok(d.candidates.object(c).expect("an answer among the objects"))
    }

    /// `player` discards a card of their choice
    /// ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)): "By default, effects
    /// that cause a player to discard a card allow the affected player to choose which card
    /// to discard." Returns the card in the graveyard, or `None` from an empty hand.
    pub fn discard_a_card(&mut self, player: Seat) -> Result<Option<ObjectRef<L>>, Pause> {
        self.discard_one_of(player, self.state.zone(player, Zone::Hand))
    }

    /// `player` discards one of `cards`, cards in their hand, of their choice, as Thirst for
    /// Discovery's "discard a basic land card" (CR 701.9b). Returns the card in the
    /// graveyard, or `None` when there is none to discard.
    pub fn discard_one_of(
        &mut self,
        player: Seat,
        cards: Bitboard<L::Slot>,
    ) -> Result<Option<ObjectRef<L>>, Pause> {
        debug_assert!((cards & !self.state.zone(player, Zone::Hand)).is_empty());
        if cards.is_empty() {
            return Ok(None);
        }
        let slot = self.ask_object(player, DecisionKind::Discard, cards, Ask::Plain)?;
        self.discard(self.state.object_ref(slot));
        Ok(Some(self.state.object_ref(slot)))
    }

    /// Asks `player` to choose one of `candidates`, or none when they may decline: a yes or
    /// no first, then the object, both for the purpose `ask`. With no candidates there is
    /// nothing to ask.
    pub fn may_choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        ask: Ask<L>,
    ) -> Result<Option<L::Slot>, Pause> {
        if candidates.is_empty() || !self.choose_yes_no(player, ask)? {
            return Ok(None);
        }
        self.choose_object(player, candidates, ask).map(Some)
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
    pub fn unless_pays(&mut self, player: Seat, cost: ManaCost) -> Result<bool, Pause> {
        let r = mana::payable(self.state, self.table, player, cost, Bitboard::EMPTY);
        if !self.checked(r) {
            return Ok(false);
        }
        if !self.choose_yes_no(player, Ask::UnlessPays { cost })? {
            return Ok(false);
        }
        self.pay(player, cost)?;
        self.sink.note(event::Event::ResolutionPayment {
            player,
            item: self.entry.clone(),
        });
        Ok(true)
    }

    /// Pays a mana cost `player` can pay, under the profile's payment rules (§10).
    fn pay(&mut self, player: Seat, cost: ManaCost) -> Result<(), Pause> {
        if self.state.profile.payment == Payment::Manual {
            let mut remaining = cost;
            while !remaining.is_zero() {
                let r = mana::manual_options(
                    self.state,
                    self.table,
                    player,
                    remaining,
                    Bitboard::EMPTY,
                );
                let options = self.checked(r);
                let option = self.ask_options(
                    player,
                    DecisionKind::ManaPayment,
                    options.as_slice(),
                    Ask::Plain,
                )?;
                let r =
                    mana::manual_pay(self.state, self.table, self.sink, player, remaining, option);
                remaining = self.checked(r);
            }
            return Ok(());
        }
        let greedy = self.state.profile.payment == Payment::Greedy;
        let r = mana::plans(
            self.state,
            self.table,
            self.sink,
            player,
            cost,
            Bitboard::EMPTY,
            greedy,
        );
        let plans = self.checked(r);
        let mut opts: L::Queue<Opt<L>, 64> = Default::default();
        for i in 0..plans.len() {
            let _ = opts.push(Opt::Plan(index::<L>(i)));
        }
        let Opt::Plan(i) = self.ask_options(
            player,
            DecisionKind::PaymentPlan,
            opts.as_slice(),
            Ask::Plain,
        )?
        else {
            unreachable!("a plan");
        };
        let r = mana::pay(
            self.state,
            self.table,
            self.sink,
            player,
            cost,
            Bitboard::EMPTY,
            &plans.as_slice()[Into::<usize>::into(i)],
        );
        self.checked(r);
        Ok(())
    }

    /// Lets `player` cast the card in `object`'s slot from the zone it is in, for as long as
    /// it stays there, or until end of turn with `this_turn`. A spell cast this way costs
    /// `extra` generic mana more, and with `exile` it is exiled rather than put into its
    /// owner's graveyard.
    pub fn allow_cast(
        &mut self,
        object: ObjectRef<L>,
        player: Seat,
        extra: u8,
        this_turn: bool,
        exile: bool,
    ) {
        let r = self.state.permissions.push(CastPermission {
            object,
            player,
            extra,
            this_turn,
            exile,
            flashback: false,
        });
        self.checked(r);
    }

    /// The card in `object`'s slot gains flashback until end of turn, its flashback cost
    /// equal to its mana cost, for `player` (Snapcaster Mage)
    /// ([CR 702.34a](https://yawgatog.com/resources/magic-rules/#R70234a)): "'Flashback
    /// [cost]' means 'You may cast this card from your graveyard if the resulting spell is an
    /// instant or sorcery spell by paying [cost] rather than paying its mana cost' and 'If the
    /// flashback cost was paid, exile this card instead of putting it anywhere else any time
    /// it would leave the stack.'" Only its owner can cast it from their graveyard, so
    /// `player` is its owner.
    pub fn grant_flashback(&mut self, object: ObjectRef<L>, player: Seat) {
        let r = self.state.permissions.push(CastPermission {
            object,
            player,
            extra: 0,
            this_turn: true,
            exile: false,
            flashback: true,
        });
        self.checked(r);
    }

    /// Lets `player` cast the card in `object`'s slot during this resolution, without paying
    /// its mana cost when `free`. The body's last action: the card is offered as it finishes
    /// resolving, which is when a spell cast during resolution would otherwise have
    /// mattered, since "no player receives priority after it's cast"
    /// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)).
    pub fn offer_cast(&mut self, object: ObjectRef<L>, player: Seat, free: bool) {
        self.offer = Some((object, player, free));
    }

    /// "Until your next turn, you may cast sorcery spells as though they had flash."
    pub fn allow_sorceries_as_flash(&mut self, player: Seat) {
        self.state.player_mut(player).sorceries_as_flash = true;
    }

    /// Adds mana to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool."
    pub fn add_mana(&mut self, player: Seat, color: ManaColor, amount: u8) {
        let source = self.entry.object;
        crate::interpret::add_mana(self.state, self.sink, player, source, color, amount);
    }

    /// Deals noncombat damage from this ability's source.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn deal_damage(&mut self, to: Recipient<L>, amount: u8) {
        let r = event::deal_damage(
            self.state,
            self.table,
            self.sink,
            self.source(),
            to,
            amount,
            false,
        );
        self.check(r);
    }

    pub fn gain_life(&mut self, player: Seat, amount: u8) {
        let r = event::gain_life(self.state, self.table, self.sink, player, amount);
        self.check(r);
    }

    pub fn lose_life(&mut self, player: Seat, amount: u8) {
        let r = event::lose_life(self.state, self.table, self.sink, player, amount);
        self.check(r);
    }

    pub fn draw(&mut self, player: Seat, cards: u8) {
        let r = event::draw(self.state, self.table, self.sink, player, cards);
        self.check(r);
    }

    pub fn add_counters(&mut self, object: ObjectRef<L>, kind: CounterKind, amount: u8) {
        let r = event::add_counters(self.state, self.table, self.sink, object, kind, amount);
        self.check(r);
    }

    pub fn remove_counters(&mut self, object: ObjectRef<L>, kind: CounterKind, amount: u8) {
        let r = event::remove_counters(self.state, self.table, self.sink, object, kind, amount);
        self.check(r);
    }

    pub fn tap(&mut self, object: ObjectRef<L>) {
        let r = event::set_tapped(self.state, self.table, self.sink, object, true);
        self.check(r);
    }

    pub fn untap(&mut self, object: ObjectRef<L>) {
        let r = event::set_tapped(self.state, self.table, self.sink, object, false);
        self.check(r);
    }

    /// Moves an object to a zone, to the top of an ordered one.
    pub fn move_to(&mut self, object: ObjectRef<L>, to: Zone, reason: Reason) {
        if !self.state.is_current(object) {
            return;
        }
        let r = event::move_one(self.state, self.table, self.sink, object.slot, to, reason);
        self.check(r);
    }

    /// Returns a spell or a permanent to its owner's hand
    /// ([CR 400.3](https://yawgatog.com/resources/magic-rules/#R4003)): "If an object would go
    /// to any library, graveyard, or hand other than its owner's, it goes to its owner's
    /// corresponding zone." A spell leaves the stack without resolving, and one cast with
    /// flashback is exiled instead (CR 702.34a).
    pub fn return_to_hand(&mut self, object: ObjectRef<L>) {
        if !self.state.is_current(object) {
            return;
        }
        let to = match self.state.object(object.slot).zone {
            Zone::Battlefield => Some(Zone::Hand),
            Zone::Stack => cast::take_spell(self.state, object)
                .map(|e| if e.flashback { Zone::Exile } else { Zone::Hand }),
            _ => None,
        };
        if let Some(to) = to {
            self.move_to(object, to, Reason::Effect);
        }
    }

    /// Puts a spell on top of its owner's library, or on the bottom unless `top`. It leaves
    /// the stack without resolving, and one cast with flashback is exiled instead
    /// (CR 702.34a).
    pub fn put_spell_into_library(&mut self, spell: ObjectRef<L>, top: bool) {
        if !self.state.is_current(spell) {
            return;
        }
        let Some(entry) = cast::take_spell(self.state, spell) else {
            return;
        };
        if entry.flashback {
            self.move_to(spell, Zone::Exile, Reason::Effect);
            return;
        }
        let r = event::move_batch_to(
            self.state,
            self.table,
            self.sink,
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
            self.state,
            self.table,
            self.sink,
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
            self.state,
            self.table,
            self.sink,
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
        zone::shuffle_library(self.state, player);
        self.sink.note(event::Event::LibraryShuffled { player });
    }

    /// Reveals a card to every player
    /// ([CR 701.20a](https://yawgatog.com/resources/magic-rules/#R70120a)): "To reveal a card,
    /// show that card to all players for a brief time." Each player knows it from then on,
    /// until it is shuffled or otherwise hidden anew.
    pub fn reveal(&mut self, slot: L::Slot) {
        chance::settle(self.state, self.sink, slot);
        for k in &mut self.state.knowledge {
            k.insert(slot);
        }
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
            self.state.controller(object.slot)
        } else {
            self.entry.controller
        };
        let top = self.state.library(player).last().copied();
        if let Some(slot) = top {
            self.reveal(slot);
            let card = self.state.object(slot).card;
            if self.table.def(card).front().types.contains(Types::LAND) {
                self.move_to(self.state.object_ref(slot), Zone::Hand, Reason::Effect);
                return Ok(());
            }
        }
        if self.on_battlefield(object) {
            self.add_counters(object, CounterKind::PlusOne, 1);
        }
        if let Some(slot) = top
            && self.choose_yes_no(player, Ask::Explore)?
        {
            self.move_to(self.state.object_ref(slot), Zone::Graveyard, Reason::Effect);
        }
        Ok(())
    }

    /// The top `n` cards of a player's library, or all of it if it has fewer.
    pub fn top_of_library(&self, player: Seat, n: u8) -> Bitboard<L::Slot> {
        let library = self.state.library(player);
        library[library.len().saturating_sub(usize::from(n))..]
            .iter()
            .copied()
            .collect()
    }

    /// `player` looks at the cards in `cards` and from then on knows them (architecture §13):
    /// a library and a hand are hidden zones
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)), "zones in which not
    /// all players can be expected to see the cards' faces."
    pub fn look_at(&mut self, player: Seat, cards: Bitboard<L::Slot>) {
        chance::settle_all(self.state, self.sink, cards);
        self.state.knowledge[player.index()] |= cards;
    }

    /// Discards a card from its owner's hand
    /// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
    /// move it from its owner's hand to that player's graveyard."
    pub fn discard(&mut self, card: ObjectRef<L>) {
        if self.state.is_current(card) && self.state.object(card.slot).zone == Zone::Hand {
            self.move_to(card, Zone::Graveyard, Reason::Discard);
        }
    }

    /// Puts cards on the bottom of their owner's library in a random order. Their identities
    /// are dealt anew among the slots they land in, so no player knows which is where
    /// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "That library's owner
    /// doesn't reveal the order in which the cards go into the library."
    pub fn put_on_bottom_in_random_order(&mut self, cards: Bitboard<L::Slot>) {
        let r = event::move_batch_to(
            self.state,
            self.table,
            self.sink,
            cards,
            Zone::Library,
            Place::Bottom,
            Reason::Effect,
            Entry::default(),
        );
        self.check(r);
        // Rebound within each owner's library, among the slots that went there.
        for owner in Seat::BOTH {
            let mut moved: L::Queue<L::Slot, 64> = Default::default();
            for &slot in self.state.library(owner) {
                if cards.contains(slot) && moved.push(slot).is_err() {
                    self.full = true;
                }
            }
            let mut rng = self.state.rng.other;
            zone::rebind(self.state, moved.as_slice(), &mut rng);
            self.state.rng.other = rng;
        }
    }

    /// Puts a card in a library on the bottom of that library, as Jace, the Mind Sculptor's
    /// "You may put that card on the bottom of that player's library." It stays in the
    /// library, so it is still the same object.
    pub fn put_library_card_on_bottom(&mut self, object: ObjectRef<L>) {
        if self.state.is_current(object) && self.state.object(object.slot).zone == Zone::Library {
            let r = zone::reorder_library(self.state, object.slot, Place::Bottom);
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
        let ask = Ask::Scry { count: n };
        let mut left = self.top_of_library(player, n);
        self.look_at(player, left);
        let mut bottom = 0;
        while !left.is_empty() {
            let Some(slot) = self.choose_or_done(player, left, ask)? else {
                break;
            };
            let r = zone::reorder_library(self.state, slot, Place::Bottom);
            self.check(r);
            left.remove(slot);
            bottom += 1;
        }
        let mut order: L::Queue<L::Slot, 60> = Default::default();
        while left.count() > 1 {
            let slot = self.choose_object(player, left, ask)?;
            left.remove(slot);
            let _ = order.push(slot);
        }
        if let Some(slot) = left.first() {
            let _ = order.push(slot);
        }
        for &slot in order.iter().rev() {
            let r = zone::reorder_library(self.state, slot, Place::Top);
            self.check(r);
        }
        if n > 0 {
            self.sink.note(event::Event::Scry {
                player,
                top: u8::try_from(order.len()).expect("scry top fits in u8"),
                bottom,
            });
        }
        Ok(())
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
        let chosen = self.choose_object(player, top, Ask::ManifestDread)?;
        let r = face_down::manifest(self.state, self.table, self.sink, chosen, player);
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
    /// ([CR 712.14a](https://yawgatog.com/resources/magic-rules/#R71214a)): "If a spell or
    /// ability puts a double-faced card onto the battlefield 'transformed' or 'converted,' it
    /// enters the battlefield with its back face up." If it already left the battlefield, it
    /// isn't exiled, so it doesn't return.
    pub fn exile_and_return_transformed(&mut self, object: ObjectRef<L>) {
        if !self.on_battlefield(object) {
            return;
        }
        self.move_to(object, Zone::Exile, Reason::Exile);
        let o = *self.state.object(object.slot);
        if o.zone != Zone::Exile {
            return;
        }
        let r = event::move_batch(
            self.state,
            self.table,
            self.sink,
            Bitboard::single(object.slot),
            Zone::Battlefield,
            Reason::Effect,
            Entry {
                controller: Some(o.owner),
                face: FaceIx::BACK,
                ..Entry::default()
            },
        );
        self.check(r);
    }

    /// Destroys a permanent ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)):
    /// "To destroy a permanent, move it from the battlefield to its owner's graveyard."
    pub fn destroy(&mut self, object: ObjectRef<L>) {
        if self.on_battlefield(object) {
            self.move_to(object, Zone::Graveyard, Reason::Destroy);
        }
    }

    /// Destroys a set of permanents simultaneously (CR 701.8a), as "Destroy each creature"
    /// does: one event, so "whenever one or more" abilities see them together.
    pub fn destroy_all(&mut self, objects: Bitboard<L::Slot>) {
        self.move_all(objects, Zone::Graveyard, Reason::Destroy);
    }

    /// [`EffectCtx::search_library`] for up to `n` cards, as Winds of Abandon's "a basic land
    /// card" for each creature exiled: those found, still in the library, one
    /// [`DecisionKind::ChooseUpTo`] at a time. The caller shuffles once they have moved.
    pub fn search_library_up_to(
        &mut self,
        player: Seat,
        filter: Filter,
        n: u8,
    ) -> Result<Bitboard<L::Slot>, Pause> {
        let library = self.state.zone(player, Zone::Library);
        self.look_at(player, library);
        let found = filter.select(self.state, self.table, library, player);
        self.choose_up_to(player, found, n, Ask::Search { max: n })
    }

    /// Puts cards onto the battlefield tapped, each under its owner's control, simultaneously:
    /// "Those cards enter tapped."
    pub fn put_onto_battlefield_tapped(&mut self, cards: Bitboard<L::Slot>) {
        let r = event::move_batch(
            self.state,
            self.table,
            self.sink,
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
        self.state.is_current(object) && self.state.object(object.slot).zone == Zone::Battlefield
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
        if self.state.object(object.slot).zone == Zone::Exile {
            let o = self.state.object_mut(object.slot);
            o.flags.set(ObjectFlags::EXILED_UNTIL, true);
            o.linked = Some(source.slot);
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
        if !self.state.is_current(object) {
            return;
        }
        let source = self.source();
        self.move_to(object, Zone::Exile, Reason::Exile);
        if self.state.is_current(source) && self.state.object(object.slot).zone == Zone::Exile {
            let o = self.state.object_mut(object.slot);
            o.flags.set(ObjectFlags::EXILED_WITH, true);
            o.linked = Some(source.slot);
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
        self.state.exiled_with(self.source().slot)
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
    ) -> Result<Option<L::Slot>, Pause> {
        Ok(self.search_library_up_to(player, filter, 1)?.iter().next())
    }

    /// Creates `count` tokens of a predefined kind under this ability's controller
    /// ([CR 111.10](https://yawgatog.com/resources/magic-rules/#R11110)).
    pub fn create_tokens(
        &mut self,
        def: &'static CardDef,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        self.create_tokens_for(self.entry.controller, def, count, how)
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
        let card = self.game_index(def);
        self.tokens(player, card, count, how)
    }

    /// Creates `count` tokens that are copies of the card with this game index, such as the
    /// card a last-known-information record names (CR 707.2).
    pub fn create_tokens_of(
        &mut self,
        card: CardIx,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        self.tokens(self.entry.controller, card, count, how)
    }

    /// Creates `count` tokens that are copies of the card with this game index under
    /// `player`'s control, as Fractured Identity's "Each player other than its controller
    /// creates a token that's a copy of it" (CR 111.2, 707.2).
    pub fn create_tokens_of_for(
        &mut self,
        player: Seat,
        card: CardIx,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        self.tokens(player, card, count, how)
    }

    /// The copiable values of a permanent, for tokens that copy it: its card, and how such a
    /// token enters ([CR 707.2](https://yawgatog.com/resources/magic-rules/#R7072)): "the
    /// copy acquires the copiable values of the original object's characteristics ... Other
    /// effects (including type-changing and text-changing effects), status, counters, and
    /// stickers are not copied." A token's are those its own creation defined
    /// ([CR 111.3](https://yawgatog.com/resources/magic-rules/#R1113)), including a copy's
    /// 1/1 exception ([CR 707.9b](https://yawgatog.com/resources/magic-rules/#R7079b)): "The
    /// final set of values for that characteristic becomes part of the copiable values of the
    /// copy." A double-faced permanent's copy has both faces, the same one up
    /// ([CR 707.8a](https://yawgatog.com/resources/magic-rules/#R7078a)). A face-down
    /// permanent's are the 2/2 with no text
    /// ([CR 708.2](https://yawgatog.com/resources/magic-rules/#R7082)): "Any listed
    /// characteristics are the copiable values of that object's characteristics." Being face
    /// down is a status, which isn't copied, so its copy is a face-up token of
    /// [`crate::card::FACE_DOWN_COPY`], not of the hidden card.
    pub fn copiable_values(&self, object: ObjectRef<L>) -> (CardIx, TokenEntry) {
        let o = self.state.object(object.slot);
        if o.face == FaceIx::DOWN {
            return (self.table.face_down_copy(), TokenEntry::default());
        }
        let how = TokenEntry {
            one_one: o.flags.has(ObjectFlags::ONE_ONE),
            face: o.face,
            ..TokenEntry::default()
        };
        (o.card, how)
    }

    fn tokens(
        &mut self,
        player: Seat,
        card: CardIx,
        count: u8,
        how: TokenEntry,
    ) -> Bitboard<L::Slot> {
        let created =
            event::create_tokens(self.state, self.table, self.sink, player, card, count, how);
        created.unwrap_or_else(|Full| {
            self.full = true;
            Bitboard::EMPTY
        })
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
    ) -> Result<Bitboard<L::Slot>, Pause> {
        let you = self.entry.controller;
        let attacking =
            crate::combat::in_combat(self.state.turn.step) && self.state.turn.active == you;
        let tokens = self.create_tokens(
            def,
            count,
            TokenEntry {
                tapped: true,
                attacking,
                ..TokenEntry::default()
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
            let opts = crate::combat::attack_targets(self.state, you.opponent());
            let Opt::Defender(walker) = self.ask_options(
                you,
                DecisionKind::AttackTarget,
                opts.as_slice(),
                Ask::AttackTarget { attackers: tokens },
            )?
            else {
                unreachable!("a defender")
            };
            if let Some(walker) = walker {
                for slot in tokens.iter() {
                    let o = self.state.object_mut(slot);
                    o.combat = Some(walker);
                    o.flags.set(ObjectFlags::ATTACKS_PLANESWALKER, true);
                }
            }
        }
        Ok(tokens)
    }

    /// Starts a floating continuous effect from this ability on one object
    /// ([CR 611.2](https://yawgatog.com/resources/magic-rules/#R6112)): "A continuous effect may
    /// be generated by the resolution of a spell or ability."
    pub fn add_effect(&mut self, affected: ObjectRef<L>, effect: Continuous, duration: Duration) {
        if !self.state.is_current(affected) {
            return;
        }
        let source = self.entry.card;
        let s = &mut *self.state;
        let timestamp = s.zones.next_timestamp;
        s.zones.next_timestamp = timestamp.wrapping_add(1);
        let pushed = s.effects.push(FloatingEffect {
            source,
            timestamp,
            duration,
            affected,
            effect,
        });
        s.flags.derived_dirty = true;
        self.check(pushed);
        layers::refresh(self.state, self.table, self.sink);
    }

    /// This ability's controller gains control of a permanent
    /// ([CR 613.1b](https://yawgatog.com/resources/magic-rules/#R6131b)): a layer-2 effect.
    /// It has not been under its new controller's control since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn gain_control(&mut self, object: ObjectRef<L>, duration: Duration) {
        if !self.on_battlefield(object) {
            return;
        }
        let player = self.entry.controller;
        self.add_effect(
            object,
            Continuous::Layer(crate::card::Modification::Control(player)),
            duration,
        );
        self.state
            .object_mut(object.slot)
            .flags
            .set(ObjectFlags::SUMMONING_SICK, true);
        let r = event::settle(
            self.state,
            self.table,
            self.sink,
            event::Event::GainControl { object, player },
        );
        self.check(r);
    }

    /// Gives a player an emblem ([CR 114.2](https://yawgatog.com/resources/magic-rules/#R1142)):
    /// "[Player] gets an emblem with [ability]. This means that [player] puts an emblem with
    /// [ability] into the command zone. The emblem is both owned and controlled by that
    /// player."
    pub fn create_emblem(&mut self, player: Seat, def: &'static CardDef) {
        let card = self.game_index(def);
        let r = self.state.player_mut(player).emblems.push(card);
        self.check(r);
    }

    /// Creates a delayed triggered ability: the ability at `ability` on this ability's card,
    /// triggering once at the moment its [`crate::card::Trigger::Delayed`] names, controlled
    /// by this ability's controller, carrying `amount`
    /// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
    pub fn delay(&mut self, ability: u8, amount: u8) {
        let trigger = PendingTrigger {
            source: self.entry.object,
            card: self.entry.card,
            face: self.entry.face,
            ability,
            controller: self.entry.controller,
            info: TriggerInfo {
                amount,
                ..TriggerInfo::default()
            },
        };
        let r = trigger::delay(self.state, trigger);
        self.check(r);
    }

    /// `player` wins the game ([CR 104.2b](https://yawgatog.com/resources/magic-rules/#R1042b)):
    /// "An effect may state that a player wins the game." The game ends at once; the body
    /// asks nothing more.
    pub fn win_game(&mut self, player: Seat) {
        if self.state.outcome.is_none() {
            let cause = crate::event::LossCause::Effect {
                source: self.entry.object,
                card: self.entry.card,
                face: self.entry.face,
                ability: self.entry.ability,
            };
            crate::sba::end_game(self.state, self.sink, player.opponent(), cause);
        }
    }

    /// This ability's controller untaps up to `n` lands of their choice, whoever controls
    /// them, as Teferi, Hero of Dominaria's "untap up to two lands". Untapping an untapped
    /// land does nothing ([CR 701.26b](https://yawgatog.com/resources/magic-rules/#R70126b)):
    /// "Only tapped permanents can be untapped", so only tapped lands are offered.
    pub fn untap_up_to_lands(&mut self, n: u8) -> Flow {
        let you = self.entry.controller;
        let tapped = Filter::All(&[Filter::Types(Types::LAND), Filter::Tapped]);
        let s = &*self.state;
        let lands = tapped.select(
            s,
            self.table,
            s.zone(Seat::P0, Zone::Battlefield) | s.zone(Seat::P1, Zone::Battlefield),
            you,
        );
        for slot in self
            .choose_up_to(you, lands, n, Ask::Untap { max: n })?
            .iter()
        {
            let land = self.state.object_ref(slot);
            self.untap(land);
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
        let won = chance::flip(self.state, self.sink, player);
        let source = self.source();
        let r = event::settle(
            self.state,
            self.table,
            self.sink,
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
        if !self.state.is_current(object) {
            return;
        }
        let r = event::move_batch(
            self.state,
            self.table,
            self.sink,
            Bitboard::single(object.slot),
            Zone::Battlefield,
            Reason::Effect,
            Entry {
                controller: Some(self.entry.controller),
                face,
                counters,
                ..Entry::default()
            },
        );
        self.check(r);
    }
}

/// The body of a mana ability: add what it produces, of the kind its activator chose
/// ([CR 106.3](https://yawgatog.com/resources/magic-rules/#R1063)): "A spell or ability that
/// produces mana instructs a player to add that mana."
pub fn add_produced_mana<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    let produces = ctx.ability.produces;
    debug_assert!(produces.includes(ctx.mana));
    ctx.add_mana(ctx.controller(), ctx.mana, produces.amount);
    Ok(())
}

/// "Draw a card."
pub fn draw_a_card<L: Layout>(ctx: &mut EffectCtx<'_, L>) -> Flow {
    ctx.draw(ctx.controller(), 1);
    Ok(())
}

/// Whether the cost a permanent's spell was cast with, marked by `flag`, was paid, for an
/// intervening "if" of its enters ability (CR 603.4). The mark is set as the permanent enters
/// and never changes while it stays; once it has left, the answer is what it was as the
/// ability triggered, and it triggered only if the cost was paid.
pub(crate) fn entered_with<L: Layout>(s: &State<L>, tr: &PendingTrigger<L>, flag: u32) -> bool {
    match tr.source {
        Some(r) if s.is_current(r) => s.object(r.slot).flags.has(flag),
        _ => true,
    }
}

/// "If it was kicked" ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)):
/// "If a spell's controller declares the intention to pay any of that spell's kicker costs,
/// that spell has been 'kicked.'"
pub fn kicked<L: Layout>(s: &State<L>, _: &GameTable, tr: &PendingTrigger<L>) -> bool {
    entered_with(s, tr, ObjectFlags::KICKED)
}
