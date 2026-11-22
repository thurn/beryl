//! Beryl's rules: turns, priority, casting, mana, combat, layers, triggers and the interpreter
//! of effect data, over the kernel's state (`beryl_kernel`) and the card vocabulary
//! (`beryl_card`). `docs/engine-architecture.md` is the design; section numbers in these docs
//! refer to it.
//!
//! Implements: none. Documents the rules library contract.
//!
//! The rules are a state machine that never calls the AI: [`next`] advances a
//! [`State`](state::State) until a player must decide or the game ends, and [`apply`] consumes
//! one [`Choice`](decision::Choice). They are generic over the state's storage
//! [`Layout`](layout::Layout); in [`Small`](layout::Small), the one search runs on, the state
//! is one flat `Copy` value. The cards live in shared read-only tables
//! ([`CardTable`](table::CardTable), [`GameTable`](table::GameTable)). Players see the game
//! only through an [`Observation`](observation::Observation).
//!
//! Other crates import the rules through `beryl_engine`, the facade that re-exports this
//! crate and the kernel under the paths they had as one crate, until the last caller has
//! moved.
//!
//! Rules code cites the Comprehensive Rules at <https://yawgatog.com/resources/magic-rules/>.
//!
//! # The contract
//!
//! The rules library is what an effect does to a game: free functions over a [`Game`] and a
//! [`Resolution`], in modules that mirror the Comprehensive Rules. This text describes the
//! library as it is built; a change that makes part of it untrue amends it in the same
//! commit. What moves an object between zones, the keyword actions, drawing,
//! counters, damage, life, tokens, the choices, paying a cost, coins, continuous effects,
//! permissions to cast, delayed triggers, emblems, winning and adding mana are built.
//!
//! ## Game
//!
//! A [`Game`] is the state, the card tables and the event sink: the three every rule needs,
//! and nothing else. A library function that changes the game takes `&mut Game`, makes its
//! change through the kernel's mutators and sends the event through the pipeline
//! (architecture §8.2), so triggers, replacement effects, the layers and the sink see it.
//! Given an object that has changed zones since, it does nothing to it (see CR 400.7). A function
//! that only reads takes the state and the table, as the kernel's queries do
//! ([`zones::library::top`]).
//!
//! A library function is `pub` and `#[inline]` where it is a mutator and its event. It
//! names no card, and no player but those it is given.
//!
//! The engine's own code (turns, casting, combat, the event pipeline) still passes the three
//! apart. It is not rewritten for this: a bead that touches such a function converts it, and
//! until then a caller of the library makes the game where it calls, as
//! `combat/assignment.rs` does to deal combat damage.
//!
//! ## Layout
//!
//! Keep rule operations as free functions in the module that owns the rule. The module path
//! gives shared verbs their meaning, as with `zones::tokens::create` and
//! `zones::emblems::create`. A layout-dependent signature declares `L: Layout`. Engine callers
//! use the same generic [`next`] and [`apply`] with either layout.
//!
//! The destruction prototype moved two operations onto `Game` and added three source lines,
//! with the same six signature lines. Concrete engine entry points halved the generated LLVM
//! IR in `beryl_search`. They raised the `beryl_rules` test-profile library build from
//! 3.2 s to 42.7 s and increased the rollout instruction count by 0.68%.
//!
//! ## Resolution
//!
//! A [`Resolution`] is one spell or ability resolving: its source, controller, card and
//! ability, its targets and which of them were legal as it began (see CR 608.2b), the costs paid
//! for it, X, what triggered it, and the record its questions are answered from. It carries
//! the question it paused at, including casting steps when an effect allows a spell to be
//! cast during resolution (see CR 608.2g).
//!
//! A library function takes a resolution beside the game only when what it does makes sense
//! only while something resolves: it reads the source or the controller, or it asks. Destroy
//! takes none; scry takes one.
//!
//! A choice made while resolving (see CR 608.2d) is asked through [`interpret::choices`],
//! which has the functions for each shape of prompt. A cast during a resolution uses the
//! casting machine to build its decisions and reads from the same answer record.
//! A question the record cannot answer pauses the resolution,
//! which is run again from the top with the answers so far (§7.3). So a function asks the
//! same questions, in the same order, with the same options, every time it runs: a save and
//! a rollout replay the answers, and a change to any of the three breaks them.
//! [`interpret::choices`] says how an effect that needs several answers asks them.
//!
//! The kind of mana an [`Effect::AddMana`](beryl_card::Effect::AddMana) adds comes from, in
//! order: the choice of a mana ability's activator (see CR 605.3b), which the resolution carries;
//! the one kind of a set that names one; and for a set of several outside a mana ability, a
//! question asked as the effect is applied (see CR 608.2d). The first is
//! [`Effect::AddMana`](beryl_card::Effect::AddMana), a mana ability's alone, and the second
//! [`Effect::AddManaOfKind`](beryl_card::Effect::AddManaOfKind), a spell's or any other
//! ability's; the third comes with its question, when a card first needs it.
//!
//! ## How a resolution stops
//!
//! A resolution can stop before its normal end. The reason travels as the `Err` of
//! what was running ([`card::Stop`]):
//!
//! - **A pause.** A question had no answer yet. The question stays on the resolution, not in
//!   the `Err`, which is one byte.
//! - **Full.** A fixed capacity ran out. The state is abandoned for its wider copy (§12.3),
//!   so nothing undoes, and nothing plays on.
//! - **End the turn.** The effect exiles the stack and enters cleanup (see CR 725.1).
//!   The transaction commits without carrying out any remaining instructions or moving the
//!   resolving spell to the graveyard.
//!
//! A function of the game alone returns `Result<T, Full>`, or its value plain when nothing it
//! does can run out of room ([`actions::shuffle`], [`actions::reveal`]). One that may ask
//! returns `Result<T, Stop>`, or [`card::Flow`] when it has nothing to return, and `?` carries
//! a `Full` into it. No function notes either on a flag, puts a default in its place or goes
//! on: a push is followed by `?`, or is a `push_bounded` whose bound is argued (the
//! `discarded-push` check). The engine turns `Full` into the stall in one place, where the
//! resolution ends (`interpret::finish`).
//!
//! A replacement effect's choice (`replacement`) can come up in code that is no resolution's and
//! returns `Result<T, Full>`, as the draw step's draw does. It stops the run as a `Full`
//! too, keeping its question in the sink (`beryl_kernel::asking`); the code that opened the
//! run, a resolution or the end of a step, tells the stop from a capacity running out by
//! that question. Only a run on a copy of the state can stop so, and it discards the copy.
//!
//! ## Modules
//!
//! A library module is named for the rule it implements, so a rule number or the words on a
//! card find the file.
//!
//! - [`game`] starts and ends games and changes life totals (see CR 103, see CR 104, see CR 119).
//! - [`turn`] advances phases and steps and handles their turn-based actions (see CR 500,
//!   see CR 502, see CR 504, see CR 514, see CR 703).
//! - [`casting`] handles land play, casting spells, activating abilities, selecting targets
//!   and paying costs (see CR 115, see CR 116, see CR 118, see CR 601, see CR 602).
//! - [`mana`] handles pools, mana abilities and the payment solver (see CR 106, see CR 605).
//! - [`stack`] orders the stack, puts triggered abilities on it and resolves its top entry
//!   (see CR 405, see CR 603, see CR 608).
//! - [`continuous`] applies continuous effects, text changes and layers (see CR 611,
//!   see CR 612, see CR 613).
//! - [`replacement`] applies replacement and prevention effects, including protection
//!   (see CR 614, see CR 615, see CR 616, see CR 702.16).
//! - [`combat`] handles combat steps and damage (see CR 506, see CR 508, see CR 509,
//!   see CR 510, see CR 511, see CR 120).
//! - [`zones`] moves objects between zones and handles libraries, drawing, tokens, emblems
//!   and counters (see CR 400, see CR 401, see CR 111, see CR 114, see CR 121, see CR 122).
//! - [`objects`] handles copies, linked abilities, one-shot effects, phasing, face-down
//!   permanents and mutate (see CR 607, see CR 610, see CR 707, see CR 702.26, see CR 702.140).
//! - [`actions`] implements keyword actions and coin flips (see CR 701, see CR 705).
//! - [`keywords`] implements flashback (see CR 702.34).
//! - `engine.rs` and `engine/` build decisions, advance the game and apply answers
//!   (see CR 117). `sba.rs` checks and performs state-based actions (see CR 704).
//!   `saga.rs` matches chapter abilities, advances lore and finds completed Sagas (see CR 715).
//! - `interpret/` reads effect data and carries out its instructions, including choices
//!   made during resolution (see CR 608.2c, see CR 608.2d, see CR 608.2f, see CR 608.2h).
//!
//! A module holds, in this order:
//!
//! 1. Its header names the rules and gives an `Implements:` declaration for the rules it owns.
//! 2. The functions, each citing its rule with a quotation. An action's function has the
//!    action's name (`actions::scry::scry`); a module named for a thing holds what is done
//!    with it (`game::life::gain`, `zones::counters::remove`, `zones::tokens::create`).
//! 3. The interpreter's arm for each effect-data entry that calls them: a `pub(crate)`
//!    function named `instruction`, or `<function>_instruction` where the module has more
//!    than one. It takes the interpreter's `Run` and the entry's operands as the vocabulary
//!    writes them (`Objects`, `Players`, `Amount`), reads them against the scene, calls the
//!    function, and returns the outcome the instructions after it read. `Run::instruction`
//!    (`interpret/run.rs`) is the table of entries, one line for each, naming its arm; an
//!    entry whose module is not written still has its arm there. Two entries that differ
//!    only in where they put a card share one arm, which each one's own names
//!    ([`zones`]'s for [`zones::library`]'s "put into a library").
//! 4. The entry's English template, a constant directly above its arm. Milestone 8
//!    writes them; none exists yet.
//!
//! A file stays under 800 lines.
//!
//! ## The engine's own rules
//!
//! `continuous::layers` (see CR 613) recomputes every permanent's characteristics one layer at a
//! time. Its submodules handle dependencies (see CR 613.8), static abilities granted in layer 6,
//! and a permanent's prospective characteristics as it enters (see CR 614.12).
//! `replacement` proposes moves, damage, draws and destructions, then applies the replacement
//! and prevention effects in the required order (see CR 616.1). `replacement::protection` checks protection
//! (see CR 702.16) for targeting, attachment and blocking. `stack::trigger` (see CR 603) matches events,
//! look-back triggers (see CR 603.10a) and state triggers (see CR 603.8) against the card table's
//! listeners. `sba` handles state-based actions (see CR 704). A library function that changes
//! the game sends its event through `replacement::event::settle`, which refreshes the layers,
//! matches triggers and records the event.
//!
//! Each of these is reached through a mask: the kinds of event the game's cards listen to or
//! replace ([`GameTable::listens_any`](table::GameTable::listens_any),
//! [`GameTable::replaces_any`](table::GameTable::replaces_any)), then those of the
//! permanents now (the state's `listening` and `replacing`), then those of one face. A game
//! with no card of a kind pays one AND for it.
//!
//! An object's ability is read through [`State::has_ability`](state::State::has_ability):
//! a reader walks the face's abilities and asks it of each. An ability granted to a permanent
//! by another's effect is read from the permanent's layer result instead, and only by
//! activation (`casting/legal.rs`, `casting/timing.rs`) and the layers.
//!
//! ## Scenarios
//!
//! A library function's hand-written scenarios are not in its module: a scenario
//! names real cards, since the oracles play it, and rules code names none. They live in
//! `beryl_cards`, in a tree that mirrors this one (`library/actions/scry.rs` for
//! [`actions::scry`]), beside the cards' own. A scenario there uses basic lands, the
//! designated prop cards and one card that has the effect. The bead that ports an entry
//! writes its scenario and lists it in `beryl_cards::library::SCENARIOS` under that card,
//! the card crate's tests and the oracles play the tree as they play a card's, and its
//! tiering test (`tests/vocabulary.rs`) holds that a hand-written scenario carries out every
//! entry a card says.
//!
//! ## Custom bodies
//!
//! What effect data cannot say stays a Rust function in the card's file. It is a
//! [`CustomBody`]: a unit type of the card file's own, whose `resolve` is generic over the
//! layout and takes the game and the resolution. The ability's `body` is [`custom_body`] of
//! that type: a `const fn` makes the instance each layout runs, so the card writes no macro
//! and names no layout. The body calls the library and the resolution's questions and reads
//! the state; it has no other way to the game.
//!
//! Nothing can read code, so the type also says what effect data says of itself
//! ([`CustomBody::SAYS`]): whether the body may ask a player a question, and how it takes
//! each of its targets from its controller. Everything else the engine, the AI and the UI
//! know of the ability comes from its data: its cost, its targets, its trigger.
//!
//! No card has one today: every card of the pool is effect data, and a test body keeps the
//! hatch compiled and run.
//!
//! An intervening "if" is condition data. One that data cannot say is written the same way
//! when the first is needed: a trait whose method reads the state, and a `const fn` beside
//! [`custom_body`].

