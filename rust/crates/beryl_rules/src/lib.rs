//! Beryl's rules: turns, priority, casting, mana, combat, layers, triggers and the interpreter
//! of effect data, over the kernel's state (`beryl_kernel`) and the card vocabulary
//! (`beryl_card`). `docs/engine-architecture.md` is the design; section numbers in these docs
//! refer to it.
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
//! [`Resolution`], in modules that mirror the Comprehensive Rules. This is the design every bead of milestone 5
//! (epic by-w8gv.9) builds on, written as it will stand when they land; a bead that builds a
//! part amends its text. What moves an object between zones, the keyword actions, drawing,
//! counters, damage, life, tokens, the choices, paying a cost, coins, continuous effects,
//! permissions to cast, delayed triggers, emblems, winning and adding mana are built.
//!
//! ## Game
//!
//! A [`Game`] is the state, the card tables and the event sink: the three every rule needs,
//! and nothing else. A library function that changes the game takes `&mut Game`, makes its
//! change through the kernel's mutators and sends the event through the pipeline
//! (architecture §8.2), so triggers, replacement effects, the layers and the sink see it.
//! Given an object that has changed zones since, it does nothing to it (CR 400.7). A function
//! that only reads takes the state and the table, as the kernel's queries do
//! ([`library::top`]).
//!
//! A library function is `pub`, generic over the layout, and `#[inline]` where it is a
//! mutator and its event. It names no card, and no player but those it is given.
//!
//! The engine's own code (turns, casting, combat, the event pipeline) still passes the three
//! apart. It is not rewritten for this: a bead that touches such a function converts it, and
//! until then a caller of the library makes the game where it calls, as `combat.rs` does to
//! deal combat damage.
//!
//! ## Resolution
//!
//! A [`Resolution`] is one spell or ability resolving: its source, controller, card and
//! ability, its targets and which of them were legal as it began (CR 608.2b), the costs paid
//! for it, X, what triggered it, and the record its questions are answered from. It also
//! carries what a resolution leaves for the engine: the question it paused at, and a card it
//! lets a player cast as it finishes (CR 608.2g).
//!
//! A library function takes a resolution beside the game only when what it does makes sense
//! only while something resolves: it reads the source or the controller, or it asks. Destroy
//! takes none; scry takes one.
//!
//! A choice made while resolving (CR 608.2d) is asked through [`choices`], which has the
//! functions for each shape of prompt and is the one module that makes a resolution's
//! question into a decision. A question the record cannot answer pauses the resolution,
//! which is run again from the top with the answers so far (§7.3). So a function asks the
//! same questions, in the same order, with the same options, every time it runs: a save and
//! a rollout replay the answers, and a change to any of the three breaks them. [`choices`]
//! says how an effect that needs several answers asks them.
//!
//! The kind of mana an [`Effect::AddMana`](beryl_card::Effect::AddMana) adds comes from, in
//! order: the choice of a mana ability's activator (CR 605.3b), which the resolution carries;
//! the one kind of a set that names one; and for a set of several outside a mana ability, a
//! question asked as the effect is applied (CR 608.2d). The first is
//! [`Effect::AddMana`](beryl_card::Effect::AddMana), a mana ability's alone, and the second
//! [`Effect::AddManaOfKind`](beryl_card::Effect::AddManaOfKind), a spell's or any other
//! ability's; the third comes with its question, when a card first needs it.
//!
//! ## How a resolution stops
//!
//! Two things stop a resolution before its end, and both travel as the `Err` of what was
//! running ([`card::Stop`]):
//!
//! - **A pause.** A question had no answer yet. The question stays on the resolution, not in
//!   the `Err`, which is one byte.
//! - **Full.** A fixed capacity ran out. The state is abandoned for its wider copy (§12.3),
//!   so nothing undoes, and nothing plays on.
//!
//! A function of the game alone returns `Result<T, Full>`, or its value plain when nothing it
//! does can run out of room ([`actions::shuffle`], [`actions::reveal`]). One that may ask
//! returns `Result<T, Stop>`, or [`card::Flow`] when it has nothing to return, and `?` carries
//! a `Full` into it. No function notes either on a flag, puts a default in its place or goes
//! on: a push is followed by `?`, or is a `push_bounded` whose bound is argued (the
//! `discarded-push` check). The engine turns `Full` into the stall in one place, where the
//! resolution ends (`interpret::finish`).
//!
//! ## Modules
//!
//! A library module is named for the rule it implements, so a rule number or the words on a
//! card find the file.
//!
//! | Rules | Module | Built, and to come |
//! | --- | --- | --- |
//! | CR 701, keyword actions | `actions/`, a file for each, named as the rules name it | [`actions::counter`], [`actions::destroy`], [`actions::discard`], [`actions::exile`], [`actions::explore`], [`actions::fateseal`], [`actions::manifest`], [`actions::manifest_dread`], [`actions::mill`], [`actions::regenerate`], [`actions::reveal`], [`actions::sacrifice`], [`actions::scry`], [`actions::search`], [`actions::shuffle`], [`actions::tap`] (tap and untap), [`actions::transform`] |
//! | CR 702, keyword abilities | `keywords/`, a file for each | [`keywords::flashback`]; kicker |
//! | any other rule | a module named for the rule's title | [`ending`] the game (CR 104), [`mana`], which the engine's payment solver shares (CR 106), [`tokens`] (CR 111), [`emblems`] (CR 114), [`costs`] (CR 118), [`life`] (CR 119), [`damage`] (CR 120), [`drawing`] (CR 121), [`counters`] (CR 122), [`zones`] (CR 400), [`library`] (CR 401), [`stack`] (CR 405), [`casting`] spells as an effect allows (CR 601), [`delayed_triggers`] (CR 603.7), [`linked`] abilities (CR 607), [`choices`] made while resolving (CR 608.2d), [`one_shot`] effects (CR 610), [`continuous`] effects (CR 611), [`coins`] (CR 705); copies (CR 707) |
//!
//! A module holds, in this order:
//!
//! 1. Its first line: the rule's title and number, with the link.
//! 2. The functions, each citing its rule with a quotation. An action's function has the
//!    action's name (`actions::scry::scry`); a module named for a thing holds what is done
//!    with it (`life::gain`, `counters::remove`, `tokens::create`).
//! 3. The interpreter's arm for each effect-data entry that calls them: a `pub(crate)`
//!    function named `instruction`, or `<function>_instruction` where the module has more
//!    than one. It takes the interpreter's `Run` and the entry's operands as the vocabulary
//!    writes them (`Objects`, `Players`, `Amount`), reads them against the scene, calls the
//!    function, and returns the outcome the instructions after it read. `Run::instruction`
//!    (`interpret/run.rs`) is the table of entries, one line for each, naming its arm; an
//!    entry whose module is not written still has its arm there. Two entries that differ
//!    only in where they put a card share one arm, which each one's own names
//!    ([`zones`]'s for [`library`]'s "put into a library").
//! 4. The entry's English template, a constant directly above its arm. Milestone 8
//!    writes them; none exists yet.
//!
//! A file stays under 800 lines.
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

