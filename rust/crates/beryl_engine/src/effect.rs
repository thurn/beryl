//! `EffectCtx`: the only way a card body touches the state (architecture §6.3).
//!
//! Every primitive builds its event and sends it through the pipeline of §8.2, so triggers,
//! replacement effects, layers and the sink see everything a body does. A primitive given a
//! reference to an object that has since changed zones does nothing to it
//! ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).

use crate::bitboard::Bitboard;
use crate::card::{Ability, CardDef, Delayed, ManaColor};
use crate::event::{self, Entry, Reason, Recipient, Sink, TokenEntry};
use crate::fixed::Full;
use crate::state::{
    Continuous, CounterKind, Duration, FloatingEffect, ObjectFlags, ObjectRef, PendingTrigger,
    State, TriggerInfo, Zone,
};
use crate::table::GameTable;
use crate::{layers, trigger};

/// What a body runs against: the working state and tables, the event sink, and the ability
/// being resolved with what triggered it.
pub struct EffectCtx<'a> {
    pub(crate) state: &'a mut State,
    pub(crate) table: &'a GameTable,
    pub(crate) sink: &'a mut Sink,
    pub(crate) source: ObjectRef,
    pub(crate) controller: u8,
    pub(crate) ability: &'static Ability,
    /// The card and face the ability is printed on, as game indices.
    pub(crate) card: u8,
    pub(crate) face: u8,
    /// For a triggered ability: what it triggered on.
    pub(crate) info: TriggerInfo,
    /// For a mana ability that can produce more than one kind of mana, the kind its
    /// activator chose.
    pub(crate) mana: ManaColor,
    /// A fixed capacity ran out; the engine stalls once the body returns (§12.3).
    pub(crate) full: bool,
}

impl<'a> EffectCtx<'a> {
    /// The context for a triggered ability's body.
    pub(crate) fn new(
        state: &'a mut State,
        table: &'a GameTable,
        sink: &'a mut Sink,
        trigger: &PendingTrigger,
        ability: &'static Ability,
    ) -> Self {
        Self {
            state,
            table,
            sink,
            source: trigger.source,
            controller: trigger.controller,
            ability,
            card: trigger.card,
            face: trigger.face,
            info: trigger.info,
            mana: ManaColor::Colorless,
            full: false,
        }
    }

    /// Ends the body: a stall if a capacity ran out, and a layer refresh for what follows.
    pub(crate) fn finish(self) -> Result<(), Full> {
        layers::refresh(self.state, self.table);
        if self.full { Err(Full) } else { Ok(()) }
    }

    fn check(&mut self, r: Result<(), Full>) {
        if r.is_err() {
            self.full = true;
        }
    }

    pub fn source(&self) -> ObjectRef {
        self.source
    }

    pub fn controller(&self) -> u8 {
        self.controller
    }

    /// The state as it is now, to read.
    pub fn state(&self) -> &State {
        self.state
    }

    pub fn table(&self) -> &GameTable {
        self.table
    }

    /// What a triggered ability triggered on, with last-known information for a look-back
    /// trigger.
    pub fn trigger(&self) -> TriggerInfo {
        self.info
    }

    /// The game index of a card or token the game's table holds.
    ///
    /// # Panics
    ///
    /// If the table lacks it: a card's tokens must be listed in its `tokens`.
    pub fn game_index(&self, def: &'static CardDef) -> u8 {
        self.table
            .index_of(def)
            .expect("a token missing from its card's tokens")
    }

    /// Adds mana to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool."
    pub fn add_mana(&mut self, player: u8, color: ManaColor, amount: u8) {
        let pool = &mut self.state.player_mut(player).mana[color as usize];
        *pool = pool.saturating_add(amount);
    }

    /// Deals noncombat damage from this ability's source.
    pub fn deal_damage(&mut self, to: Recipient, amount: u8) {
        let r = event::deal_damage(
            self.state,
            self.table,
            self.sink,
            self.source,
            to,
            amount,
            false,
        );
        self.check(r);
    }

    pub fn gain_life(&mut self, player: u8, amount: u8) {
        let r = event::gain_life(self.state, self.table, self.sink, player, amount);
        self.check(r);
    }

    pub fn lose_life(&mut self, player: u8, amount: u8) {
        let r = event::lose_life(self.state, self.table, self.sink, player, amount);
        self.check(r);
    }

    pub fn draw(&mut self, player: u8, cards: u8) {
        let r = event::draw(self.state, self.table, self.sink, player, cards);
        self.check(r);
    }

    pub fn add_counters(&mut self, object: ObjectRef, kind: CounterKind, amount: u8) {
        let r = event::add_counters(self.state, self.table, self.sink, object, kind, amount);
        self.check(r);
    }

    pub fn remove_counters(&mut self, object: ObjectRef, kind: CounterKind, amount: u8) {
        let r = event::remove_counters(self.state, self.table, self.sink, object, kind, amount);
        self.check(r);
    }

    pub fn tap(&mut self, object: ObjectRef) {
        let r = event::set_tapped(self.state, self.table, self.sink, object, true);
        self.check(r);
    }

    pub fn untap(&mut self, object: ObjectRef) {
        let r = event::set_tapped(self.state, self.table, self.sink, object, false);
        self.check(r);
    }

    /// Moves an object to a zone, to the top of an ordered one.
    pub fn move_to(&mut self, object: ObjectRef, to: Zone, reason: Reason) {
        if !self.state.is_current(object) {
            return;
        }
        let r = event::move_one(self.state, self.table, self.sink, object.slot, to, reason);
        self.check(r);
    }