// New enum variants must be classified by each reader, and values must fit the receiving
// representation in either state layout. Cargo requires the rest of the workspace lint
// settings to be inherited as a unit, so the rules add these lints here.
#![deny(
    clippy::wildcard_enum_match_arm,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::cast_precision_loss
)]

pub mod actions;
mod bodies;
pub mod card;
pub mod casting;
pub mod combat;
pub mod continuous;
pub mod coverage;
mod engine;
pub mod game;
mod interpret;
pub mod keywords;
pub mod mana;
pub mod objects;
mod reference;
pub mod replacement;
pub mod resolution;
mod saga;
mod sba;
mod scratch;
pub mod stack;
mod turn;
pub mod zones;

// The game state and everything that writes it are the kernel's (`beryl_kernel`).
// The rules name its modules as their own and export none of them whole: a caller reads the
// kernel from the kernel, or from the facade. What the rules do export of it stands beside
// items of their own, in `layout`, `profile` and `replacement::event`.
pub(crate) use beryl_kernel::copiable::additions;
pub(crate) use beryl_kernel::{
    PlayerSet, bitboard, chance, decision, definition, filter, fixed, ids, invariants, observation,
    query, state, static_ability, table, zone,
};

/// The kernel's storage layouts.
pub mod layout {
    pub use beryl_kernel::layout::*;
}