pub mod actions;
mod announced;
mod bodies;
pub mod card;
mod cast;
pub mod casting;
pub mod choices;
pub mod coins;
mod combat;
pub mod continuous;
pub mod costs;
pub mod counters;
pub mod coverage;
pub mod damage;
pub mod delayed_triggers;
pub mod drawing;
pub mod emblems;
pub mod ending;
mod engine;
pub mod event;
mod face_down;
pub mod game;
mod interpret;
pub mod keywords;
mod layers;
pub mod library;
pub mod life;
pub mod linked;
pub mod mana;
pub mod one_shot;
mod reference;
pub mod resolution;
mod resolve;
mod rule_change;
mod sba;
mod scratch;
pub mod stack;
mod start;
mod stops;
pub mod tokens;
mod trigger;
mod turn;
pub mod zones;

// The game state and everything that writes it are the kernel's (`beryl_kernel`).
// The rules name its modules as their own and export none of them whole: a caller reads the
// kernel from the kernel, or from the facade. What the rules do export of it stands beside
// items of their own, in `layout`, `profile` and `event`.
pub(crate) use beryl_kernel::{
    PlayerSet, bitboard, chance, decision, filter, fixed, ids, invariants, observation, query,
    replacement, state, static_ability, table, zone,
};

/// The kernel's storage layouts.
pub mod layout {
    pub use beryl_kernel::layout::*;
}

/// The kernel's restriction profiles, and where they stop a player in a game.
pub mod profile {
    pub use crate::stops::{NextStop, can_declare_blockers, next_step_stop, next_stop};
    pub(crate) use crate::stops::{asks_lone, stops_for};
    pub use beryl_kernel::profile::*;
}

pub use announced::target_takes;
pub use bodies::{CustomBody, custom_body};
pub use cast::{cost_being_cast, payable_responses, pointless};
pub use combat::preferred_damage;
pub use drawing::wins_instead_of_drawing;
pub use engine::{
    Counts, Scratch, apply, apply_decided, concede, interchangeable, next, next_in_step,
    next_within, observe, paused_events, refusal, view,
};
pub use game::Game;
pub use keywords::flashback::paid as flashback_paid;
pub use mana::mana_contribution;
pub use reference::check_references;
pub use resolution::Resolution;

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
