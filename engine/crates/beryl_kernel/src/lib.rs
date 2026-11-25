//! Beryl's kernel: the game state and the only code that writes it.
//!
//! Implements: none. Documents the kernel contract.
//! The kernel owns [`State`] and its storage [`Layout`]s, the slots and references that name
//! objects, the bitboards and fixed queues it is built from, the per-game card table, the
//! zone-move and chance chokepoints, the invariants, and the records, decisions and events
//! that pass between it and the rules. It depends on `beryl_card` alone and holds no rule of
//! the game's flow: turns, priority, casting, mana, combat, layers, triggers and the
//! interpreter are `beryl_rules`'s, which reaches the state through the API below.
//! Kernel modules group stored state and declare rules their operations enforce.
//! [`Layout`] names no rules type: what the rules keep once per layout they read through
//! [`layout::PerLayout`].
//!
//! # The contract
//!
//! Only the kernel writes a field. This text describes the kernel as it is built; a change
//! that makes part of it untrue amends it in the same commit. Every field of [`State`] is
//! private; the sets, queries and player API below exist only where a link names them.
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
//! [`State::rederive`] clears the mark, so no rules code holds the flag. Besides the
//! battlefield's own changes, the static abilities on the battlefield may read what no layer
//! writes, as a characteristic-defining ability reads the graveyards: each face declares such
//! inputs ([`static_ability::Inputs`]), the recompute stores those of the permanents, and the
//! zone move marks the cache stale for a graveyard only while one is in that set. The counter
//! mutators take whether the layers read the counters as an argument, which
//! [`State::layers_read_counters`] answers from the same set. A zone is named by a
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
//! | move an object between zones (see CR 400.7), draw, order or shuffle a library, end a token | [`State::move_object`], [`State::draw`], [`State::reorder_library`], [`State::shuffle_library`], [`State::rebind_at_random`], [`State::cease_to_exist`] | the new incarnation and its reset fields; both zones' membership bitboards; the ordered library, graveyard and stack; the battlefield queue and every permanent's index into it; `derived_dirty`, for a graveyard while a permanent reads the graveyards; the attachments, combat references and exile links that named it; what the stores held about it, which ends ("Stores"); its identity, settled before anyone sees it ([`chance`]); both players' knowledge |
//! | change a permanent's status, damage or counters | [`State::set_tapped`], [`State::set_summoning_sick`], [`State::reset_permanents_for_turn`], [`State::turn_face_up`], [`State::transform_permanent`], [`State::flip_permanent`], [`State::mark_damage`], [`State::clear_damage`], [`State::add_counters`], [`State::remove_counters`], [`State::order_timestamps`] | a face with its timestamp and who knows it; permanents reordered keep the timestamps they had among them; `derived_dirty` where the kernel can tell the layers read the field |
//! | phase permanents out and in (see CR 702.26) | [`State::phase_out`], [`State::phase_in`] ([`phasing`]) | the zone, incarnation and stores untouched; the battlefield queue, the owner's battlefield bitboard and the set of phased-out permanents; what is attached phases with its permanent, and what phased out alone phases in unattached once its permanent left; combat references to it end; who it phases in for, where that is not its base controller ([`memos::Memo::PhasedOutUnder`]); `derived_dirty` |
//! | put a spell on the stack, make a token, have a permanent enter, merge a mutating spell with a permanent (see CR 731), put a meld card under the permanent its partner melded into (see CR 713.6) | [`State::become_spell`], [`State::make_token`], [`State::arrive`] with a [`mutate::Arrival`], [`State::remember`], [`State::merge`], [`State::meld_under`] | what a permanent enters with (tapped, attacking, its face, controller, counters and what it is attached to) is set as one change, before anything reads it; a merged permanent stays the object it was, its other cards in [`beryl_card::Zone::Merged`] with a [`memos::Memo::PartOf`], its values a copy effect's: the top card's face and every ability of each other card ([`copiable::additions::Addition::every_ability`]); its cards leave with it ([`State::move_object`], see CR 731.3) |
//! | declare and end combat | [`State::declare_attacker`], [`State::choose_attacked`], [`State::declare_blocker`], [`State::block`], [`State::mark_struck_first`], [`State::remove_from_combat`], [`State::end_combat`] | the combat flags and the combat reference change together; nothing stays related to a permanent removed from combat |
//! | show or hide a card | [`State::reveal`], [`State::look`], [`State::forget`], and for a paused resolution [`State::keep_seen`] | each player's knowledge; the identity is settled first |
//! | store the layers' results | [`State::rederive`], which lends the cache to the one function that fills it ([`derive::Rederive`]) | the derived table by battlefield index, the listening and replacing masks, the rules the static abilities change for each player, the layer inputs the static abilities read ([`static_ability::Inputs`]), whether an effect removed abilities (for [`State::has_ability`]) or changed land types (for [`State::makes_card_mana`] and [`State::intrinsic_mana`]), whether any permanent is face down (for [`State::any_face_down`]), what each face-down permanent's controller sees, `derived_dirty` cleared |
//! | change a player | [`State::gain_life`], [`State::lose_life`], [`State::damage_player`], [`State::add_mana`], [`State::spend_mana`], [`State::add_remembered_mana`], [`State::spend_remembered_mana`], [`State::empty_mana_pools`], [`State::count_land_played`], [`State::declare_mulligan`], [`State::keep_hand`], [`State::count_bottomed`], [`State::count_mulligan`], [`State::add_emblem`] | a pool spends only mana it holds, and holds at most four entries of mana that remembers in [`Small`] (a fifth is `Full`); a hand is kept with what it owes the bottom of the library; a mulligan taken is one to declare again |
//! | move the turn, count toward a budget, end or restart the game | [`State::set_starting_player`], [`State::begin_turn`], [`State::begin_step`], [`State::give_priority`], [`State::pass_priority`], [`State::reset_passes`], [`State::declare_attack`], [`State::set_cleanup_check`], [`State::count_decision`], [`State::count_state_based_check`], [`State::reset_state_based_checks`], [`State::end_game`], [`State::restart`] | passes reset with priority; the lands played and the decisions offered begin again with the turn; that attackers were declared ends with the combat ([`State::end_combat`]), while tracked attackers are remembered for the turn |
//! | use the stack | [`State::put_on_stack`], [`State::take_top_of_stack`], [`State::take_off_stack`] and [`State::take_ability_off_stack`], which return the entry, [`State::announce_x`], [`State::announce_target`], [`State::record_spent_on_x`] | a spell's entry goes on once its card is in the stack zone and comes off before the card leaves, so the zone move keeps the stack's set of objects and these keep its order; an entry taken from below the top repairs the ability targets above it, which name abilities by their place ([`state::Target`]); a spell's entry taken off without resolving is kept when an ability still on the stack or waiting to be put there needs the departed spell's characteristics ([`State::departed_spell`]); the mana spent on an ability's X is kept only in `Large`, and recording it in `Small` is `Full` ([`state::SpentMana`]) |
//! | queue triggers | [`State::queue_trigger`], [`State::take_trigger`], [`State::delay_trigger`], [`State::fire_delayed`] | a delayed trigger that fires waits with the others, once |
//! | keep and forget last-known information (see CR 603.10a) | [`State::record_last_known`], [`State::forget_last_known`] | one row for each object that left, copied from its current layer results before it moves; a row no waiting trigger, stack entry or delayed trigger names is forgotten |
//! | open and settle a question | [`State::ask`], [`State::settle_question`], [`State::pause_resolution`], [`State::resume_resolution`], [`State::record_answer`] | a question is open or priority is; a paused resolution keeps what each player knew as it began, and only resuming it gives that back |
//! | start and end what a card creates | [`State::start_effect`], [`State::start_effects`], [`State::start_effect_on_set`], [`State::expire`] | an effect's timestamp; the kinds live in its store; the slots a store has entries about; a locked-in set's place in the store's table; `derived_dirty` where the entry is a layer input |
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
//!   records, so no mutator takes one and no event exists without its change.
//!
//! The checks deny warnings (clippy with `-D warnings`), so dropping an event does not pass
//! them: no visible change happens without one. A mutator takes as an argument what the event needs and the state cannot say,
//! such as a zone change's [`event::Reason`] or the source of damage. Bookkeeping no player
//! sees as an event (passes, budgets, the open question, the derived cache) returns nothing,
//! and neither does a change the rules announce as part of a larger one: what a permanent
//! enters with is part of its zone change, and each pick of a combat declaration is part of
//! the declaration, announced once it is whole (see CR 508.1, 509.1): [`State::declare_attack`]
//! returns that event. Spending mana and the steps of a mulligan are no event today.
//! [`State::end_game`] returns none either: the rules announce a win or a loss to each
//! player with its cause, and a stall, which is no rules outcome, to no one. It may replace
//! an outcome already there, because a state that ran out of a capacity is abandoned. The
//! zone move itself returns nothing: its event carries the reason the pipeline moved it for,
//! and the deal and a mulligan move cards before the game's first turn with no event at all
//! (see CR 103.5).
//!
//! ## Entities and records
//!
//! [`State`], [`state::Object`], [`state::Player`], [`state::Turn`] and [`state::Zones`] have
//! kernel-owned fields, read through getters and written by mutators. Definition indices and
//! base controllers are read through queries. Rules code builds records such as
//! [`state::StackEntry`], [`state::PendingTrigger`], [`state::Target`] and [`state::Casting`].
//! [`state::Derived`] carries layer results into the kernel. [`State::decider`] sets a
//! [`decision::Decision`]'s answerer for player control (see CR 724). [`state::Resolving`] holds the
//! kernel's record of what each player knew at the start of the resolution. [`beryl_card::SpecIx`],
//! [`beryl_card::AbilityIx`] and [`beryl_card::GrantIx`] identify entries in separate lists.
//!
//! ### Object references
//!
//! A [`state::ObjectRef`] holds a slot and its incarnation. Zone moves, library rebinds and
//! returning a token slot to the pool increment it. The increment past 65,535 returns
//! [`fixed::Full`] in both layouts, so the 65,536th change stalls the game with earlier
//! references still stale.
//!
//! Rules actions take [`reference::Permanent`] for battlefield actions, [`reference::Spell`]
//! for spell objects and [`reference::CardIn`] or [`reference::Cards`] for cards in one zone.
//! Ability entries use stack places. At an old caller use
//! `let Some(permanent) = s.permanent(raw) else { return Ok(()); };`. Decisions and open
//! questions keep bare slots, which name objects only in the state that asked ([`decision::Opt`]).
//! Check [`State::current`] immediately before mutating through a stored reference. Phasing
//! keeps the same incarnation, so validate a stored battlefield reference with
//! [`State::permanent`] before use. For a slot from the current battlefield or a new arrival,
//! [`State::permanent_at`] gets the current reference.
//! [`State::phasing_in_for`] selects phased-out permanents for [`State::phase_in`].
//! [`reference::Permanents`] and [`reference::Cards`] select slots for one instruction.
//! Use `.slots()` for batch events and unconverted calls. For sequential actions on permanents
//! use [`State::next_permanent`] to read them in battlefield order.
//!
//! [`memos::Memo::ExiledWith`] survives its source leaving (see “Duration and expiry”).
//! A [`memos::Memo::PartOf`] component leaves with its merged permanent. Zone departure
//! clears attachments and combat references to that slot, including
//! [`state::AttackTarget::Planeswalker`]. [`effects::Affects::Object`] stores both the slot
//! and incarnation, exposed together as an [`state::ObjectRef`].
//!
//! ## The board door
//!
//! [`State::board`] lends a [`board::Board`], the one handle that arranges a position with no
//! event and no rules action: `place`, `place_token`, the `set_*` methods, `attach`,
//! `swap_cards`, `add_emblem`, `order_library`, `begin_at`, `begin_before_turn`,
//! `set_flip_winner`, `renumber_deck_slots` and `reverse_deck_slots`, and `corrupt`, which
//! breaks a state for the tests of the invariant check. Scenarios, checkpoints, tools and
//! tests go through it. A card body is lent the state shared, so it cannot open the door,
//! and rules code has no use for it: the `board-door` check in the `names` lane refuses
//! `.board()` and `board::Board` in `beryl_engine` and `beryl_cards` outside their tests.
//!
//! ## Stores
//!
//! A struct field holds what the Comprehensive Rules give a player, an object or the turn.
//! What a card creates lives in a store. **Effects** holds continuous effects from resolved
//! spells and abilities. **Memos** holds how an object was cast or entered, its Class level
//! and whether it was solved. **Trackers** holds what happened this turn that a card reads.
//! [`State::signature`] compares a slot's entries in these stores and [`counters`].
//!
//! ### Where everything goes
//!
//! | What milestone 3 left | Where it goes | Bead |
//! | --- | --- | --- |
//! | the `effects` queue, `Continuous::Layer` | Effects: an entry on an object, payload [`effects::Payload::Layer`] of a [`effects::LayerChange`] | by-w8gv.7.2, built |
//! | `Continuous::ExileInsteadOfGraveyard` | Effects: an entry on an object, payload [`effects::Payload::Move`] | by-w8gv.7.2, built |
//! | `Player::sorceries_as_flash` | Effects: an entry on a player, payload [`effects::Payload::Rule`], lasting until that player's next turn | by-w8gv.7.3, built |
//! | the `Rule` mask (`rule_bit`, a `u8`) | [`rule_changes::Rules`], a set of the rule changes a game numbered; a static ability's rule changes for players are found with the layers ("Static rule changes") | by-w8gv.7.3, built; numbered by-w8gv.9.7 |
//! | the `permissions` queue, `CastPermission`; its `this_turn` | Effects: an entry on the card, payload [`effects::Payload::Cast`] of an [`effects::Permission`] naming the player and the terms the spell is cast on; `this_turn` is [`store::Duration::EndOfTurn`], and either way it ends with the card's object. [`State::cast_permissions`] reads them | by-w8gv.7.4, built |
//! | `CastPermission::extra`, `Casting::extra` | Effects: an [`effects::Payload::Cost`] on the spell, started as it is cast from the permission's terms. Before the cast the cost is read of the permission; from then on of the cost changes on the spell ([`State::cost_increase`]) | by-w8gv.7.4, built |
//! | `CastPermission::exile` and `flashback`, `StackEntry::exile` and `flashback` | the permission's terms carry them to the cast, which starts an [`effects::Payload::Move`] on the spell: exile instead of a graveyard, or [`effects::MoveReplacement::Flashback`], exile instead of anywhere else (see CR 702.34a). Every move of the spell applies it where it applies any replacement (the rules' `replacement::moving`), and "was a flashback cost paid" is whether that replacement is on the spell | by-w8gv.7.4, built |
//! | `State::entered_x` | Memos: the permanent's `X` ([`memos::Memo::X`]) | by-w8gv.7.6, built |
//! | `ObjectFlags` `KICKED`, `OFFSPRING`, `IMPENDING`, `EVOKED` | Memos: the costs paid as the object was cast ([`memos::Memo::Paid`]), one entry holding the set of them, with a kind in the store's index for each kind of casting option of the vocabulary's list | by-w8gv.7.6, built |
//! | `EXILED_UNTIL`, `EXILED_WITH`, `Object::linked`, `State::link`, `State::forget_links_to` | Memos: a memo of the exiled card naming the slot of what exiled it ([`memos::Memo::ExiledWith`], [`memos::Memo::ExiledUntil`]; "Duration and expiry" says how the first outlives that object), read by [`State::exiled_with`] and [`State::exiled_until`] | by-w8gv.7.7, built |
//! | a field naming the creature an Aura put onto the battlefield | Memos: a memo of the creature naming the Aura by slot and incarnation ([`memos::Memo::PutOntoBattlefieldWith`]), which outlives the Aura as [`memos::Memo::ExiledWith`] does, read by [`State::put_onto_battlefield_with`] | by-bajq.8.2, built |
//! | `LOYALTY_ACTIVATED`, `ENTERED_THIS_TURN` | Trackers, keyed by permanent: a kind of [`trackers::Tracker`] each, noted with [`State::track`] and read with [`State::tracked`] | by-w8gv.7.8, built |
//! | [`state::Counters`]' long-tail pair (`other_kind`, `other`) | the counters table ([`counters::CounterTable`]), a row for each kind on each object, beside the three kinds the rules themselves read, which stay fields of the object. The kinds are the vocabulary's list (`beryl_card::counter_kinds`), and [`State::add_counters`], [`State::remove_counters`] and [`State::counters_of`] take any of them | by-w8gv.7.9, built |
//! | `Flag::OneOne` | Effects: a copy effect on the token ([`effects::Payload::Copy`]) holding its copiable values with their exceptions ([`copiable::additions::Addition`], see CR 707.9b), behind [`state::Flag::Copy`] so that [`State::copiable`] of any other object is one bit test. The zone move carries a token's copy effect to its next incarnation ([`effects::copy`]). A card under a copy effect, such as one that entered as a copy ([`State::enter_as_copy`]) or became one ([`State::become_copy`]), remembers the card it really is ([`memos::Memo::Really`]), which [`State::card_really_is`] and [`State::definition_name`] read and the zone move makes it again | by-bajq.6.1 to 6.3, built |
//!
//! A fact about a permanent that ends with the turn (a loyalty ability was activated, it
//! entered this turn) is a tracker keyed by object, not a memo with an expiry. A memo is
//! written once, as the object is cast or enters, and read by that object's own abilities;
//! a tracker is reset whole by the turn and exists only in a game with a card that reads
//! it. Kept as memos, each turn's reset would walk a keyed list and each read search it;
//! as a tracker the fact is one bit of a set of slots. The list of kinds
//! ([`trackers::Tracker`]) is closed and holds what a card of the pool reads. How many
//! times a player did something this turn (spells cast, cards drawn, cards drawn in their
//! draw step) is the store's second shape, a count kept for each player
//! ([`trackers::Count`]), noted with [`State::count`] and read with [`State::counted`]. A
//! count of the whole game, or one that outlasts the turn, joins with a shape and a span of
//! its own.
//!
//! ### Shapes, capacities and bytes
//!
//! | Store | In [`Small`] | Entry | Capacity | Bytes | In [`Large`] |
//! | --- | --- | --- | --- | --- | --- |
//! | Effects | a queue in the order the effects began, and a mask of the kinds live in it | 24 | 64 ([`effects::EFFECTS`]) | 1,552 | a `Vec` of 32-byte entries |
//! | the keyed slots, shared by every store | one set of slots | | | 32 | 128 |
//! | Effects' locked-in sets | a table of sets of slots, which an entry names by index; an empty set is a free place, and the table ends at its last set in use | 32 | 4 ([`effects::SETS`]) | 136 | a `Vec` |
//! | the rules changed for each player | two sets of rules for each: those of the permanents' static abilities, stored with the layer results, and those of the Effects store's entries, its index | 8, a bit for each rule change the game numbered | 2 players, twice | 32 | the same |
//! | Memos | a queue of (slot, memo) and a mask of the kinds live in it | 4, of the 4 allowed | 32 ([`memos::MEMOS`]) | 136, of the 136 allowed | a `Vec` of 4-byte entries |
//! | Trackers | a set of slots for each kind keyed by permanent | 32 | 3 kinds | 96 | 128 each |
//! | Trackers' counts | a byte for each player and kind of count, padded to the sets' alignment | 1 | 3 kinds, 2 players | 8 | the same |
//! | Counters | a queue of (slot, kind, count), in the order each kind was first put on its object | 3 | 32 ([`counters::COUNTERS`]) | 97, of the 104 allowed | a `Vec` |
//!
//! A full fixed-layout store returns [`fixed::Full`] so the game can retry in `Large`.
//! A list that cannot fill says why in a comment and takes
//! [`layout::Queue::push_bounded`], which checks the argument in debug builds; a list a
//! heuristic samples takes [`layout::Queue::push_if_room`]; the `discarded-push` check in
//! the `names` lane refuses a push whose result is bound to `_` in the kernel,
//! `beryl_engine` and `beryl_cards` outside their tests. A locked-in set is 32 bytes, so the sets are a
//! table of their own and never part of an entry. Counter kinds cannot be fields of each of
//! 255 objects, so all but the three hot kinds are a keyed table.
//!
//! `size_of::<State<Small>>()` was 19,992 before the milestone and is 20,040 after by-w8gv.7.2: 40
//! for the store's mask and the keyed slots, 8 of padding for the field order ([`State`]'s
//! rustdoc). It is 20,040 after by-w8gv.7.3 too: the four bytes of rule sets are those
//! `sorceries_as_flash` and its padding gave back. After by-w8gv.7.4 it is 19,832: the
//! `permissions` queue gave back 80 and each of the stack's 64 entries two bytes,
//! [`state::StackEntry`] being 58 bytes without its two flags. After by-w8gv.7.6 it is 19,880: the
//! Memos store's 100 bytes less the 50 of `entered_x`. After by-w8gv.7.5, which landed after it, it
//! is 20,016: the table of locked-in sets, 136. After by-w8gv.7.8 it is 20,080: the Trackers
//! store's two sets of slots, 64. After by-w8gv.7.9 it is 20,176: the table of counters, 96. After
//! by-w8gv.9.7 it is 20,216: each of the players' four sets of rule changes is a word, where it was
//! a byte. It is 20,088 since [`state::StackEntry`] holds a [`state::StackItem`], a spell, an
//! activated ability or a triggered ability with only the fields of its kind: each of the 64
//! entries is 56 bytes, two fewer. It is 20,104 since what the permanents listen to is a 64-bit
//! [`event::Listens`], where it was 16 bits, beside a 64-bit [`event::Replaces`] of what their
//! static abilities replace: 16 more. After by-bajq.1.4 it is 20,112: the Trackers store's counts
//! for each player, 8. It is 20,056 since last-known information is a table of the state
//! ([`last_known`]): each of the 96 waiting and delayed triggers is 6 bytes smaller and each of the
//! 64 stack entries 6, 960 in all, and the table of 16 rows of 56 bytes, with its length, takes 904
//! of them. It was 20,184 when ability removal began (128 bytes that no change reported came
//! between), and is 20,336 since each of the 128 [`state::Derived`] rows holds a mask of the
//! abilities its permanent lost, packed to two-byte alignment, and 20,344 since a value chosen as a
//! permanent enters. It is 19,840 since [`state::Object`] is aligned to two bytes and 22 bytes
//! long, where it was 24: 504 fewer, the 510 of the 255 slots less 6 of padding. It is 19,856 since
//! the list of effects that reach other zones ([`query::zone_characteristics`]): four entries of 3 bytes and the
//! list's length, 16 with padding. It is 19,888 since a [`query::Copiable`] holds two copy
//! exceptions: each of the 16 last-known rows is 60 bytes, 2 more. It is 19,920 since each player's
//! pool holds up to four entries of mana that remembers ([`mana_memory`]), four bytes each with the
//! queue's length. It is 20,008 since the table of four sets of text changes that abilities on the
//! stack keep (88 bytes); the byte naming one in each stack entry fits the entry's padding. It is
//! 20,016 since a decision names the player who answers it ([`decision::Decision::decider`]): the
//! question a paused resolution keeps is 8 bytes longer, the decision having no spare byte;
//! [`state::Turn::controller`] fits the turn's padding. It is 20,128 since spells that left the
//! stack are kept for abilities of their own that copy them ([`last_known::DepartedSpell`]): two
//! rows of 56 bytes, 112. It is 20,144 since the state counts resolutions in a row with no choice
//! and keeps the hash a loop of mandatory actions is compared with ([`repeats`]): a `u16` and a
//! `u64`, 16 with padding. It is 20,184 since a [`query::Copiable`] holds three copy exceptions,
//! 40 bytes more in all. It was 20,216 when phasing began (32 bytes that no change reported
//! came between), and is 20,248 since the set of phased-out permanents ([`phasing`]), 32 bytes.
//! It is 20,456 with the turn plan, 20,464 with [`memos::Link`] and 20,472 with the face-down
//! flag ([`State::any_face_down`]). The limit is 20,480. Each bead may change the size at its
//! own push by at most its allotment, whatever landed before it, and reports it as the count:
//!
//! | Bead | Allotment | Why |
//! | --- | --- | --- |
//! | by-w8gv.7.4 | -200 or lower (landed: -208) | the `permissions` queue (80) and two bytes of each of 64 stack entries go; permissions share the 64 effects |
//! | by-w8gv.7.5 | +136 (landed: +136) | the locked-in sets |
//! | by-w8gv.7.6 | +88 (landed: +48) | the Memos store less `entered_x` (50) |
//! | by-w8gv.7.8 | +64 (landed: +64) | two sets of slots |
//! | by-w8gv.7.9 | +104 (landed: +96) | the counters table |
//! | by-bajq.1.1 | +16 (landed: +16) | the listener mask grows from 16 to 64 bits, beside a 64-bit mask of what static abilities replace |
//! | by-bajq.1.4 | +8 (landed: +8) | three counts for each of two players, padded |
//! | by-bajq.1.2 | 0 or lower (landed: -56) | last-known information leaves every trigger record for a table of 16 rows |
//! | by-bajq.4.3 | -300 or lower (landed: -504) | each object is 22 bytes, where alignment made it 24 |
//! | by-bajq.5.1 | +48 (landed: +16) | up to four effects that reach other zones, beside the layer results |
//! | by-bajq.5.3 | +136 (landed: +88) | a byte in each of the 64 stack entries naming its captured text changes, and the table of four captured sets |
//! | by-bajq.6.1 | +72 (landed: +32, and +40 by by-bajq.6.10) | copy exceptions in each `Copiable`, which each of the 16 last-known rows holds: two, then three |
//! | by-bajq.7.4 | +48 (landed: +32) | up to four entries of mana that remembers in each player's pool |
//! | by-bajq.8.3 | +16 (landed: +16) | the count of mandatory resolutions in a row and the hash it compares, padded |
//! | by-bajq.8.4 | +32 (landed: +32) | a set of the phased-out permanents' slots |
//! | Turn plan | +160 | the queue of 24 remaining steps and their positions |
//!
//! ### Affected sides and payloads
//!
//! An effect is started as one value, [`effects::Continuous`]: what it does with what it
//! does it to. Its variants are the pairs the game has a meaning for (a layer change, a
//! zone-change replacement, a permission to cast and a cost change on one object; a rule
//! change on a player or an object, [`effects::Subject`]), so no caller can write another,
//! and [`State::start_effect`] takes nothing else. The mutators are the only way into the
//! store, so every entry is one of those pairs or a layer change on a locked-in set, and a
//! reader meets no other: [`State::layer_effects`] gives the permanents each change is on
//! ([`effects::Permanents`], one or a set), [`State::rule_applies`] asks about a
//! [`effects::Subject`], and the queries by kind give what their kind is on.
//!
//! An entry keeps the pair as two halves, the affected side ([`effects::Affects`]) and
//! the payload ([`effects::Payload`]), because the pair as one enum is 32 bytes in
//! [`Small`] and the two apart are 24 (measured in by-w8gv.9.6: a layer change is 16 bytes
//! with an alignment of 8, so a variant holding it beside what it is on is 24 before the
//! entry's source, timestamp and duration). Both stay public to read, for a view, a tool
//! and a comparison of two games.
//!
//! The affected side is one of four: an object, named by slot and
//! incarnation, a player, and a locked-in set, by its index in the store's table (built);
//! the game, which joins with its first user, since no card yet changes a rule for
//! everyone. The fields are the variant's own, so every side fits four bytes.
//!
//! A locked-in set is the objects one effect affects when it affects several (see [CR 611.2c], "the set
//! of objects it affects is determined when that continuous effect begins. After that point, the
//! set won't change."): [`State::start_effect_on_set`] starts one entry for each part of the
//! effect, all with one timestamp (see CR 613.6, 613.7), for the permanents it is given, and the
//! layers apply it to each member
//! ([`State::locked_in`]). A set holds slots, not incarnations: a member that ends leaves its set
//! where every stored thing about it ends ("Duration and expiry"), so whatever a set holds is still
//! the object it was, and a card that comes back is no member. The effect ends with its duration or
//! with its last member, and its place in the table is free again. Only an effect on two objects or
//! more takes places, one for each part: on one it is an entry on that object for each part, the
//! same effect, and on none it never begins. A set is affected by a layer change alone, as a player is by a rule change alone (the
//! store's invariants, which the mutators' arguments keep): nothing else the cards do is one effect
//! on several objects, and each query by kind would otherwise search the sets. An effect cannot be
//! started on another's set: only [`State::start_effect_on_set`] names one, and it makes the set it
//! names.
//!
//! Its payload ([`effects::Payload`]) is a layer change, a zone-change replacement or a
//! rule change, a permission to cast, a cost change, a regeneration shield, a prevention
//! shield, a player-controlling effect ([`effects::control`]) or an extra turn
//! ([`effects::extra_turn`]). A prevention shield is a payload on a player besides a rule
//! change, control and an extra turn, and names the source of damage chosen for it, if any, by an [`object::ObjectRef`] that stops
//! matching once that source changes zones. A payload is a
//! resolved kernel type: every word of the card that needed the resolving ability to
//! settle it is settled, as [`store::Duration`] names the player "your next turn" means. A
//! rule change is one the game numbered ([`rule_changes::RuleId`], a byte), which names no
//! player: the affected side is who it applies to. [`effects::LayerChange`] is the resolved form of `beryl_card`'s
//! `Modification`, whose `Control` names no seat: [`effects::LayerChange::resolve`] gives
//! it the ability's controller. A payload is at most 15 bytes and names no slot, so an
//! entry stays 24 bytes, which the store's file asserts, and converts to another layout
//! with no code of its own.
//!
//! ### Duration and expiry
//!
//! Two things end what is stored, each written once ([`store`]):
//!
//! - **A duration ends.** [`State::expire`] ends everything, in every store, that lasts for
//!   the [`store::Duration`] it is given. The rules call it as that moment comes, since the
//!   kernel moves the turn and holds none of its rules: with `UntilYourNextTurn` of the
//!   active player as a turn begins and `EndOfTurn` in the cleanup step. A
//!   player-controlling effect or extra turn is `Indefinite`: its turn takes it (see CR 724.1b).
//! - **An object ends.** "While the card stays in its zone" is no duration: whatever is
//!   stored about an object ends with it
//!   (see [CR 400.7]), inside the three
//!   chokepoints that bump an incarnation ([`State::move_object`],
//!   [`State::cease_to_exist`], a shuffle's rebind). `State` keeps the set of slots any
//!   store has an entry about, the members of the locked-in sets among them, so the move
//!   tests one bit and calls nothing else: it pays for the sets only while an object of
//!   one moves. This replaces three sweeps, all gone: a drop of stale effects, one of
//!   the turn's permissions and one of the cast value of X. A permission to
//!   cast a card "for as long as it remains exiled" is an entry of no stated duration on
//!   the card: it ends as the card leaves.
//!
//! Memos clear the same way, as their object ends. A card "exiled with" a permanent names that
//! object by its slot and incarnation byte ([`memos::Link`]): an ability of it still on the stack
//! finds the card after it has left (see CR 607.2a), and a new object in the slot does not (see
//! CR 400.7). A memo of a permanent put onto the battlefield with an Aura names the slot alone. As
//! an object arrives in a slot, the move ends those memos naming it and the links with its byte
//! (`State::forget_links_to`), behind one AND. A card exiled "until" a permanent leaves needs no
//! such end: the rules return it as it leaves (see CR 610.3), and the store's invariant holds
//! that permanent to the battlefield.
//!
//! Counters end the same way (see [CR 122.2]: "Counters on an object are not retained if that object
//! moves from one zone to another."): the move resets the object's fields and ends its rows of the
//! table with everything else stored about it. A row also ends with its last counter, so the table
//! holds no row of none.
//!
//! Trackers of this turn are cleared whole as the turn begins, inside
//! [`State::begin_turn`] with the lands played, and a permanent's bits as it leaves the
//! battlefield, in the arm of the move that already unlists it. So the Trackers store keys
//! no slot: among the keyed slots, every permanent that entered this turn would leave by
//! the stores' rare path. A tracker lasts for no [`store::Duration`] either: the turn it
//! is of ends where the kernel begins the next.
//!
//! ### Indexing and what an absent kind costs
//!
//! Each store keeps a mask of the kinds live in it, kept by the mutators that add and end
//! entries, so a reader of a kind with nothing live stops at one AND of a byte the state
//! already has in cache. That is exact where a [`GameTable`] bit could only bound it (a
//! Rust body is opaque to the table), so the kinds of effect and memo have no capability
//! bit. [`table::Capability`] holds what no live entry can say, found once in
//! [`GameTable::new`]: whether a card of the game reads a tracker, so [`State::track`], the
//! rules' write, is behind [`GameTable::can`]. Whether a permanent's static ability changes
//! a rule for players is a bit of the listener mask the layers fold from the battlefield
//! ("Static rule changes").
//!
//! | Hot read | With the kind absent | With it live |
//! | --- | --- | --- |
//! | the layers' loop over effects ([`State::layer_effects`]) | one AND of the store's kinds | one pass over the store's entries, at most 64 of 24 bytes, skipping other kinds; for an entry on a set, a pass over the battlefield testing each permanent's bit |
//! | the Effects store's replacements of a move, read by the rules' `replacement::moving` ([`State::move_replacements`]) | one AND | one pass comparing each entry's affected object |
//! | a proposal's match against the permanents' static replacement abilities (the rules' `replacement`: a move, an entering, a draw, damage) | one AND of the state's replacement mask ([`State::replacing`]) with the proposal's [`event::ReplacementKind`] | for each permanent whose face replaces that kind ([`query::Copiable::may_replace`]), its abilities, each asked [`State::has_ability`] |
//! | "does this object have this ability" ([`State::has_ability`]), "does this land make its card's mana" ([`State::makes_card_mana`], [`State::intrinsic_mana`]) | one test of a flag the layers store: no effect removes abilities or changes land types | the permanent's layer result, out of line |
//! | an object's last-known information ([`State::last_known`]) | not read: only an ability that looked back reads it | a pass over the table's rows, at most 16 |
//! | what a player may cast ([`State::cast_permissions`]) and what a spell costs more ([`State::cost_increase`]) | one AND each | one pass; a hand's cards need no entry |
//! | the zone move's reset | one bit test of the keyed slots | each store ends its entries about the slot, out of line; an arrival also tests the Memos kinds for a link to the slot |
//! | the turn's reset | Effects: the store's length read twice a turn. Trackers: thirteen words cleared, since the kernel begins the turn with no table to ask | Effects: one pass. Trackers: the same thirteen words, in place of two flags on every permanent |
//! | [`State::signature`] | one bit test of the keyed slots, and one of the two bits as two objects are compared (`store::Stored`) | out of line, the two objects' memos, their rows of counters and the effects on them compared: a permanent's layer results show what its effects make of it now, not for how long |
//! | "is a rule change of this kind in force" ([`State::rules_for`], [`GameTable::rules_of`]) | for a player, their two sets of rules joined and one AND with the table's set of the kind, which is the whole answer; for a permanent, one AND of the table's rules of its card and one of the store's kinds | for a permanent, a second AND of the rules of the face that is up, or one pass over the store's entries |
//! | an event's match against triggered abilities (the rules' `stack::trigger::match_event`) | one AND of the kinds any card of the game listens to ([`GameTable::listens_any`]) | one AND of the battlefield's listener mask; then, for each permanent whose card listens to the kind, the listeners of that kind on the face that is up ([`GameTable::listeners`]), and no other ability of it |
//! | the same match against abilities that work outside the battlefield ([`listener::AWAY`]) | one AND of the kinds they listen to ([`GameTable::listens_away`]) | the membership of the zones some card works in ([`GameTable::zones_away`]), each slot's card's zones read once ([`GameTable::works_away`]), then its listeners of the kind ([`GameTable::away_listeners`]): the state keeps no set of these objects, which the zone move would have to keep with no table to ask |
//! | a zone move's match against abilities that look back (the rules' `stack::trigger::match_look_back`, see CR 603.10a) | one AND of the kinds any card listens to with [`event::EventKind::Dies`] and [`event::EventKind::Leaves`]; the permanents leaving are read from the move's own list, so nothing else is written | for each permanent leaving, the battlefield's listeners of those kinds, then those that work away; a row of last-known information for each permanent some ability triggered on |
//! | forgetting last-known information as a player would receive priority ([`State::forget_last_known`]) | one test of the length of each of its two tables | out of line, a pass over the waiting triggers, delayed triggers and stack for each row |
//! | state triggers (the rules' `stack::trigger::match_states`, at each state-based check) | one AND of [`event::EventKind::State`] with the kinds any card listens to | as the match of an event, then for each such ability a pass over the waiting triggers and the stack for the same source and ability, so nothing is stored to say it triggered |
//! | the layers' search for the rules static abilities change for players | one AND of the replacement mask the pass has just folded from the permanents ([`event::Replaces::PLAYER_RULE_CHANGE`]): no read of its own | a second pass over the permanents, out of line, in a recompute with such a permanent on the battlefield |
//! | the characteristics of an object off the battlefield ([`State::colors`], [`State::types`] and the rest) | one test of the length of the list of effects that reach other zones ([`query::zone_characteristics`]) | out of line, the copiable values and then each effect in the list whose filter accepts the object |
//! | whether a card in a graveyard has escape ([`State::escape_from`]), which casting asks of the caster's graveyard at each priority | one test of the length of the same list, before the graveyard is read | out of line, for each card, each escape the list holds whose filter accepts the card |
//! | a mana ability activated only as an instant, which priority offers on its own ([`table::is_instant_mana`]) | one AND of the table's capabilities ([`table::Capability::InstantMana`]) | out of line, a pass over the player's permanents |
//! | the power and toughness of an object off the battlefield ([`State::power`], [`State::toughness`]) | one AND of the table's capabilities ([`table::Capability::DefinesPt`]) | out of line, the copiable values and then each characteristic-defining ability of the object's face, its amount evaluated ([`query::defined_power`]) |
//! | a permanent's protection, read by damage, targets, blocks and attachments ([`State::protection`]) | one AND of the state's replacement mask ([`event::Replaces::PROTECTION`]), which the layers set when an effect that gives protection applied | the permanent's layer result read, and the source's colors |
//! | a tracker or a count ([`State::track`], [`State::tracked`], [`State::count`], [`State::counted`]) | one AND of the table's capabilities where the rules would write it; a read finds an empty set or zero | a bit set or tested, or a byte written or read |
//! | a count of counters ([`State::counters_of`]) | of a kind the object keeps in a field, one read; of any other, one bit test of the keyed slots | one pass over the table's rows, at most 32 of 3 bytes |
//!
//! Measured for by-w8gv.7.2 on the rollout benchmark (404,858,188 instructions before):
//! the bit test in the zone move and the turn's two expiries cost about 0.03%; the store
//! in the field order the compiler chose counted 406,151,164, nearly all of it loops that
//! read fields whose offsets had moved; in [`State`]'s order now, 402,475,964.
//!
//! Measured for by-w8gv.7.3 (402,475,964 before, 398,897,988 after): the walk of the
//! battlefield for a sorcery-speed limit at each priority decision was about 7.0 million
//! instructions, 1.75%; the layers' pass that replaces it, run only with such a permanent
//! out, is 1.7 million. The benchmark is built with the engine's `coverage` feature, so a
//! hit recorded at each read costs it too: reporting the ability at every read of a
//! player's rule, by finding it on the battlefield again, counted 410,363,089.
//!
//! Measured for by-w8gv.7.6 (396,070,902 before, 389,343,295 after): the benchmark's decks
//! hold one card that makes a memo, a creature with evoke, so the kind is live in few of
//! its states. A permanent enters with one write fewer, of its flags, and the record the
//! rules carry for each object of a move is smaller by the flags' width; the zone move
//! and [`State::signature`] pay the bit test of the keyed slots they paid already.
//!
//! Measured for by-w8gv.7.8 (387,956,541 before, 389,728,983 after, 0.46% more): the
//! write as a permanent enters and the clearing as one leaves are about 0.03%, and
//! [`State::signature`]'s two bits about as much. The rest is the check of whether a
//! loyalty ability may be activated, which the benchmark's decisions make tens of
//! thousands of times: a tracker read there is a load and a bit test of the state where
//! the flag was a test of the object already in hand, and the loop about it compiled to
//! more besides. It reads one tracker at most decisions, asking the timing first; with the
//! store laid out at the far end of the state it counted 389,992,845, and with the check
//! out of line 391,310,485.
//!
//! The table of counters has no mask of kinds and no capability bit: a read is of one
//! object, and the keyed slots already say whether anything is stored about it.
//!
//! Measured for by-w8gv.7.9 (389,728,983 before, 389,975,802 after, 0.06% more): the
//! benchmark's decks hold one card with counters of a kind of the table, an artifact that
//! gains one as creatures enter and spends them to draw. Offering its ability reads the
//! count behind the keyed bit where it read a field of the object in hand, and
//! [`State::signature`] compares two objects' rows, nearly always none.
//!
//! The milestone in all, as slow CI counted each bead's tip: 404,858,188 before, then
//! 402,475,964 (by-w8gv.7.2), 398,897,988 (.3), 396,070,902 (.4), 389,343,295 (.6),
//! 387,776,236 (.7), 387,956,541 (.5), 389,728,983 (.8), 389,975,802 (.9 and .10): 3.7%
//! fewer, with the gate's bar lowered twice and never raised. The one rise of note is the
//! Trackers store's, above.
//!
//! Measured for by-w8gv.7.11 (389,975,802 before, 389,230,361 after): comparing the
//! effects on two objects as their memos and rows were compared, each store's view beside
//! the object, counted 390,514,859; one view of all three behind the one bit
//! (`store::Stored`), with the comparison out of line, is smaller to build for each
//! candidate than the two views it replaced.
//!
//! ### Static rule changes
//!
//! Whether a rule change applies to a player or a permanent now is asked the same way whether a
//! static ability or a resolved effect makes it (see [CR 613.11]: "Some continuous effects affect game
//! rules rather than objects."):
//!
//! - A rule change is the vocabulary's data (`beryl_card`'s `RuleChange`: who may or may
//!   not do what, and when), and any number of cards may say any number of them. A game
//!   numbers those its own cards name as its table is built ([`rule_changes`]): a static
//!   ability names the one it makes, and an ability whose effect starts one names that
//!   ([`GameTable::rule`], [`GameTable::rule_id`]). The table's rule changes of each card
//!   and face, the state's for each player and an effect's payload are those numbers, a
//!   set of them one word ([`rule_changes::Rules`]). The table refuses a rule change the
//!   rules have no check for ([`rule_changes::RuleKind::of`], [`rule_changes::Unchecked`]), and
//!   keeps the set of each kind they do check ([`GameTable::rules_of`]).
//! - The rules ask by kind: the rule changes in force for a player
//!   ([`State::rules_for`]) held against those of a kind is one AND, the whole answer
//!   while none of that kind is in force, and only when one is does the check read what
//!   it says (the spells its filter accepts). A change of a kind that is asked of the
//!   permanent with the ability is its face's ([`query::Copiable::rules`]), and its
//!   condition is kept with its number ([`GameTable::rule_condition`]).
//!   [`State::rule_applies`] answers for one numbered rule change, for tools and tests.
//! - A static ability's change for players is kept current with the layers, never found
//!   by walking the battlefield as the rules ask. A card with such an ability has
//!   [`event::Replaces::PLAYER_RULE_CHANGE`] in its replacement mask, which the layers'
//!   pass already reads of every permanent for what replaces. With the bit in what it folded, the pass
//!   adds each such permanent's rules to the set of each player they apply to, control
//!   being settled, and stores the sets as it finishes ([`derive::Rederive::finish`], with
//!   `replacing`); without it, it stores empty sets and has read nothing more. The sets
//!   are current while the layer results are, and a read while they are stale panics in
//!   debug builds, as a read of a layer result does. The contract first gave this a
//!   [`table::Capability`], which says what a game's cards could do; the mask says what
//!   the permanents on the battlefield do now, and costs a game with such a card nothing
//!   while none is out.
//! - A resolved effect's change for a player is an [`effects::Payload::Rule`] entry on
//!   that player. The Effects store keeps its own set of each player's rules, exact from
//!   the moment an entry starts or ends, so starting or ending one changes no layer input
//!   and the two sets never need each other. The contract first had one set for both
//!   sources (by-w8gv.7.2); with two, an effect that ends dirties nothing and each owner
//!   checks its own (the store's invariant, the engine's reference recompute).
//! - A rule change about the permanent with the ability asks its copiable values, which
//!   a face-down permanent has none of. The Effects store keeps entries on an object too
//!   ([`State::effect_rules_on`]), which no card starts: the table refuses an ability
//!   whose effect would.
//! - A change whose meaning depends on a value its permanent chose as it entered, as "sources
//!   with the chosen name" does ([`rule_changes::RuleKind::CantActivateNamed`]), is numbered
//!   once for the game like any other. Its check finds the permanents that make it only
//!   once it is in force for the player, and reads each one's [`State::chosen`]. Only a
//!   static ability may make one: the table refuses an effect that would start it.
//! - A change to what casting some spells costs ([`rule_changes::RuleKind::SpellCost`]: an
//!   increase, a reduction or a floor) applies to players but is asked of the permanents
//!   that make it, since its condition ("as long as this artifact is untapped") is the
//!   ability's and the layers read none. In a game whose cards make none, the total cost
//!   asks one test of the table's set; in one whose cards do, it walks the battlefield
//!   for the makers. Only a static ability may make one.
//! - "Can't" beats "can" (see [CR 101.2])
//!   in the rules that read two changes together, not here: each rule is its own bit.
//! - Which permanent makes a change, to tell a player why, is asked only once it is known
//!   to apply (the rules' `continuous::rule_change::maker`).
//!
//! ### What a store implements once
//!
//! `store::Store`, in the store's own file, with `Clone` and equality, which it derives,
//! `Hash` and [`relayout::Relayout`]. Nothing below grows with a new kind of entry. A new
//! kind is its own entry and its reader, and the compiler asks for no more (tried with a
//! scratch kind in each store, by-w8gv.7.11): of effect, a variant of [`effects::Continuous`]
//! and of [`effects::Payload`], and its bit of the store's kinds; of memo, a variant of [`memos::Memo`] and its arm in
//! each of the memo's own methods; of tracker, an entry of the list in [`trackers`] and
//! the [`table::Capability`] of its name, with how a card is found to read it; of counter,
//! an entry of `beryl_card::counter_kinds`.
//!
//! | Once | Where |
//! | --- | --- |
//! | the store of a new game | `Store::empty`, named once in [`State::empty`] |
//! | the copy | `Clone`, named once in [`Large::clone_state`]'s destructure, so the compiler finds a new store; `Small` copies the state whole |
//! | the hash | `Hash` of the entries and not of the store's indices (its kinds, the keyed slots), so a hash of a state is a hash of the game: [`State`]'s own `Hash` feeds its fields in a fixed order whatever order they are laid out in, which `beryl_bench`'s pin of its corpus holds to |
//! | another layout, and what does not fit one | [`relayout::Relayout`] and `Store::fits`, from [`State::fits`] |
//! | the invariants | `Store::check`, from [`State::check_invariants`]: every entry about an object names its current incarnation, the kinds mask agrees with the entries, a tracker is of a permanent and of none in a game no card of which reads it, a row of counters is of one kind once for an object, of a kind no object keeps in a field and never of none, and the keyed slots are exactly the stores' keys |
//! | the save description | `Store::lingering`, from [`State::off_board`], which a checkpoint records, the Effects store's with the words and the order saves hold for a permission to cast and a rule change. A memo of a permanent's spell is none of it: the one a later turn reads, a cost a static ability asks about, is saved with the permanent, as its counters are |
//! | what a player may see | one accessor on [`Observation`] in the store's file ([`Observation::effects`], [`Observation::memos`], [`Observation::tracked`], [`Observation::counters`]): an effect is public, a memo and a counter as public as their object, a tracker as public as the battlefield |
//! | an object ending, a duration ending | `Store::forget` and `Store::expire`, from the two moments above; the Trackers store's `forget` from the move's leaving of the battlefield, since it keys no slot |
//! | [`State::named`] | `Store::names`: the slots an entry names other than as the object it is about, as a card in exile remembers what exiled it. Three stores answer none; the Memos store answers behind its kinds, and its invariant holds a kind of memo that names a slot to the bit that says so |
//! | [`State::signature`] | compares what the stores hold about the object (`store::Stored`, which names each keyed store once): behind the keyed bit, its memos ([`memos::MemosOf`]), the counters it keeps in no field ([`counters::CounterRows`]) and the effects on it, alone or as a member of a set ([`effects::EffectsOn`]); for a permanent, what is tracked of it ([`trackers::Tracked`]). So no rule keeps a list of the objects an effect tells apart |
//!
//! ### The API
//!
//! A write is a [`State`] mutator, `#[inline]` and generic over the layout:
//! [`State::start_effect`], [`State::start_effect_on_set`], [`State::remember`],
//! [`State::track`], [`State::add_counters`], [`State::remove_counters`] and
//! [`State::expire`]. One whose change a player sees
//! returns its [`event::Event`] and pushes nothing; starting or ending an effect is no
//! event of its own, since the rules announce what it is part of, as they do a change of
//! control. The mutator that changes a layer input sets `derived_dirty` itself. A store
//! that is full returns [`fixed::Full`]. Reads are queries by kind
//! ([`State::layer_effects`], [`State::move_replacements`], [`State::rule_applies`],
//! [`State::cast_permissions`], [`State::cost_increase`], [`State::paid`],
//! [`State::x_of`], [`State::exiled_with`], [`State::exiled_until`], [`State::tracked`],
//! [`State::counters_of`]), with [`State::effects`], [`State::effects_on`],
//! [`State::memos`], [`State::trackers`] and [`State::counters`] for tools, tests and
//! views. No `&mut` to a store leaves the crate.
//!
//! ## Sets and orders
//!
//! Outside the kernel a [`Bitboard`] is a set: union, intersection, difference, `contains`,
//! `count`, `is_empty`, [`Bitboard::any`], [`Bitboard::all`], [`Bitboard::at_least`],
//! [`Bitboard::filter`], [`Bitboard::sum`], [`Bitboard::fold`], [`Bitboard::min`],
//! [`Bitboard::max`], [`Bitboard::any_pair`], [`Bitboard::sorted`] and [`Bitboard::sole`] give
//! order-independent answers. A fold uses an order-independent operation such as union.
//! The kernel owns iteration by slot. Ask for a game order by name:
//!
//! | Order | Query |
//! | --- | --- |
//! | the battlefield, as permanents entered it | [`State::battlefield`] |
//! | a library, top first | [`State::library_from_top`] for objects, [`State::library_cards_from_top`] for cards |
//! | the stack, from the top or bottom | [`State::stack`] |
//! | a graveyard, as cards were put there (see CR 404.2) | [`State::graveyard`] |
//! | players in turn order (see CR 101.4) | [`State::players_in_turn_order`] and the others under "Players" |
//! | a batch, in an order no seed decides: permanents as they entered, then library cards top first, then the rest by name | [`State::in_game_order`] |
//! | a hand, by card definition | [`State::hand_by_definition`] |
//! | the sources of one class of a mana payment, by definition, then as they entered the battlefield | [`State::mana_payment_sources`] |
//!
//! Per-object mutations use [`State::in_game_order`], which returns [`fixed::Full`] if the
//! batch exceeds the queue. [`SlotId::all`] enumerates storage and [`SlotId::pool`] finds free
//! slots for tokens, spell copies and cards brought into the game. Ascending slot order indexes
//! a recorded [`decision::Choice`], and [`decision::collapse()`] picks one of interchangeable objects.
//!
//! A stack entry's targets are named the same way: a [`state::TargetPlace`] is a target's
//! place in the entry's list, a [`state::TargetPlaces`] is a set of them with `insert`,
//! `contains` and `is_empty`, and [`state::placed`] gives the targets with their places in
//! the order they were chosen (see CR 601.2c).
//!
//! ## Queries
//!
//! Other crates read derived answers, not fields ([`query`]). A slot's definition index has
//! no getter outside the crate, and an object none for its base controller: each is asked
//! for by the rule that wants it. [`State::base_controller`] is the one named for what it
//! is, for the layers, which start from it, and a saved position, which keeps it.
//!
//! | The rules ask | Query |
//! | --- | --- |
//! | the card that represents an object: nothing for a token (see CR 111.1, 111.6) | [`State::card_behind`] |
//! | what it is before any other effect, and what a copy of it would be (see CR 707.2); for a merged permanent, its top card's face and every ability of its other cards (see CR 731.2a, [`Copiable::merged_faces`]), whose other cards are [`State::pile`] | [`State::copiable`], a [`Copiable`] |
//! | the definition and face it has the values of, for what the card table keeps per definition and no copy exception changes | [`State::definition`], a [`definition::Definition`]; its power and toughness before the other layers, [`State::copiable_pt`] |
//! | its mana value: a spell's with its X (see CR 202.3e), a transformed permanent's front face's and a copied back face's 0 (see CR 712.6d) | [`State::mana_value`] |
//! | who controls it: a permanent's layer result, a spell's caster, elsewhere the owner (see CR 108.4a) | [`State::controller`] |
//! | its power and toughness: the layer results on the battlefield, elsewhere the copiable values and then its characteristic-defining abilities, which work in every zone (see CR 604.3, [`query::defined_power`]) | [`State::power`], [`State::toughness`] |
//! | its colors, card types, supertypes and subtypes: the layer results on the battlefield, elsewhere the copiable values and then the continuous effects that reach other zones ([`query::zone_characteristics`]) | [`State::colors`], [`State::types`], [`State::supertypes`], [`State::has_subtype`] |
//! | its keywords: the layer results on the battlefield, the copiable values elsewhere | [`State::keywords`] |
//! | whether it has an ability of its face: a permanent may have lost it (see CR 613.1f); elsewhere it has them all | [`State::has_ability`], [`State::has_every_ability`] |
//! | whether a card off the battlefield has escape from an effect that reaches it (see CR 702.138a) | [`State::escape_from`] |
//! | the mana its basic land types make (see CR 305.6, 305.7) | [`State::intrinsic_mana`] |
//! | the colors a permanent has protection from | [`State::protection`] |
//! | what an object that left was as it last existed (see CR 603.10a) | [`State::last_known`] |
//! | what the color words and subtypes its text names read as after text changes (see CR 612.1); every reader of such a word ([`beryl_card::words`]) takes them | [`State::words_of`] |
//!
//! The kernel's filter evaluation and every reader of an object that may be off the
//! battlefield ask these. A reader that holds a permanent in a walk of the battlefield (the
//! state-based actions, combat, the mana solver) may take its layer result
//! ([`State::derived`]) instead, which is the same answer. An ability granted by another
//! permanent's effect is a
//! [`state::Derived`] `granted` entry that [`State::has_ability`] does not answer for, and
//! trigger matching reads no granted ability.
//! | the objects another record of the state names, which no twin stands in for (architecture §7.2) | [`State::named`] |
//! | every ability it has now: its face's own it has not lost, then those granted to it (see CR 613.1f) | [`State::abilities`], [`State::has`] |
//!
//! A [`Copiable`] is a definition, the face of it that is up and the exceptions of the copy
//! effects that made it ([`copiable::additions::Addition`], see CR 707.9b): two in `Small`, a third
//! [`fixed::Full`]. An addition names an exception by the card whose face lists it and its
//! place in that list ([`beryl_card::Face::excepts`]), so copying takes the exceptions with
//! the rest and appends the copier's own, and no reader walks a chain of copies. It answers with the face's characteristics ([`Copiable::face`], the blank
//! ones for a face-down permanent, see CR 708.2), the table's facts about that face, its name,
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
//! ## Format
//!
//! [`format::Format`] defines the seats and teams, starting life and hands, mulligans,
//! first draw, turn steps, land plays, hand limit, zone ownership, deck roles and losses.
//! [`GameTable::format`] supplies constant rules so rollouts incur no extra reads.
//!
//! ## Players
//!
//! Rules code uses the state's sets and orders. [`PlayerSet`] has no order of its own.
//!
//! | The rules ask | Query |
//! | --- | --- |
//! | every player (see CR 102.1) | [`State::players`] |
//! | a player's opponents (see CR 102.2, 102.3) | [`State::opponents_of`] |
//! | the defending players of this turn's combat (see CR 506.2, 802.2) | [`State::defending_players`] |
//! | the defending player an attacker attacks (see CR 508.5) | [`State::defending_player`] |
//! | the next player in turn order: who takes the next turn, who gets priority on a pass (see CR 103.1, 117.3d) | [`State::next_player`] |
//! | players in the order they act, from the active player (see CR 101.4) | [`State::players_in_turn_order`] |
//! | the same from a named player, as from the starting player (see CR 103.5) | [`State::players_in_turn_order_from`] |
//! | players by seat: a decision's candidates, the events of one simultaneous happening | [`PlayerSet::in_seat_order`] |
//! | who knows an object's identity | [`State::who_knows`] |
//! | every object in a zone, whoever owns it (see CR 400.1) | [`State::in_zone`] |
//!
//! Saves and `rollouts_never_change` pin seat order. Use it to list players, or document
//! why the order does not affect the result.
//! Each of these answers for the two-player game, and its rustdoc cites the rule that
//! changes the answer with more players: no multiplayer logic is written.
//! `Seat` has no `opponent`. The shortcut is the trait [`two_player::TwoPlayerSeat`], in
//! scope only where a module imports it: the kernel's turn order and test door, and callers
//! outside the rules that mean the other seat at this table (a view's "opponent", an
//! evaluation's other side, tests). Rules modules and card bodies never import it.
//!
//! ## Layouts
//!
//! How many players a game has, how many cards a deck, a library and a graveyard may hold,
//! how many objects occupy the shared pool at once, how many objects there are in all and
//! how many distinct cards a game contains are parameters of the [`Layout`], and nothing
//! outside the layouts assumes them (by-w8gv.5.14, by-w8gv.5.15):
//!
//! | parameter | where a layout sets it |
//! |---|---|
//! | players | [`SlotId::PLAYERS`] of its slots; [`Layout::PerPlayer`], the table with one entry each |
//! | deck, library and graveyard size | [`SlotId::DECK_SLOTS`]; [`Layout::Deck`], the queue that long |
//! | tokens, spell copies and cards brought into the game | the pool, [`SlotId::POOL`] to [`SlotId::COUNT`]: [`SlotId::pool`], [`Layout::TOKEN_POOL`] |
//! | objects in all | [`SlotId::COUNT`] |
//! | distinct cards, tokens and emblems among them | [`CardId::COUNT`] of [`Layout::Card`], the game index a state stores |
//!
//! [`Small`] is two players, 60-card decks, 255 slots and 255 distinct cards (a one-byte
//! [`CardIx`]), and stays within 20 KiB; [`Large`] is two players, 256-card decks, 1,024 slots
//! and 65,535 distinct cards (a two-byte [`LargeCardIx`]). Code generic over the layout asks
//! `L::Slot` and `L::Card` and builds `L::PerPlayer` and `L::Deck` values; a value that is the
//! same in every layout (a [`profile::Profile`]'s stops, a [`GameTable`]'s decks) is a
//! [`layout::PerSeat`], one entry for each seat `beryl_card` names. `Seat` stays the
//! two-variant enum it is, so `Seat::BOTH` still walks the players inside the kernel: a third
//! player needs a third seat first.
//!
//! The numbering of slots belongs to the slot type, not to the layout beside it, so a
//! conversion between two numberings needs only their types and a slot number of one cannot
//! pass for a number of the other. A slot keeps its [`ids::SlotPlace`] in every numbering (the
//! `i`th card of a player's deck, the `k`th slot of the pool) and its order among the
//! others, so [`relayout::Relayout`] takes a game to another layout place by place and a
//! `Choice` over objects names the same object in each. The UI's protocol names an object
//! by its number among [`LargeSlot`]s whatever layout its game is in ([`SlotId::num`]). A
//! layout with other deck sizes is a new slot type and a new `impl Layout`.
//!
//! A game index names the same card in every layout ([`CardId::to`]). State records
//! store `L::Card`. A record outside the state, such as a [`chance::Random`] reveal, uses
//! the widest index ([`CardId::wide`]). [`GameTable`] can return the caller's index type
//! because a state exists only when that type can index its whole table.
//!
//! [`State::deal`] returns [`Misfit`] if the layout cannot hold a deck or the table.
//! [`State::try_relayout`] returns it if the target cannot hold a deck, token, list or
//! table. [`GameTable`] has no layout-specific capacity checks.
//!
//! [CR 611.2c]: https://yawgatog.com/resources/magic-rules/#R6112c
//! [CR 400.7]: https://yawgatog.com/resources/magic-rules/#R4007
//! [CR 122.2]: https://yawgatog.com/resources/magic-rules/#R1222
//! [CR 613.11]: https://yawgatog.com/resources/magic-rules/#R61311
//! [CR 101.2]: https://yawgatog.com/resources/magic-rules/#R1012

