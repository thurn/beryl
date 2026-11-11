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
//! amends its text. Each part names its bead. Every field of [`State`] is private now; the
//! sets, queries and player API below exist only where a link names them.
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
//! The object mutators are built ([`mutate`]): [`State`]'s way to an object to write is
//! private to the crate. An attachment or a base controller changes only as a permanent
//! enters or a spell is cast, so each is part of that mutator.
//!
//! The zones, the battlefield order, each player's knowledge and the derived cache are the
//! crate's too ([`zone`], [`derive::Rederive`]). Only a mutator marks the cache stale, and only
//! [`State::rederive`] clears the mark, so no rules code holds the flag. Whether counters
//! change a layer result is the rules' to say, with the card in hand, so the counter
//! mutators take that answer (`layers::counters_matter`). A zone is named by a
//! [`state::ZoneKey`], whose rustdoc says how a shared zone and a command zone would join it.
//!
//! The players, the turn, the random streams, the profile, the budgets and the outcome are
//! private as well, with a mutator for each thing the rules do to them. The kernel moves the
//! turn as it is told and holds none of its rules: which step follows which, and when a
//! budget stalls the game, are the rules' to say. The random streams are drawn only inside
//! the chance chokepoints ([`zone`], [`chance`]); [`State::reseed`] and
//! [`State::set_profile`] are the two game-level writes a search or a session makes.
//!
//! | The rules | Mutators | What each leaves consistent |
//! | --- | --- | --- |
//! | move an object between zones (CR 400.7), draw, order or shuffle a library, end a token | [`State::move_object`], [`State::draw`], [`State::reorder_library`], [`State::shuffle_library`], [`State::rebind_at_random`], [`State::cease_to_exist`] | the new incarnation and its reset fields; both zones' membership bitboards; the ordered library, graveyard and stack; the battlefield queue and every permanent's index into it; `derived_dirty`; the attachments, combat references and exile links that named it; its identity, settled before anyone sees it ([`chance`]); both players' knowledge |
//! | change a permanent's status, damage or counters | [`State::set_tapped`], [`State::set_summoning_sick`], [`State::reset_permanents_for_turn`], [`State::turn_face_up`], [`State::mark_damage`], [`State::clear_damage`], [`State::add_counters`], [`State::remove_counters`] | a face with its timestamp and who knows it; `derived_dirty` where the kernel can tell the layers read the field |
//! | put a spell on the stack, make a token, have a permanent enter | [`State::become_spell`], [`State::make_token`], [`State::arrive`] with a [`mutate::Arrival`], [`State::link`] | what a permanent enters with (tapped, attacking, its face, controller, counters and what it is attached to) is set as one change, before anything reads it |
//! | declare and end combat | [`State::declare_attacker`], [`State::choose_attacked`], [`State::forget_attacked`], [`State::declare_blocker`], [`State::block`], [`State::mark_struck_first`], [`State::remove_from_combat`], [`State::end_combat`] | the combat flags and the combat reference change together; nothing stays related to a permanent removed from combat |
//! | show or hide a card | [`State::reveal`], [`State::look`], [`State::forget`], and for a paused resolution [`State::restore_knowledge`] and [`State::keep_seen`] | each player's knowledge; the identity is settled first |
//! | store the layers' results | [`State::rederive`], which lends the cache to the one function that fills it ([`derive::Rederive`]) | the derived table by battlefield index, the listening mask, what each face-down permanent's controller sees, `derived_dirty` cleared |
//! | change a player | [`State::gain_life`], [`State::lose_life`], [`State::damage_player`], [`State::add_mana`], [`State::spend_mana`], [`State::empty_mana_pools`], [`State::count_land_played`], [`State::declare_mulligan`], [`State::keep_hand`], [`State::count_bottomed`], [`State::count_mulligan`], [`State::add_emblem`] | a pool spends only mana it holds; a hand is kept with what it owes the bottom of the library; a mulligan taken is one to declare again |
//! | move the turn, count toward a budget, end the game | [`State::set_starting_player`], [`State::begin_turn`], [`State::begin_step`], [`State::give_priority`], [`State::pass_priority`], [`State::reset_passes`], [`State::declare_attack`], [`State::set_cleanup_check`], [`State::count_decision`], [`State::count_state_based_check`], [`State::reset_state_based_checks`], [`State::end_game`] | passes reset with priority; the lands played and the decisions offered begin again with the turn; that attackers were declared ends with the combat ([`State::end_combat`]) |
//! | use the stack | [`State::put_on_stack`], [`State::take_top_of_stack`], [`State::take_off_stack`], which returns the entry, [`State::announce_x`], [`State::announce_target`] | a spell's entry goes on once its card is in the stack zone and comes off before the card leaves, so the zone move keeps the stack's set of objects and these keep its order |
//! | queue triggers | [`State::queue_trigger`], [`State::take_trigger`], [`State::delay_trigger`], [`State::fire_delayed`] | a delayed trigger that fires waits with the others, once |
//! | open and settle a question | [`State::ask`], [`State::settle_question`], [`State::pause_resolution`], [`State::record_answer`] | a question is open or priority is; a paused resolution keeps what each player knew as it began |
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
//! such as a zone change's [`event::Reason`] or the source of damage. Bookkeeping no player
//! sees as an event (passes, budgets, the open question, the derived cache) returns nothing,
//! and neither does a change the rules announce as part of a larger one: what a permanent
//! enters with is part of its zone change, and each pick of a combat declaration is part of
//! the declaration, announced once it is whole (CR 508.1, 509.1): [`State::declare_attack`]
//! returns that event. Spending mana and the steps of a mulligan are no event today.
//! [`State::end_game`] returns none either: the rules announce a win or a loss to each
//! player with its cause, and a stall, which is no rules outcome, to no one. It may replace
//! an outcome already there, because a state that ran out of a capacity is abandoned. The
//! zone move itself returns nothing: its event carries the reason the pipeline moved it for,
//! and the deal and a mulligan move cards before the game's first turn with no event at all
//! (CR 103.5).
//!
//! ## Entities and records
//!
//! [`State`], [`state::Object`], [`state::Player`], [`state::Turn`] and [`state::Zones`] are
//! entities: private fields, read through `#[inline]` getters, written by mutators. An
//! object has its getters, one for each field and one for each status flag, and so do a
//! player and the turn, one for each field; their fields are `pub(crate)` for the kernel's own
//! mutators, and nothing outside the crate can read or write one directly. An
//! object's definition index and base controller are private already: they are read
//! through the queries below.
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
//! Where a record names an entry of one of a definition's lists, the field has that list's
//! index type ([`beryl_card::index`]): a target's [`beryl_card::SpecIx`], a trigger's
//! [`beryl_card::AbilityIx`], a grant's [`beryl_card::GrantIx`]. None is a bare byte, so one
//! cannot be read as another.
//!
//! ## The board door
//!
//! [`State::board`] lends a [`board::Board`], the one handle that arranges a position with no
//! event and no rules action: `place`, `place_token`, the `set_*` methods, `attach`,
//! `swap_cards`, `add_emblem`, `order_library`, `begin_at`, `begin_before_turn`,
//! `set_flip_winner`, `renumber_deck_slots` and `reverse_deck_slots`, and `corrupt`, which
//! breaks a state for the tests of the invariant check. Scenarios, checkpoints, tools and
//! tests go through it. A card body is lent the state shared, so it cannot open the door;
//! rules code never names it, and a search for `.board()` finds whoever does.
//!
//! ## The legacy accessor
//!
//! [`State::legacy_mut`] lends a [`legacy::Legacy`], which writes, raw, what milestone 4
//! (by-w8gv.7.1) deletes, which therefore gets no typed mutator: the `effects` and
//! `permissions` queues, which become stores whose payload is a resolved form of
//! `Modification`; `entered_x`; `Player::sorceries_as_flash`; [`state::CastPermission`]'s
//! `extra`, `exile` and `flashback`; the [`state::ObjectFlags`] `KICKED`, `OFFSPRING`,
//! `IMPENDING`, `EVOKED`, `EXILED_UNTIL`, `EXILED_WITH` and `LOYALTY_ACTIVATED`; and
//! [`state::Counters`]' long-tail kind (`other_kind`, `other`). [`legacy::Legacy`]'s rustdoc
//! lists each with the store that replaces it. Reads use the getters. Milestone 4 deletes
//! the accessor with the last of them. The zone move and the turn's resets clear those flags
//! inside the kernel, with everything else an object forgets.
//!
//! ## Sets and orders
//!
//! Outside the kernel a [`Bitboard`] is a set: union, intersection, difference, `contains`,
//! `count`, `is_empty`, [`Bitboard::any`], [`Bitboard::all`], [`Bitboard::at_least`],
//! [`Bitboard::filter`], [`Bitboard::sum`] and [`Bitboard::sole`], its one member. It does
//! not iterate, it has no first member and its words are the crate's. Each of those takes a
//! `Fn`, which cannot carry what it saw from one member to the next, so its answer is the
//! same in any order. [`PlayerSet`] is the same for players. A caller that needs a sequence
//! asks for an order by name:
//!
//! | Order | Query |
//! | --- | --- |
//! | the battlefield, as permanents entered it | [`State::battlefield`] |
//! | a library, top first | [`State::library_from_top`] |
//! | a graveyard, as cards were put there (CR 404.2) | [`State::graveyard`] |
//! | players in turn order (CR 101.4) | [`State::players_in_turn_order`] and the others under "Players" |
//! | a batch, in an order no seed decides: permanents as they entered, then library cards top first, then the rest by name | [`State::in_game_order`] |
//! | a decision's candidates, by ascending slot | [`State::in_candidate_order`], [`Observation::in_candidate_order`], [`decision::Candidates::objects`] |
//!
//! The last is not a rules order, since the deal and every shuffle put cards in slots at
//! random. A `Choice` is an index into candidates listed that way, so saves and
//! `rollouts_never_change` depend on it. It lists candidates. Anything else that walks it
//! says in a comment why its result is the same in any order: it builds a set or a sum, it
//! sorts next, or it does to each member something no event shows. A walk whose order an
//! event, a queue or a zone would show takes a rules order instead.
//!
//! A stack entry's targets are named the same way: a [`state::TargetPlace`] is a target's
//! place in the entry's list, a [`state::TargetPlaces`] is a set of them with `insert`,
//! `contains` and `is_empty`, and [`state::placed`] gives the targets with their places in
//! the order they were chosen (CR 601.2c).
//!
//! ## Queries
//!
//! Other crates read derived answers, not fields ([`query`]). A slot's definition index and
//! its base controller have no getter outside the crate: each is asked for by the rule that
//! wants it.
//!
//! | The rules ask | Query |
//! | --- | --- |
//! | the card that represents an object: nothing for a token (CR 111.1, 111.6) | [`State::card_behind`] |
//! | what it is before any other effect, and what a copy of it would be (CR 707.2) | [`State::copiable`], a [`Copiable`] |
//! | its mana value: a spell's with its X (CR 202.3e), a transformed permanent's front face's and a copied back face's 0 (CR 712.8e) | [`State::mana_value`] |
//! | who controls it: a permanent's layer result, a spell's caster, elsewhere the owner (CR 108.4a) | [`State::controller`] |
//! | its power and toughness: the layer results on the battlefield, the copiable values elsewhere | [`State::power`], [`State::toughness`] |
//!
//! A [`Copiable`] is a definition, the face of it that is up and a copy effect's exception
//! (CR 707.9b). It answers with the face's characteristics ([`Copiable::face`], the blank
//! ones of CR 708.2 for a face-down permanent), the table's facts about that face, its name,
//! and its power and toughness. It never hands out the index it holds:
//!
//! - [`Copiable::record`] is the index and face a record keeps and a token copy is made of,
//!   which for a face-down permanent are the public blank card's, never its hidden card's.
//!   [`State::make_token`] takes a `Copiable`, so no token is made of an index read off an
//!   object.
//! - [`Copiable::key`] is a [`query::DefinitionKey`]: it compares, orders and indexes a
//!   table kept per definition, and names no card.
//! - [`Copiable::turned`] asks the same of another face: the spell a card would be cast as,
//!   or the permanent it would enter as.
//!
//! What only a card has is asked of [`State::card_behind`], which a caller must answer for a
//! token too: the faces it may be cast or played with, the cost to turn it face up.
//! [`State::definition_name`] is the definition's name whichever face is up, for an order
//! no seed decides and for tools. A player's view goes through [`Observation::card`], which
//! is nothing for a card its viewer does not know. The queries cost what the reads they
//! replaced did: none caches anything in the state.
//!
//! ## Players
//!
//! Rules code reaches players through sets and orders, never the other player of two.
//! [`PlayerSet`] is the order-free set (`beryl_card`'s `Players` is the vocabulary's word for
//! which players an effect names, so the set is not called that): `contains`, `count`,
//! `is_empty`, [`PlayerSet::any`], [`PlayerSet::filter`], [`PlayerSet::without`] and
//! [`PlayerSet::sole`], its one member. It does not iterate. The state names the sets and
//! puts one in order:
//!
//! | The rules ask | Query |
//! | --- | --- |
//! | every player (CR 102.1) | [`State::players`] |
//! | a player's opponents (CR 102.2, 102.3) | [`State::opponents_of`] |
//! | the defending players of this turn's combat (CR 506.2, 802.2) | [`State::defending_players`] |
//! | the defending player an attacker attacks (CR 508.5) | [`State::defending_player`] |
//! | the next player in turn order: who takes the next turn, who gets priority on a pass (CR 103.1, 117.3d) | [`State::next_player`] |
//! | players in the order they act, from the active player (CR 101.4) | [`State::players_in_turn_order`] |
//! | the same from a named player, as from the starting player (CR 103.5) | [`State::players_in_turn_order_from`] |
//! | players by seat: a decision's candidates, the events of one simultaneous happening | [`State::players_in_seat_order`] |
//! | who knows an object's identity | [`State::who_knows`] |
//! | every object in a zone, whoever owns it (CR 400.1) | [`State::in_zone`] |
//!
//! Seat order is to players what candidate order is to objects: no order of the rules, and
//! what saves and `rollouts_never_change` see. A walk in it lists players or says in a
//! comment why its result is the same in any order.
//!
//! Each of these answers for the two-player game, and its rustdoc cites the rule that
//! changes the answer with more players: no multiplayer logic is written (plan K7).
//! `Seat` has no `opponent`. The shortcut is the trait [`two_player::TwoPlayerSeat`], in
//! scope only where a module imports it: the kernel's turn order and test door, and callers
//! outside the rules that mean the other seat at this table (a view's "opponent", an
//! evaluation's other side, tests). Rules modules and card bodies never import it.
//!
//! ## Layouts
//!
//! How many players a game has, how many cards a deck, a library and a graveyard may hold,
//! how many tokens and spell copies exist at once and how many objects there are in all are
//! parameters of the [`Layout`], and nothing outside the layouts assumes them (by-w8gv.5.14;
//! the card-index width is by-w8gv.5.15's):
//!
//! | parameter | where a layout sets it |
//! |---|---|
//! | players | [`SlotId::PLAYERS`] of its slots; [`Layout::PerPlayer`], the table with one entry each |
//! | deck, library and graveyard size | [`SlotId::DECK_SLOTS`]; [`Layout::Deck`], the queue that long |
//! | tokens and spell copies | the pool, [`SlotId::POOL`] to [`SlotId::COUNT`]: [`SlotId::pool`], [`Layout::TOKEN_POOL`] |
//! | objects in all | [`SlotId::COUNT`] |
//!
//! [`Small`] is two players, 60-card decks and 255 slots, and stays within 20 KiB;
//! [`Large`] is two players, 256-card decks and 1,024 slots. Code generic over the layout
//! asks `L::Slot` and builds `L::PerPlayer` and `L::Deck` values; a value that is the same
//! in every layout (a [`profile::Profile`]'s stops, a [`GameTable`]'s decks) is a
//! [`layout::PerSeat`], one entry for each seat `beryl_card` names. `Seat` stays the
//! two-variant enum it is, so `Seat::BOTH` still walks the players inside the kernel: a
//! third player needs a third seat first.
//!
//! The numbering of slots belongs to the slot type, not to the layout beside it, so a
//! conversion between two numberings needs only their types and a slot number of one cannot
//! pass for a number of the other. A slot keeps its [`ids::Place`] in every numbering (the
//! `i`th card of a player's deck, the `k`th slot of the pool) and its order among the
//! others, so [`relayout::Relayout`] takes a game to another layout place by place and a
//! `Choice` over objects names the same object in each. The UI's protocol names an object
//! by its number among [`LargeSlot`]s whatever layout its game is in ([`SlotId::num`]). A
//! layout with other deck sizes is a new slot type and a new `impl Layout`.
//!
//! A game that does not fit a layout is refused where a state of that layout is made, with
//! a [`Misfit`] and never a panic: [`State::deal`] refuses a deck with more cards than the
//! layout's `DECK_SLOTS`, and [`State::try_relayout`] a game with a deck, a token or a list
//! past what the target holds. [`GameTable`] is not generic over the layout and refuses no
//! deck for its size.

pub mod bitboard;
pub mod board;
pub mod chance;
pub mod decision;
pub mod derive;
pub mod event;
pub mod filter;
mod fit;
pub mod fixed;
mod floating;
pub mod ids;
pub mod invariants;
mod knowledge;
pub mod layout;
pub mod legacy;
pub mod mutate;
mod new;
mod object;
pub mod observation;
mod order;
mod pending;
mod player;
mod player_set;
mod players;
pub mod profile;
pub mod query;
pub mod relayout;
pub mod rng;
mod stack;
pub mod state;
pub mod table;
mod turn;
pub mod two_player;
pub mod zone;

pub use bitboard::Bitboard;
pub use fit::Misfit;
pub use ids::{CardIx, LargeSlot, Place, Seat, Slot, SlotId};
pub use layout::{Large, Layout, Small};
pub use observation::Observation;
pub use player_set::PlayerSet;
pub use query::Copiable;
pub use state::State;
pub use table::GameTable;
