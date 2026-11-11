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
//! | move an object between zones (CR 400.7), draw, order or shuffle a library, end a token | [`State::move_object`], [`State::draw`], [`State::reorder_library`], [`State::shuffle_library`], [`State::rebind_at_random`], [`State::cease_to_exist`] | the new incarnation and its reset fields; both zones' membership bitboards; the ordered library, graveyard and stack; the battlefield queue and every permanent's index into it; `derived_dirty`; the attachments, combat references and exile links that named it; what the stores held about it, which ends ("Stores"); its identity, settled before anyone sees it ([`chance`]); both players' knowledge |
//! | change a permanent's status, damage or counters | [`State::set_tapped`], [`State::set_summoning_sick`], [`State::reset_permanents_for_turn`], [`State::turn_face_up`], [`State::mark_damage`], [`State::clear_damage`], [`State::add_counters`], [`State::remove_counters`] | a face with its timestamp and who knows it; `derived_dirty` where the kernel can tell the layers read the field |
//! | put a spell on the stack, make a token, have a permanent enter | [`State::become_spell`], [`State::make_token`], [`State::arrive`] with a [`mutate::Arrival`], [`State::remember`] | what a permanent enters with (tapped, attacking, its face, controller, counters and what it is attached to) is set as one change, before anything reads it |
//! | declare and end combat | [`State::declare_attacker`], [`State::choose_attacked`], [`State::forget_attacked`], [`State::declare_blocker`], [`State::block`], [`State::mark_struck_first`], [`State::remove_from_combat`], [`State::end_combat`] | the combat flags and the combat reference change together; nothing stays related to a permanent removed from combat |
//! | show or hide a card | [`State::reveal`], [`State::look`], [`State::forget`], and for a paused resolution [`State::keep_seen`] | each player's knowledge; the identity is settled first |
//! | store the layers' results | [`State::rederive`], which lends the cache to the one function that fills it ([`derive::Rederive`]) | the derived table by battlefield index, the listening mask, what each face-down permanent's controller sees, `derived_dirty` cleared |
//! | change a player | [`State::gain_life`], [`State::lose_life`], [`State::damage_player`], [`State::add_mana`], [`State::spend_mana`], [`State::empty_mana_pools`], [`State::count_land_played`], [`State::declare_mulligan`], [`State::keep_hand`], [`State::count_bottomed`], [`State::count_mulligan`], [`State::add_emblem`] | a pool spends only mana it holds; a hand is kept with what it owes the bottom of the library; a mulligan taken is one to declare again |
//! | move the turn, count toward a budget, end the game | [`State::set_starting_player`], [`State::begin_turn`], [`State::begin_step`], [`State::give_priority`], [`State::pass_priority`], [`State::reset_passes`], [`State::declare_attack`], [`State::set_cleanup_check`], [`State::count_decision`], [`State::count_state_based_check`], [`State::reset_state_based_checks`], [`State::end_game`] | passes reset with priority; the lands played and the decisions offered begin again with the turn; that attackers were declared ends with the combat ([`State::end_combat`]) |
//! | use the stack | [`State::put_on_stack`], [`State::take_top_of_stack`], [`State::take_off_stack`], which returns the entry, [`State::announce_x`], [`State::announce_target`] | a spell's entry goes on once its card is in the stack zone and comes off before the card leaves, so the zone move keeps the stack's set of objects and these keep its order |
//! | queue triggers | [`State::queue_trigger`], [`State::take_trigger`], [`State::delay_trigger`], [`State::fire_delayed`] | a delayed trigger that fires waits with the others, once |
//! | open and settle a question | [`State::ask`], [`State::settle_question`], [`State::pause_resolution`], [`State::resume_resolution`], [`State::record_answer`] | a question is open or priority is; a paused resolution keeps what each player knew as it began, and only resuming it gives that back |
//! | start and end what a card creates | [`State::start_effect`], [`State::start_effect_on_set`], [`State::expire`] | an effect's timestamp; the kinds live in its store; the slots a store has entries about; a locked-in set's place in the store's table; `derived_dirty` where the entry is a layer input |
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
//! The checks deny warnings (clippy with `-D warnings`), so dropping an event does not pass
//! them: no visible change happens without one. A mutator takes as an argument what the event needs and the state cannot say,
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
//! [`State::is_current`] checks; where a record is filed is the mutator's to keep. One field
//! is not the rules' to build: what each player knew as a resolution began is the crate's
//! own in [`state::Resolving`], kept as it pauses and given back as it resumes.
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
//! tests go through it. A card body is lent the state shared, so it cannot open the door,
//! and rules code has no use for it: the `board-door` check in the `names` lane refuses
//! `.board()` and `board::Board` in `beryl_engine` and `beryl_cards` outside their tests.
//!
//! ## The legacy accessor
//!
//! [`State::legacy_mut`] lends a [`legacy::Legacy`], which writes, raw, what milestone 4
//! (by-w8gv.7.1) deletes and has not yet moved to a store ("Stores"), which therefore gets
//! no typed mutator: the [`state::ObjectFlags`] `LOYALTY_ACTIVATED`; and
//! [`state::Counters`]' long-tail kind (`other_kind`, `other`).
//! [`legacy::Legacy`]'s rustdoc lists each with the store that replaces it. Reads use the
//! getters. Milestone 4 deletes the accessor with the last of them (by-w8gv.7.10). The zone
//! move and the turn's resets clear those flags inside the kernel, with everything else an
//! object forgets.
//!
//! ## Stores
//!
//! A struct field holds what the Comprehensive Rules give a player, an object or the turn.
//! What a card creates lives in a store (plan K4): **Effects**, the continuous effects of
//! resolved spells and abilities; **Memos**, what an object remembers of how it was cast or
//! entered; **Trackers**, what happened this turn that a card of the game reads. This is the
//! design every bead of milestone 4 (epic by-w8gv.7) builds on, written as it will stand
//! when they land; a bead that builds a part amends its text. Built so far: the shared
//! core ([`store`]), the Effects store for objects with its layer and zone-change payloads
//! ([`effects`]) and the capability bits ([`table::Capability`]) (by-w8gv.7.2); a player as
//! the affected side, the rule-change payload and [`State::rule_applies`] (by-w8gv.7.3);
//! the permission to cast and the cost change as payloads, and flashback's zone-change
//! replacement (by-w8gv.7.4); the Memos store ([`memos`]) with what a permanent remembers
//! of its spell, the costs paid and the value of X, and [`State::signature`]'s comparison
//! of memos (by-w8gv.7.6); what exiled a card, as memos of the card (by-w8gv.7.7); a
//! locked-in set of objects as the affected side (by-w8gv.7.5).
//!
//! ### Where everything goes
//!
//! | What milestone 3 left | Where it goes | Bead |
//! | --- | --- | --- |
//! | the `effects` queue, `Continuous::Layer` | Effects: an entry on an object, payload [`effects::Payload::Layer`] of a [`effects::LayerChange`] | by-w8gv.7.2, built |
//! | `Continuous::ExileInsteadOfGraveyard` | Effects: an entry on an object, payload [`effects::Payload::Move`] | by-w8gv.7.2, built |
//! | `Player::sorceries_as_flash` | Effects: an entry on a player, payload [`effects::Payload::Rule`], lasting until that player's next turn | by-w8gv.7.3, built |
//! | the `Rule` mask (`rule_bit`, a `u8`) | `beryl_card`'s `Rules`, a set sized from the list of rules; a static ability's rule changes for players are found with the layers ("Static rule changes") | by-w8gv.7.3, built |
//! | the `permissions` queue, `CastPermission`; its `this_turn` | Effects: an entry on the card, payload [`effects::Payload::Cast`] of an [`effects::Permission`] naming the player and the terms the spell is cast on; `this_turn` is [`store::Duration::EndOfTurn`], and either way it ends with the card's object. [`State::cast_permissions`] reads them | by-w8gv.7.4, built |
//! | `CastPermission::extra`, `Casting::extra` | Effects: an [`effects::Payload::Cost`] on the spell, started as it is cast from the permission's terms. Before the cast the cost is read of the permission; from then on of the cost changes on the spell ([`State::cost_increase`]) | by-w8gv.7.4, built |
//! | `CastPermission::exile` and `flashback`, `StackEntry::exile` and `flashback` | the permission's terms carry them to the cast, which starts an [`effects::Payload::Move`] on the spell: exile instead of a graveyard, or [`effects::MoveReplacement::Flashback`], exile instead of anywhere else (CR 702.34a). Every move of the spell applies it where it applies any replacement (`beryl_engine`'s `event::exiled_instead`), and "was a flashback cost paid" is whether that replacement is on the spell | by-w8gv.7.4, built |
//! | `State::entered_x` | Memos: the permanent's `X` ([`memos::Memo::X`]) | by-w8gv.7.6, built |
//! | `ObjectFlags` `KICKED`, `OFFSPRING`, `IMPENDING`, `EVOKED` | Memos: the costs paid as the object was cast ([`memos::Memo::Paid`]), one entry holding the set of them, with a kind in the store's index for each kind of casting option of the vocabulary's list | by-w8gv.7.6, built |
//! | `EXILED_UNTIL`, `EXILED_WITH`, `Object::linked`, `State::link`, `zone::forget_exiled_with` | Memos: a memo of the exiled card naming the slot of what exiled it ([`memos::Memo::ExiledWith`], [`memos::Memo::ExiledUntil`]; "Duration and expiry" says how the first outlives that object), read by [`State::exiled_with`] and [`State::exiled_until`] | by-w8gv.7.7, built |
//! | `LOYALTY_ACTIVATED`, `ENTERED_THIS_TURN` | Trackers, keyed by permanent | by-w8gv.7.8 |
//! | [`state::Counters`]' long-tail pair (`other_kind`, `other`) | the counters table, keyed (object, kind), beside the three kinds every game reads, which stay in the object | by-w8gv.7.9 |
//! | `ObjectFlags::ONE_ONE` | stays a field. It is a copy's exception (CR 707.9b), part of the copiable values the rules give a token, and survives its zone changes as `TOKEN` does. That a flag holds exactly one exception is the copy effects' to widen (`docs/rules-coverage.md` §4), not a store's | none |
//!
//! A fact about a permanent that ends with the turn (a loyalty ability was activated, it
//! entered this turn) is a tracker keyed by object, not a memo with an expiry. A memo is
//! written once, as the object is cast or enters, and read by that object's own abilities;
//! a tracker is reset whole by the turn and exists only in a game with a card that reads
//! it. Kept as memos, each turn's reset would walk a keyed list and each read search it;
//! as a tracker the fact is one bit of a set of slots.
//!
//! ### Shapes, capacities and bytes
//!
//! | Store | In [`Small`] | Entry | Capacity | Bytes | In [`Large`] |
//! | --- | --- | --- | --- | --- | --- |
//! | Effects | a queue in the order the effects began, and a mask of the kinds live in it | 24 | 64 ([`effects::EFFECTS`]) | 1,552 | a `Vec` of 32-byte entries |
//! | the keyed slots, shared by every store | one set of slots | | | 32 | 128 |
//! | Effects' locked-in sets | a table of sets of slots, which an entry names by index; an empty set is a free place, and the table ends at its last set in use | 32 | 4 ([`effects::SETS`]) | 136 | a `Vec` |
//! | the rules changed for each player | two sets of rules for each: those of the permanents' static abilities, stored with the layer results, and those of the Effects store's entries, its index | 1 for each eight rules of the list | 2 players, twice | 4 | the same |
//! | Memos | a queue of (slot, memo) and a mask of the kinds live in it | 3, of the 4 allowed | 32 ([`memos::MEMOS`]) | 100, of the 136 allowed | a `Vec` of 4-byte entries |
//! | Trackers | a set of slots for each kind keyed by permanent | 32 | 2 kinds | 64 | 128 each |
//! | Counters | a queue of (slot, kind, count) | 3 | 32 | 104 | a `Vec` |
//!
//! A store that fills returns [`fixed::Full`] and the game stalls into `Large`
//! (`docs/rules-coverage.md`, principle 3). A locked-in set is 32 bytes, so the sets are a
//! table of their own and never part of an entry. Counter kinds cannot be fields of each of
//! 255 objects, so all but the three hot kinds are a keyed table.
//!
//! `size_of::<State<Small>>()` was 19,992 before the milestone and is 20,040 after
//! by-w8gv.7.2: 40 for the store's mask and the keyed slots, 8 of padding for the field
//! order ([`State`]'s rustdoc). It is 20,040 after by-w8gv.7.3 too: the four bytes of rule
//! sets are those `sorceries_as_flash` and its padding gave back. After by-w8gv.7.4 it is
//! 19,832: the `permissions` queue gave back 80 and each of the stack's 64 entries two
//! bytes, [`state::StackEntry`] being 58 bytes without its two flags. After by-w8gv.7.6 it
//! is 19,880: the Memos store's 100 bytes less the 50 of `entered_x`. After by-w8gv.7.5,
//! which landed after it, it is 20,016: the table of locked-in sets, 136.
//! The limit is 20,480. Each bead may change the size at its
//! own push by at most its allotment, whatever landed before it, and reports the size as it
//! reports the count:
//!
//! | Bead | Allotment | Why |
//! | --- | --- | --- |
//! | by-w8gv.7.3 | +16 (landed: 0) | the rule sets, a byte each while the list has at most eight rules, less `sorceries_as_flash` |
//! | by-w8gv.7.4 | -200 or lower (landed: -208) | the `permissions` queue (80) and two bytes of each of 64 stack entries go; permissions share the 64 effects |
//! | by-w8gv.7.5 | +136 (landed: +136) | the locked-in sets |
//! | by-w8gv.7.6 | +88 (landed: +48) | the Memos store less `entered_x` (50) |
//! | by-w8gv.7.7 | 0 (landed: 0) | its memos are entries of by-w8gv.7.6's store |
//! | by-w8gv.7.8 | +64 | two sets of slots |
//! | by-w8gv.7.9 | +104 | the counters table |
//! | by-w8gv.7.10 | 0 or lower | deletes only |
//!
//! With every bead but by-w8gv.7.4 landed the size is at most 20,448, so any order holds
//! the limit; the target with all landed is at most 20,256. The plan does not count on
//! [`state::Object`] shrinking: it stays 24 bytes while its flags are a `u32`, since
//! `linked` and the long-tail pair free three bytes and alignment gives none back. Once
//! by-w8gv.7.6 to by-w8gv.7.9 have landed the flags left fit a `u16` and the object 20
//! bytes, 1,020 fewer in all; by-w8gv.7.10 takes that if the count holds, as a bonus.
//!
//! ### Affected sides and payloads
//!
//! An effect affects one of four sides ([`effects::Affects`]): an object, named by slot and
//! incarnation, a player, and a locked-in set, by its index in the store's table (built);
//! the game, which joins with its first user, since no card yet changes a rule for
//! everyone. The fields are the variant's own, so every side fits four bytes.
//!
//! A locked-in set is the objects one effect affects when it affects several
//! ([CR 611.2c](https://yawgatog.com/resources/magic-rules/#R6112c), "the set of objects it
//! affects is determined when that continuous effect begins. After that point, the set
//! won't change."): [`State::start_effect_on_set`] starts one entry with one timestamp
//! (CR 613.7) for the permanents it is given, and the layers apply it to each member
//! ([`State::locked_in`]). A set holds slots, not incarnations: a member that ends leaves
//! its set where every stored thing about it ends ("Duration and expiry"), so whatever a
//! set holds is still the object it was, and a card that comes back is no member. The
//! effect ends with its duration or with its last member, and its place in the table is
//! free again. Only an effect on two objects or more takes a place: on one it is an entry
//! on that object, the same effect, and on none it never begins. A set is affected by a
//! layer change alone, as a player is by a rule change alone (the store's invariants):
//! nothing else the cards do is one effect on several objects, and each query by kind
//! would otherwise search the sets. An effect cannot be started on another's set.
//!
//! Its payload ([`effects::Payload`]) is a layer change, a zone-change replacement or a
//! rule change, a permission to cast or a cost change (built). A payload is a
//! resolved kernel type: every word of the card that needed the resolving ability to
//! settle it is settled, as [`store::Duration`] names the player "your next turn" means. A
//! rule change is the vocabulary's own `Rule`, which names no player: the affected side is
//! who it applies to. A player is affected by a rule change alone (the store's invariant). [`effects::LayerChange`] is the resolved form of `beryl_card`'s
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
//!   active player as a turn begins and `EndOfTurn` in the cleanup step.
//! - **An object ends.** "While the card stays in its zone" is no duration: whatever is
//!   stored about an object ends with it
//!   ([CR 400.7](https://yawgatog.com/resources/magic-rules/#R4007)), inside the three
//!   chokepoints that bump an incarnation ([`State::move_object`],
//!   [`State::cease_to_exist`], a shuffle's rebind). `State` keeps the set of slots any
//!   store has an entry about, the members of the locked-in sets among them, so the move
//!   tests one bit and calls nothing else: it pays for the sets only while an object of
//!   one moves. This replaces three sweeps, all gone: `State::drop_stale_effects`,
//!   `Legacy::end_permissions_of_turn` and the one in `Legacy::remember_x`. A permission to
//!   cast a card "for as long as it remains exiled" is an entry of no stated duration on
//!   the card: it ends as the card leaves.
//!
//! Memos clear the same way, as their object ends. A memo that names another object (the
//! cards "exiled with" a permanent) names its slot, not its incarnation: the cards stay
//! exiled with that object after it has left its zone, for an ability of it still on the
//! stack, until a new object arrives in the slot (CR 607.2a). So the move also ends the
//! memos naming the slot an object arrives in, on the battlefield or back in the pool
//! (`State::forget_exiled_with`), behind one AND of the Memos store's kinds. A card exiled
//! "until" a permanent leaves needs no such end: the rules return it as the permanent
//! leaves (CR 610.3), and the store's invariant holds that permanent to the battlefield.
//!
//! Trackers of this turn are cleared whole as the turn begins, inside
//! [`State::begin_turn`] with the lands played, and a permanent's bits as it leaves the
//! battlefield, in the arm of the move that already unlists it.
//!
//! ### Indexing and what an absent kind costs
//!
//! Each store keeps a mask of the kinds live in it, kept by the mutators that add and end
//! entries, so a reader of a kind with nothing live stops at one AND of a byte the state
//! already has in cache. That is exact where a [`GameTable`] bit could only bound it (a
//! Rust body is opaque to the table), so the kinds of effect and memo have no capability
//! bit. [`table::Capability`] holds what no live entry can say, found once in
//! [`GameTable::new`]: whether a card of the game reads a tracker, so the rules' write is
//! behind [`GameTable::can`]. Whether a permanent's static ability changes a rule for
//! players is a bit of the listener mask the layers fold from the battlefield
//! ("Static rule changes").
//!
//! | Hot read | With the kind absent | With it live |
//! | --- | --- | --- |
//! | the layers' loop over effects ([`State::layer_effects`]) | one AND of the store's kinds | one pass over the store's entries, at most 64 of 24 bytes, skipping other kinds; for an entry on a set, a pass over the battlefield testing each permanent's bit |
//! | `event::destination` ([`State::move_replacements`]) | one AND | one pass comparing each entry's affected object |
//! | what a player may cast ([`State::cast_permissions`]) and what a spell costs more ([`State::cost_increase`]) | one AND each | one pass; a hand's cards need no entry |
//! | the objects that are no other's twin for what an effect does to them ([`State::told_apart_by_effects`]) | one AND of the kinds that are no layer change | one pass |
//! | the zone move's reset | one bit test of the keyed slots | each store ends its entries about the slot, out of line; an arrival also tests the Memos kinds for a link to the slot |
//! | the turn's reset | Effects: the store's length read twice a turn. Trackers: nothing in a game no card of which reads them | Effects: one pass. Trackers: eight words cleared in place of two flags on every permanent |
//! | [`State::signature`] | one bit test of the keyed slots | the object's memos compared too; its layer changes already show in its layer results |
//! | "does this rule apply" ([`State::rule_applies`]) | for a player, their two sets of rules joined and one AND, which is the whole answer; for a permanent, one AND of the table's rules of its card and one of the store's kinds | for a permanent, a second AND of the rules of the face that is up, or one pass over the store's entries |
//! | the layers' search for the rules static abilities change for players | one AND of the listener mask the pass has just folded from the permanents ([`event::kind::RULE_CHANGE`]): no read of its own | a second pass over the permanents, out of line, in a recompute with such a permanent on the battlefield |
//! | a tracker (by-w8gv.7.8) | one AND of the table's capabilities where the rules would write it | a bit set or tested |
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
//! ### Static rule changes
//!
//! One question, [`State::rule_applies`], answers "does this rule change apply to this
//! player or permanent now", whether a static ability or a resolved effect makes it
//! ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311): "Some continuous
//! effects affect game rules rather than objects."):
//!
//! - `beryl_card`'s `Rule` is a list, and `Rules`, a set of them, has a bit for each in as
//!   many bytes as the list needs. The table's rules of each card, a face's facts and the
//!   state's rules for each player are that set, so a rule added to the list widens them
//!   all and no mask is widened by hand. Each rule says whose rule it changes, read for the
//!   controller of what makes the change (`Rule::whose`), or that it is about the permanent
//!   with the ability.
//! - A static ability's change for players is kept current with the layers, never found
//!   by walking the battlefield as the rules ask. A card with such an ability has
//!   [`event::kind::RULE_CHANGE`] in its listener mask, which the layers' pass already
//!   reads of every permanent for what listens. With the bit in what it folded, the pass
//!   adds each such permanent's rules to the set of each player they apply to, control
//!   being settled, and stores the sets as it finishes ([`derive::Rederive::finish`], with
//!   `listening`); without it, it stores empty sets and has read nothing more. The sets
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
//! - A rule about the permanent with the ability asks its copiable values, which a
//!   face-down permanent has none of, and the Effects store for entries on it.
//! - "Can't" beats "can" ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012))
//!   in the rules that read two changes together, not here: each rule is its own bit.
//! - Which permanent makes a change, to tell a player why, is asked only once it is known
//!   to apply (`beryl_engine`'s `rule_change::maker`).
//!
//! ### What a store implements once
//!
//! `store::Store`, in the store's own file, with `Clone` and equality, which it derives,
//! `Hash` and [`relayout::Relayout`]. Nothing below grows with a new kind of entry, which
//! is a variant of the store's payload:
//!
//! | Once | Where |
//! | --- | --- |
//! | the store of a new game | `Store::empty`, named once in [`State::empty`] |
//! | the copy | `Clone`, named once in [`Large::clone_state`]'s destructure, so the compiler finds a new store; `Small` copies the state whole |
//! | the hash | `Hash` of the entries and not of the store's indices (its kinds, the keyed slots), so a hash of a state is a hash of the game: [`State`]'s own `Hash` feeds its fields in a fixed order whatever order they are laid out in, which `beryl_bench`'s pin of its corpus holds to |
//! | another layout, and what does not fit one | [`relayout::Relayout`] and `Store::fits`, from [`State::fits`] |
//! | the invariants | `Store::check`, from [`State::check_invariants`]: every entry about an object names its current incarnation, the kinds mask agrees with the entries, and the keyed slots are exactly the stores' keys |
//! | the save description | `Store::lingering`, from [`State::off_board`], which a checkpoint records. A memo of a permanent's spell is none of it: the one a later turn reads, a cost a static ability asks about, is saved with the permanent |
//! | what a player may see | one accessor on [`Observation`] in the store's file ([`Observation::effects`], [`Observation::memos`]): an effect is public, a memo as public as its object |
//! | an object ending, a duration ending | `Store::forget` and `Store::expire`, from the two moments above |
//! | [`State::signature`] | compares what the stores hold about the object, behind the keyed bit, for what its layer results do not already show: its memos ([`memos::MemosOf`]) |
//!
//! ### The API
//!
//! A write is a [`State`] mutator, `#[inline]` and generic over the layout:
//! [`State::start_effect`], [`State::start_effect_on_set`], [`State::remember`] and
//! [`State::expire`] so far. One whose change a player sees
//! returns its [`event::Event`] and pushes nothing; starting or ending an effect is no
//! event of its own, since the rules announce what it is part of, as they do a change of
//! control. The mutator that changes a layer input sets `derived_dirty` itself. A store
//! that is full returns [`fixed::Full`]. Reads are queries by kind
//! ([`State::layer_effects`], [`State::move_replacements`], [`State::rule_applies`],
//! [`State::cast_permissions`], [`State::cost_increase`], [`State::paid`],
//! [`State::x_of`], [`State::exiled_with`], [`State::exiled_until`]), with
//! [`State::effects`] and [`State::memos`] for tools, tests and views. No `&mut` to a
//! store leaves the crate.
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
//! Other crates read derived answers, not fields ([`query`]). A slot's definition index has
//! no getter outside the crate, and an object none for its base controller: each is asked
//! for by the rule that wants it. [`State::base_controller`] is the one named for what it
//! is, for the layers, which start from it, and a saved position, which keeps it.
//!
//! | The rules ask | Query |
//! | --- | --- |
//! | the card that represents an object: nothing for a token (CR 111.1, 111.6) | [`State::card_behind`] |
//! | what it is before any other effect, and what a copy of it would be (CR 707.2) | [`State::copiable`], a [`Copiable`] |
//! | its mana value: a spell's with its X (CR 202.3e), a transformed permanent's front face's and a copied back face's 0 (CR 712.6d) | [`State::mana_value`] |
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
//! how many tokens and spell copies exist at once, how many objects there are in all and
//! how many distinct cards a game contains are parameters of the [`Layout`], and nothing
//! outside the layouts assumes them (by-w8gv.5.14, by-w8gv.5.15):
//!
//! | parameter | where a layout sets it |
//! |---|---|
//! | players | [`SlotId::PLAYERS`] of its slots; [`Layout::PerPlayer`], the table with one entry each |
//! | deck, library and graveyard size | [`SlotId::DECK_SLOTS`]; [`Layout::Deck`], the queue that long |
//! | tokens and spell copies | the pool, [`SlotId::POOL`] to [`SlotId::COUNT`]: [`SlotId::pool`], [`Layout::TOKEN_POOL`] |
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
//! pass for a number of the other. A slot keeps its [`ids::Place`] in every numbering (the
//! `i`th card of a player's deck, the `k`th slot of the pool) and its order among the
//! others, so [`relayout::Relayout`] takes a game to another layout place by place and a
//! `Choice` over objects names the same object in each. The UI's protocol names an object
//! by its number among [`LargeSlot`]s whatever layout its game is in ([`SlotId::num`]). A
//! layout with other deck sizes is a new slot type and a new `impl Layout`.
//!
//! A game index means the same card in every layout, and only its width differs
//! ([`CardId::to`]). Every record of a state that names a card stores `L::Card`, so those
//! records are generic over the layout ([`Copiable`], `Derived`, `Lki`, `GrantRef`). What is
//! kept outside any state names a card by the widest index ([`CardId::wide`]): a
//! [`chance::Random`] reveal. [`GameTable`] is not generic over the layout: it takes a game
//! index of any width and gives one of the width its caller stores, which is sound because
//! a state of a layout exists only for a table that layout's index numbers.
//!
//! A game that does not fit a layout is refused where a state of that layout is made, with
//! a [`Misfit`] and never a panic: [`State::deal`] refuses a deck with more cards than the
//! layout's `DECK_SLOTS` and a table of more distinct cards than its game index numbers, and
//! [`State::try_relayout`] a game with a deck, a token, a list or a table past what the
//! target holds. [`GameTable`] refuses no deck for its size and no decks for how many
//! distinct cards they name.

pub mod bitboard;
pub mod board;
pub mod chance;
pub mod decision;
pub mod derive;
pub mod effects;
pub mod event;
pub mod filter;
mod fit;
pub mod fixed;
pub mod ids;
pub mod invariants;
mod knowledge;
pub mod layout;
pub mod legacy;
pub mod memos;
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
pub mod store;
pub mod table;
mod turn;
pub mod two_player;
pub mod zone;

pub use bitboard::Bitboard;
pub use fit::Misfit;
pub use ids::{CardId, CardIx, LargeCardIx, LargeSlot, Place, Seat, Slot, SlotId};
pub use layout::{Large, Layout, Small};
pub use observation::Observation;
pub use player_set::PlayerSet;
pub use query::Copiable;
pub use state::State;
pub use table::GameTable;
