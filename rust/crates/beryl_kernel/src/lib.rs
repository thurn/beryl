//! Beryl's kernel: the game state and the only code that writes it (plan K6 and K7;
//! `docs/project-50k.md`, "State" and "Kernel rules").
//!
//! The kernel owns [`State`] and its storage [`Layout`]s, the slots and references that name
//! objects, the bitboards and fixed queues it is built from, the per-game card table, the
//! zone-move and chance chokepoints, the invariants, and the records, decisions and events
//! that pass between it and the rules. It depends on `beryl_card` alone and holds no rule of
//! the game's flow: turns, priority, casting, mana, combat, layers, triggers and the
//! interpreter are `beryl_engine`'s, which reaches the state through the API below.
//! [`Layout`] names no rules type: what the rules keep once per layout they read through
//! [`layout::PerLayout`].
//!
//! # The contract
//!
//! Only the kernel writes a field. This is the design every bead of milestone 3 (epic
//! by-w8gv.5) builds on, written as it will stand when they land; a bead that builds a part
//! amends its text. Each part names its bead. Until then the fields it closes are `pub` and
//! say which bead makes them private, and the mutators, sets, queries and player API below
//! exist only where a link names them.
//!
//! ## Mutators
//!
//! A mutator is a method of [`State`], `#[inline]` and generic over the layout: one for each
//! thing the rules do to the state. It takes what the rules decided and makes the whole
//! change, so no caller repeats a part of it. No `&mut` to an object, a player or a queue
//! leaves the crate. One that runs out of a capacity returns [`fixed::Full`], which the rules
//! turn into a stall: the state is then abandoned for its wider copy, so a mutator never
//! undoes.
//!
//! | The rules | Mutators | What each leaves consistent |
//! | --- | --- | --- |
//! | move an object between zones (CR 400.7), create a token, end one (by-w8gv.5.3, .5.4; today [`zone`]) | `move_object`, `create_token`, `cease_to_exist` | the new incarnation and its reset fields; both zones' membership bitboards; the ordered library, graveyard and stack; the battlefield queue and every permanent's index into it; `derived_dirty`; the attachments, combat references and exile links that named it; its identity, settled before anyone sees it ([`chance`]); both players' knowledge |
//! | change a permanent's status, damage or counters (by-w8gv.5.3) | `set_tapped`, `set_face`, `set_summoning_sick`, `mark_damage`, `clear_damage`, `add_counters`, `remove_counters` | `derived_dirty` where the layers read the field |
//! | attach, change control, declare and end combat (by-w8gv.5.3) | `attach`, `unattach`, `set_base_controller`, `declare_attacker`, `declare_blocker`, `remove_from_combat`, `end_combat` | a reference names a current permanent; the combat flags and the combat reference change together; `derived_dirty` |
//! | show or hide a card (by-w8gv.5.4) | `reveal`, `look`, `forget` | each player's knowledge; the identity is settled first |
//! | store the layers' results (by-w8gv.5.4) | `rederive`, which lends the cache to the one function that fills it | the derived table by battlefield index, the listening mask, `derived_dirty` cleared |
//! | change a player or the turn (by-w8gv.5.7) | `change_life`, `add_mana`, `spend_mana`, `empty_mana`, `count_land_played`, `set_mulligan`, `begin_turn`, `begin_step`, `give_priority`, `pass_priority`, `end_game` | passes reset with priority; the per-turn counts reset with the turn; the outcome is set once |
//! | use the stack and the queues (by-w8gv.5.8) | `put_on_stack`, `take_off_stack`, which returns the entries, `queue_trigger`, `take_triggers`, `delay_trigger`, `ask`, `settle_question` | the stack queue and the stack's bitboard hold the same spells; a question is open or priority is |
//!
//! ## Events
//!
//! A mutator whose change a player can see returns the [`event::Event`] it caused, marked
//! `#[must_use]`. It neither pushes to the [`event::Sink`] nor runs anything after. The
//! rules' pipeline (`beryl_engine`'s `event::settle`) takes every such event: it refreshes
//! the layers, matches triggers and records the event. Three reasons:
//!
//! - What follows an event is rules the kernel cannot call, and it must happen with the
//!   event in hand before anything else reads the state.
//! - Search runs with the sink off and still matches triggers on the event, so the sink
//!   cannot carry it. A returned value inlines to what the pipeline builds today.
//! - The kernel knows what changed; only the rules know why. `settle` adds the cause as it
//!   records (plan K23), so no mutator takes one and no event exists without its change.
//!
//! Builds deny warnings, so dropping an event does not compile: no visible change happens
//! without one. A mutator takes as an argument what the event needs and the state cannot say,
//! such as a zone change's [`event::Reason`]. Bookkeeping no player sees as an event (passes,
//! budgets, the open question, the derived cache) returns nothing.
//!
//! ## Entities and records
//!
//! [`State`], [`state::Object`], [`state::Player`], [`state::Turn`] and [`state::Zones`] are
//! entities: private fields, read through `#[inline]` getters (by-w8gv.5.9 for the readers in
//! other crates), written by mutators.
//!
//! Records stay plain data with `pub` fields. The rules build one and hand it to a mutator;
//! the kernel stores it and gives it back whole: [`state::StackEntry`],
//! [`state::PendingTrigger`], [`state::Target`], [`state::TriggerInfo`], [`state::Lki`],
//! [`state::Casting`] and the other payloads of [`state::Pending`], and a
//! [`decision::Decision`] with its parts. [`state::Derived`] is one going the other way: the
//! layers compute it, `rederive` stores it, everyone reads it. No invariant ties a record's
//! fields to the rest of the state except the references it holds, which
//! [`State::is_current`] checks; where a record is filed is the mutator's to keep. The kernel
//! fills [`state::Resolving`]'s knowledge itself as a resolution pauses.
//!
//! ## The board door
//!
//! `State::board` lends a `Board`, the one handle that arranges a position with no event and
//! no rules action: `place`, `place_token`, the `set_*` methods, `attach`, `add_emblem`,
//! `order_library`, `begin_at`, `begin_before_turn`, `set_flip_winner`, `renumber_deck_slots`
//! and `reverse_deck_slots`. Scenarios, checkpoints, tools and tests go through it. A card
//! body is lent the state shared, so it cannot open the door; rules code never names it, and
//! a search for `.board()` finds whoever does. Today these are methods of [`State`] itself
//! (`setup.rs`); by-w8gv.5.3 moves them and their callers behind the door.
//!
//! ## The legacy accessor
//!
//! `State::legacy_mut` lends, raw, what milestone 4 (by-w8gv.7.1) deletes, which therefore
//! gets no typed mutator: the `effects` and `permissions` queues, which become stores whose
//! payload is a resolved form of `Modification`; `entered_x`; `Player::sorceries_as_flash`;
//! [`state::CastPermission`]'s `extra`, `exile` and `flashback`; the [`state::ObjectFlags`]
//! `KICKED`, `OFFSPRING`, `IMPENDING`, `EVOKED`, `EXILED_UNTIL`, `EXILED_WITH` and
//! `LOYALTY_ACTIVATED`; and [`state::Counters`]' long-tail kind (`other_kind`, `other`).
//! Reads use the getters. Milestone 4 deletes the accessor with the last of them.
//!
//! ## Sets and orders
//!
//! Outside the kernel a [`Bitboard`] is a set: union, intersection, difference, `contains`,
//! `count`, `is_empty`, `any`, `all`, `filter` and `sole`, its one member. It does not
//! iterate. A caller that needs a sequence asks the state for an order by name (by-w8gv.5.6):
//!
//! | Order | Query |
//! | --- | --- |
//! | the battlefield, as permanents entered it | [`State::battlefield`] |
//! | a library, top first | `State::library_from_top` |
//! | a graveyard, as cards were put there (CR 404.2) | [`State::graveyard`] |
//! | players in turn order from the active player (CR 101.4) | `State::players_in_turn_order` |
//! | a batch, in an order no seed decides: permanents as they entered, then library cards top first, then the rest by name | `State::in_game_order` |
//! | a decision's candidates, by ascending slot | `State::in_candidate_order` |
//!
//! The last is not a rules order, since the deal and every shuffle put cards in slots at
//! random. A `Choice` is an index into candidates listed that way, so saves and
//! `rollouts_never_change` depend on it; it is for listing candidates and nothing else.
//!
//! ## Queries
//!
//! Other crates read derived answers, not fields (by-w8gv.5.5):
//!
//! ```text
//! fn card_behind(&self, slot: L::Slot) -> Option<CardIx>
//! fn copiable(&self, t: &GameTable, slot: L::Slot) -> Copiable
//! fn mana_value(&self, t: &GameTable, slot: L::Slot) -> u8
//! fn controller(&self, slot: L::Slot) -> Seat
//! fn power(&self, t: &GameTable, slot: L::Slot) -> i8
//! fn toughness(&self, t: &GameTable, slot: L::Slot) -> i8
//! ```
//!
//! `card_behind` is the card that represents the object, and nothing for a token or a copy
//! of a spell (CR 111.1, 707.10). `copiable` is what a copy of it would have (CR 707.2): the
//! definition and face, with a copy effect's exceptions (CR 707.9b). `mana_value` counts a
//! spell's X (CR 202.3e), a transformed permanent's front face and a copied back face's 0
//! (CR 712.8e). `controller` is a permanent's layer result, a spell's stack entry's, and
//! elsewhere the owner (CR 108.4a). `power` and `toughness` are the layer results on the
//! battlefield and the copiable values elsewhere. [`State::card`], the raw definition index,
//! is then the kernel's own.
//!
//! ## Players
//!
//! Rules code reaches players through sets and orders (by-w8gv.5.13), never the other
//! player of two. `Players` is an order-free set (the interpreter's `Who`, moved here), and:
//!
//! ```text
//! fn players(&self) -> Players
//! fn opponents_of(&self, player: Seat) -> Players
//! fn players_in_turn_order(&self) -> impl Iterator<Item = Seat>
//! fn defending_players(&self) -> Players
//! fn defending_player(&self, attacker: L::Slot) -> Option<Seat>
//! ```
//!
//! `defending_player` is the player an attacking creature attacks, or the controller of the
//! planeswalker it attacks (CR 506.2). `Seat::opponent` is not part of the API: it stays
//! inside the two-player layout. Player count, deck size, slot count and card-index width
//! are parameters of the layout (by-w8gv.5.14, by-w8gv.5.15).

pub mod bitboard;
pub mod chance;
pub mod decision;
pub mod event;
pub mod filter;
pub mod fixed;
mod floating;
pub mod ids;
pub mod invariants;
pub mod layout;
mod new;
mod object;
pub mod observation;
mod pending;
mod player;
pub mod profile;
pub mod relayout;
pub mod rng;
mod setup;
mod stack;
pub mod state;
pub mod table;
pub mod zone;

pub use bitboard::Bitboard;
pub use ids::{CardIx, LargeSlot, Seat, Slot, SlotId};
pub use layout::{Large, Layout, Small};
pub use observation::Observation;
pub use state::State;
pub use table::GameTable;