/// The kernel's restriction profiles, and where they stop a player in a game.
pub mod profile {
    pub use crate::turn::stops::{NextStop, can_declare_blockers, next_step_stop, next_stop};
    pub(crate) use crate::turn::stops::{asks_lone, stops_for};
    pub use beryl_kernel::profile::*;
}

pub use bodies::{CustomBody, custom_body};
pub use casting::{cost_being_cast, payable_responses, pointless, target_takes};
pub use combat::preferred_damage;
pub use continuous::layers::rederive;
pub use engine::{
    AdvanceUntil, Counts, Scratch, apply, apply_decided, concede, interchangeable, next,
    next_in_step, next_within, observe, paused_events, refusal, view,
};
pub use game::Game;
pub use interpret::choices;
pub use keywords::flashback::paid as flashback_paid;
pub use mana::mana_contribution;
pub use reference::check_references;
pub use resolution::Resolution;
pub use zones::drawing::wins_instead_of_drawing;

// The tests set games up as a caller of the facade does, with the kernel's names at the root.
#[cfg(test)]
pub(crate) use beryl_kernel::{
    bitboard::Bitboard,
    decision::{
        Answers, Candidates, Choice, Decision, DecisionKind, Opt, Outcome, Replay, StallReason,
        Step,
    },
    ids::{Seat, Slot},
    layout::{Large, Layout},
    observation::Observation,
    profile::{Payment, Profile},
    rng,
    state::{State, TurnStep},
    table::{CardTable, Decklist, GameTable},
};

#[cfg(test)]
mod tests;