    /// Destroys a permanent ([CR 701.8a](https://yawgatog.com/resources/magic-rules/#R7018a)):
    /// "To destroy a permanent, move it from the battlefield to its owner's graveyard."
    pub fn destroy(&mut self, object: ObjectRef) {
        if self.on_battlefield(object) {
            self.move_to(object, Zone::Graveyard, Reason::Destroy);
        }
    }

    /// Sacrifices a permanent ([CR 701.21a](https://yawgatog.com/resources/magic-rules/#R70121a)):
    /// "To sacrifice a permanent, its controller moves it from the battlefield directly to its
    /// owner's graveyard."
    pub fn sacrifice(&mut self, object: ObjectRef) {
        if self.on_battlefield(object) {
            self.move_to(object, Zone::Graveyard, Reason::Sacrifice);
        }
    }

    fn on_battlefield(&self, object: ObjectRef) -> bool {
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
    pub fn exile_until_source_leaves(&mut self, object: ObjectRef) {
        if !self.on_battlefield(self.source) || !self.on_battlefield(object) {
            return;
        }
        self.move_to(object, Zone::Exile, Reason::Exile);
        if self.state.object(object.slot).zone == Zone::Exile {
            let o = self.state.object_mut(object.slot);
            o.flags.set(ObjectFlags::EXILED_UNTIL, true);
            o.linked = self.source.slot;
        }
    }

    /// Creates `count` tokens of a predefined kind under this ability's controller
    /// ([CR 111.10](https://yawgatog.com/resources/magic-rules/#R11110)).
    pub fn create_tokens(&mut self, def: &'static CardDef, count: u8, how: TokenEntry) -> Bitboard {
        let card = self.game_index(def);
        self.create_tokens_of(card, count, how)
    }

    /// Creates `count` tokens that are copies of the card with this game index, such as the
    /// card a last-known-information record names (CR 707.2).
    pub fn create_tokens_of(&mut self, card: u8, count: u8, how: TokenEntry) -> Bitboard {
        let created = event::create_tokens(
            self.state,
            self.table,
            self.sink,
            self.controller,
            card,
            count,
            how,
        );
        created.unwrap_or_else(|Full| {
            self.full = true;
            Bitboard::EMPTY
        })
    }

    /// Starts a floating continuous effect from this ability on one object
    /// ([CR 611.2](https://yawgatog.com/resources/magic-rules/#R6112)): "A continuous effect may
    /// be generated by the resolution of a spell or ability."
    pub fn add_effect(&mut self, affected: ObjectRef, effect: Continuous, duration: Duration) {
        if !self.state.is_current(affected) {
            return;
        }
        let s = &mut *self.state;
        let timestamp = s.zones.next_timestamp;
        s.zones.next_timestamp = timestamp.wrapping_add(1);
        let pushed = s.effects.push(FloatingEffect {
            source: self.source,
            timestamp,
            duration,
            affected,
            effect,
        });
        s.flags.derived_dirty = true;
        self.check(pushed);
        layers::refresh(self.state, self.table);
    }

    /// This ability's controller gains control of a permanent
    /// ([CR 613.1b](https://yawgatog.com/resources/magic-rules/#R6131b)): a layer-2 effect.
    /// It has not been under its new controller's control since their most recent turn began
    /// ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)).
    pub fn gain_control(&mut self, object: ObjectRef, duration: Duration) {
        if !self.on_battlefield(object) {
            return;
        }
        let player = self.controller;
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
    pub fn create_emblem(&mut self, player: u8, def: &'static CardDef) {
        let card = self.game_index(def);
        let r = self.state.player_mut(player).emblems.push(card);
        self.check(r);
    }

    /// Creates a delayed triggered ability: the ability at `ability` on this ability's card,
    /// triggering once at `at`, controlled by this ability's controller, carrying `amount`
    /// ([CR 603.7](https://yawgatog.com/resources/magic-rules/#R6037)).
    pub fn delay(&mut self, ability: u8, at: Delayed, amount: u8) {
        let trigger = PendingTrigger {
            source: self.source,
            card: self.card,
            face: self.face,
            ability,
            controller: self.controller,
            info: TriggerInfo {
                amount,
                ..TriggerInfo::default()
            },
        };
        let r = trigger::delay(self.state, trigger, at);
        self.check(r);
    }

    /// Flips a coin and returns whether the player won the flip
    /// ([CR 705.2](https://yawgatog.com/resources/magic-rules/#R7052)): "the player that flips
    /// the coin calls 'heads' or 'tails.' If the call matches the result, the player wins the
    /// flip." A fair coin against any call is a fair win, so one draw from the game's
    /// randomness decides it (architecture §14).
    pub fn flip_coin(&mut self, player: u8) -> bool {
        let won = self.state.rng.other.below(2) == 0;
        let r = event::settle(
            self.state,
            self.table,
            self.sink,
            event::Event::CoinFlip { player, won },
        );
        self.check(r);
        won
    }

    /// Puts a permanent card onto the battlefield under this ability's controller, with the
    /// face up that it enters with (a double-faced card returned "transformed" enters with
    /// its back face up) and any counters it enters with.
    pub fn put_onto_battlefield(
        &mut self,
        object: ObjectRef,
        face: u8,
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
                controller: Some(self.controller),
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
pub fn add_produced_mana(ctx: &mut EffectCtx) -> crate::card::Flow {
    let produces = ctx.ability.produces;
    debug_assert!(produces.includes(ctx.mana));
    ctx.add_mana(ctx.controller, ctx.mana, produces.amount);
    Ok(())
}

/// The body of an ability that does nothing when it resolves, such as a static ability.
pub fn nothing(_: &mut EffectCtx) -> crate::card::Flow {
    Ok(())
}
