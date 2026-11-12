//! `EffectCtx`: how a card body written before the rules library touches the game
//! (architecture §6.3). It is a shim: a [`Game`] and a [`Resolution`], and a method for each
//! thing a body did, which the library's functions replace one module at a time (the crate's
//! contract, "The shim"). No new code calls it.
//!
//! Every primitive builds its event and sends it through the pipeline of §8.2, so triggers,
//! replacement effects, layers and the sink see everything a body does. A primitive given a
//! reference to an object that has since changed zones does nothing to it
//! ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)).

use crate::actions::{
    counter, destroy, discard, manifest_dread, reveal, search, shuffle, tap, transform,
};
use crate::bitboard::Bitboard;
use crate::card::{
    Ability, AbilityIx, CardDef, CastingIx, CastingKind, FaceAbility, Flow, ManaColor, ManaCost,
    Prompt, RuleChange, Stop, Types,
};
use crate::event::{Reason, Recipient, Sink, TokenEntry};
use crate::filter::Filter;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::keywords::flashback;
use crate::layout::{Layout, Small};
use crate::query::Copiable;
use crate::resolution::Resolution;
use crate::state::{
    Continuous, CounterKind, Duration, ObjectRef, PendingTrigger, StackEntry, State, Target,
    TargetPlace, TargetPlaces, TriggerInfo, Zone, ZoneKey,
};
use crate::table::GameTable;
use crate::zone::Place;
use crate::{casting, choices, coins, continuous, costs, delayed_triggers, emblems};
use crate::{counters, damage, drawing, library, life, linked, one_shot, stack, tokens, zones};
use crate::{layers, mana, trigger};

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

    /// [`counter::counter`].
    pub fn counter(&mut self, spell: ObjectRef<L>, exile: bool) -> bool {
        let to = if exile { Zone::Exile } else { Zone::Graveyard };
        let r = counter::counter(&mut self.game, spell, to);
        self.checked(r)
    }

    /// [`stack::spell_controller`].
    pub fn spell_controller(&self, spell: ObjectRef<L>) -> Option<Seat> {
        stack::spell_controller(self.game.state, spell)
    }

    /// [`stack::spell_mana_value`].
    pub fn spell_mana_value(&self, spell: ObjectRef<L>) -> Option<u8> {
        stack::spell_mana_value(self.game.state, self.game.table, spell)
    }

    /// [`choices::yes_no`].
    pub fn choose_yes_no(
        &mut self,
        player: Seat,
        prompt: Prompt,
        about: Bitboard<L::Slot>,
    ) -> Result<bool, Stop> {
        choices::yes_no(&mut self.game, &mut self.resolution, player, prompt, about)
    }

    /// [`choices::pick_up_to`].
    pub fn choose_up_to(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        n: u8,
        prompt: Prompt,
    ) -> Result<Bitboard<L::Slot>, Stop> {
        choices::pick_up_to(
            &mut self.game,
            &mut self.resolution,
            player,
            candidates,
            n,
            prompt,
        )
    }

    /// [`choices::pick`].
    pub fn choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<L::Slot, Stop> {
        choices::pick(
            &mut self.game,
            &mut self.resolution,
            player,
            candidates,
            prompt,
        )
    }

    /// [`choices::may_pick`].
    pub fn may_choose_object(
        &mut self,
        player: Seat,
        candidates: Bitboard<L::Slot>,
        prompt: Prompt,
    ) -> Result<Option<L::Slot>, Stop> {
        choices::may_pick(
            &mut self.game,
            &mut self.resolution,
            player,
            candidates,
            prompt,
        )
    }

    /// [`discard::discard_one_of`] the cards in `player`'s hand.
    pub fn discard_a_card(&mut self, player: Seat) -> Result<Option<ObjectRef<L>>, Stop> {
        let hand = self.game.state.zone(ZoneKey::Of(player, Zone::Hand));
        discard::discard_one_of(&mut self.game, &mut self.resolution, player, hand)
    }

    /// [`discard::discard_cards`].
    pub fn discard_cards(&mut self, player: Seat, n: u8) -> Result<Bitboard<L::Slot>, Stop> {
        discard::discard_cards(&mut self.game, &mut self.resolution, player, n)
    }

    /// [`discard::discard_one_of`].
    pub fn discard_one_of(
        &mut self,
        player: Seat,
        cards: Bitboard<L::Slot>,
    ) -> Result<Option<ObjectRef<L>>, Stop> {
        discard::discard_one_of(&mut self.game, &mut self.resolution, player, cards)
    }

    /// [`costs::may_pay`], asked as [`Effect::Unless`](beryl_card::Effect::Unless) asks.
    pub fn unless_pays(&mut self, player: Seat, cost: ManaCost) -> Result<bool, Stop> {
        costs::may_pay(
            &mut self.game,
            &mut self.resolution,
            player,
            cost,
            costs::UNLESS,
        )
    }

    /// [`casting::offer`].
    pub fn offer_cast(&mut self, object: ObjectRef<L>, player: Seat, free: bool) {
        casting::offer(&mut self.resolution, object, player, free);
    }

    /// [`mana::add`], as an effect of this ability's source.
    pub fn add_mana(&mut self, player: Seat, color: ManaColor, amount: u8) {
        let source = self.resolution.entry.object;
        mana::add(&mut self.game, player, source, color, amount);
    }

    /// [`continuous::start`].
    pub fn add_effect(&mut self, effect: Continuous<L>, duration: Duration) {
        let r = continuous::start(&mut self.game, &self.resolution, effect, duration);
        self.check(r);
    }

    /// [`continuous::change_rule`].
    pub fn change_rule(&mut self, rule: &'static RuleChange, duration: Duration) {
        let r = continuous::change_rule(&mut self.game, &self.resolution, rule, duration);
        self.check(r);
    }

    /// [`continuous::gain_control`].
    pub fn gain_control(&mut self, object: ObjectRef<L>, duration: Duration) {
        let r = continuous::gain_control(&mut self.game, &self.resolution, object, duration);
        self.check(r);
    }

    /// [`casting::allow`].
    pub fn allow_cast(
        &mut self,
        object: ObjectRef<L>,
        player: Seat,
        extra: u8,
        this_turn: bool,
        exile: bool,
    ) {
        let (game, resolution) = (&mut self.game, &self.resolution);
        let r = casting::allow(game, resolution, object, player, extra, this_turn, exile);
        self.check(r);
    }

    /// [`flashback::grant`].
    pub fn grant_flashback(&mut self, object: ObjectRef<L>, player: Seat) {
        let r = flashback::grant(&mut self.game, &self.resolution, object, player);
        self.check(r);
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

    /// [`drawing::draw`].
    pub fn draw(&mut self, player: Seat, cards: u8) {
        let r = drawing::draw(&mut self.game, player, cards);
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

    /// [`zones::move_to`].
    pub fn move_to(&mut self, object: ObjectRef<L>, to: Zone, reason: Reason) {
        let r = zones::move_to(&mut self.game, object, to, reason);
        self.check(r);
    }

    /// [`zones::return_to_hand`].
    pub fn return_to_hand(&mut self, object: ObjectRef<L>) {
        let r = zones::return_to_hand(&mut self.game, object);
        self.check(r);
    }

    /// [`library::put_into`] for a spell: on top of its owner's library, or on the bottom
    /// unless `top`.
    pub fn put_spell_into_library(&mut self, spell: ObjectRef<L>, top: bool) {
        let place = if top { Place::Top } else { Place::Bottom };
        let r = library::put_into(&mut self.game, spell, place);
        self.check(r);
    }

    /// [`library::put_into`], under `under` cards from the top.
    pub fn put_into_library(&mut self, object: ObjectRef<L>, under: u8) {
        let r = library::put_into(&mut self.game, object, Place::UnderTop(under));
        self.check(r);
    }

    /// [`zones::move_all`].
    pub fn move_all(&mut self, objects: Bitboard<L::Slot>, to: Zone, reason: Reason) {
        let r = zones::move_all(&mut self.game, objects, to, reason);
        self.check(r);
    }

    /// [`shuffle::shuffle`].
    pub fn shuffle(&mut self, player: Seat) {
        shuffle::shuffle(&mut self.game, player);
    }

    /// [`reveal::reveal`].
    pub fn reveal(&mut self, slot: L::Slot) {
        reveal::reveal(&mut self.game, slot);
    }

    /// [`library::top`].
    pub fn top_of_library(&self, player: Seat, n: u8) -> Bitboard<L::Slot> {
        library::top(self.game.state, player, n)
    }

    /// [`zones::look_at`].
    pub fn look_at(&mut self, player: Seat, cards: Bitboard<L::Slot>) {
        zones::look_at(&mut self.game, player, cards);
    }

    /// [`library::put_on_bottom_in_random_order`].
    pub fn put_on_bottom_in_random_order(&mut self, cards: Bitboard<L::Slot>) {
        let r = library::put_on_bottom_in_random_order(&mut self.game, cards);
        self.check(r);
    }

    /// [`library::put_into`], on the bottom, for a card in a library: "You may put that
    /// card on the bottom of that player's library."
    pub fn put_library_card_on_bottom(&mut self, object: ObjectRef<L>) {
        let r = library::put_into(&mut self.game, object, Place::Bottom);
        self.check(r);
    }

    /// [`manifest_dread::manifest_dread`].
    pub fn manifest_dread(&mut self, player: Seat) -> Flow {
        manifest_dread::manifest_dread(&mut self.game, &mut self.resolution, player)
    }

    /// [`transform::exile_and_return_transformed`].
    pub fn exile_and_return_transformed(&mut self, object: ObjectRef<L>) {
        let r = transform::exile_and_return_transformed(&mut self.game, object);
        self.check(r);
    }

    /// [`destroy::destroy`].
    pub fn destroy(&mut self, object: ObjectRef<L>) {
        let r = destroy::destroy(&mut self.game, object);
        self.check(r);
    }

    /// [`search::search_library_up_to`].
    pub fn search_library_up_to(
        &mut self,
        player: Seat,
        filter: Filter,
        n: u8,
    ) -> Result<Bitboard<L::Slot>, Stop> {
        search::search_library_up_to(&mut self.game, &mut self.resolution, player, filter, n)
    }

    /// [`search::search_library`].
    pub fn search_library(
        &mut self,
        player: Seat,
        filter: Filter,
    ) -> Result<Option<L::Slot>, Stop> {
        search::search_library(&mut self.game, &mut self.resolution, player, filter)
    }

    /// [`zones::put_onto_battlefield_tapped`].
    pub fn put_onto_battlefield_tapped(&mut self, cards: Bitboard<L::Slot>) {
        let r = zones::put_onto_battlefield_tapped(&mut self.game, cards);
        self.check(r);
    }

    /// [`one_shot::exile_until_leaves`], until this ability's source leaves.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn exile_until_source_leaves(&mut self, object: ObjectRef<L>) {
        let r = one_shot::exile_until_leaves(&mut self.game, object, self.resolution.source());
        self.check(r);
    }

    /// [`linked::exile_linked`], to this ability's source.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn exile_linked(&mut self, object: ObjectRef<L>) {
        let r = linked::exile_linked(&mut self.game, object, self.resolution.source());
        self.check(r);
    }

    /// [`linked::exiled_with`] this ability's source.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn exiled_with_source(&self) -> Bitboard<L::Slot> {
        linked::exiled_with(self.game.state, self.source())
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

    /// [`tokens::create_attacking`]: `count` tokens of a predefined kind under this ability's
    /// controller, attacking any of the defending players.
    pub fn create_attacking_tokens(
        &mut self,
        def: &'static CardDef,
        count: u8,
    ) -> Result<Bitboard<L::Slot>, Stop> {
        let (game, you) = (&mut self.game, self.resolution.controller());
        let (what, whom) = (tokens::predefined(game.table, def), game.state.players());
        tokens::create_attacking(game, &mut self.resolution, you, what, count, whom)
    }

    /// [`emblems::create`], of the emblem `def`.
    pub fn create_emblem(&mut self, player: Seat, def: &'static CardDef) {
        let card = self.game_index(def);
        let r = emblems::create(&mut self.game, player, card);
        self.check(r);
    }

    /// [`delayed_triggers::create`].
    pub fn delay(&mut self, ability: AbilityIx, amount: u8) {
        let r = delayed_triggers::create(&mut self.game, &self.resolution, ability, amount);
        self.check(r);
    }

    /// [`tap::untap_up_to`]: this ability's controller untaps up to `n` lands.
    pub fn untap_up_to_lands(&mut self, n: u8) -> Flow {
        let (you, lands) = (self.controller(), Filter::Types(Types::LAND));
        tap::untap_up_to(&mut self.game, &mut self.resolution, you, lands, n)
    }

    /// [`coins::flip`], for this ability's source.
    ///
    /// # Panics
    ///
    /// For an emblem's ability, which has no source.
    pub fn flip_coin(&mut self, player: Seat) -> bool {
        let r = coins::flip(&mut self.game, player, self.resolution.source());
        self.checked(r)
    }
}

/// "If it was kicked" ([CR 702.33d](https://yawgatog.com/resources/magic-rules/#R70233d)):
/// "If a spell's controller declares the intention to pay any of that spell's kicker costs,
/// that spell has been 'kicked.'"
pub fn kicked<L: Layout>(s: &State<L>, _: &GameTable, tr: &PendingTrigger<L>) -> bool {
    trigger::entered_with(s, tr, CastingKind::Kicker)
}