// Name every enum case so new vocabulary cannot silently use an old fallback. Check
// numeric conversions so changing layouts cannot silently discard bits or change a sign.
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

pub mod abilities;
pub mod asking;
pub mod bitboard;
pub mod board;
pub mod cause;
pub mod chance;
pub mod chosen;
pub mod copiable;
pub mod counters;
pub mod decision;
pub mod definition;
pub mod derive;
pub mod effects;
pub mod event;
pub mod filter;
mod fit;
pub mod fixed;
pub mod format;
pub mod ids;
pub mod invariants;
mod knowledge;
pub mod last_known;
pub mod layout;
pub mod listener;
pub mod mana_memory;
pub mod memos;
pub mod mutate;
mod new;
mod object;
pub mod observation;
mod order;
pub mod payable;
mod pending;
pub mod phasing;
mod player;
mod player_set;
mod players;
pub mod profile;
pub mod query;
pub mod reference;
pub mod relayout;
pub mod repeats;
pub mod replacement;
pub mod rng;
pub mod rule_changes;
mod sideboard;
mod stack;
pub mod state;
pub mod static_ability;
pub mod store;
pub mod subgame;
pub mod subtypes;
pub mod table;
mod timestamps;
pub mod trackers;
mod turn;
pub mod two_player;
pub mod zone;
pub use beryl_card::TurnPhase;
pub use bitboard::Bitboard;
pub use fit::Misfit;
pub use ids::{CardId, CardIx, LargeCardIx, LargeSlot, Seat, Slot, SlotId, SlotPlace};
pub use layout::{Large, Layout, Small};
pub use observation::Observation;
pub use player_set::PlayerSet;
pub use query::Copiable;
pub use state::{Attachment, State};
pub use table::GameTable;
pub use turn::plan::{CombatPosition, Placement, StepPosition, StepTarget, TurnChange};
