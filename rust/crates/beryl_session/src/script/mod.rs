//! The script: what one seat's screen does between two views (plan K23 to K26,
//! `docs/project-50k.md`, Target architecture, Presentation). The session builds one beside each
//! view, per seat; the stage plays it and infers nothing. A script says what happens and in
//! what order. It holds no geometry and no timing: where things lie is the stage's layout, and
//! how long a verb takes is the design language's data (`data/ui-conventions.json`).
//!
//! # Shape
//!
//! A [`Script`] is beats in order; a beat is the [`Step`]s that happen together, in the order
//! a stagger takes them. A step is an [`Act`], one [`Verb`] applied to a card, an ability or a
//! player, and its cause. A beat ends when its last step's motion does.
//!
//! # Names
//!
//! A [`Subject`] names what a step is about, the same across steps and across scripts:
//!
//! - A card or token is its slot, the `slot` of its [`ObjectView`]. A card keeps its slot for
//!   the whole game, through every zone. Only a token's or a copy's slot is used again, and
//!   only after a `vanish` step with no record has ended it: from an `appear` to that `vanish`
//!   a slot is one thing, and the stage keeps one node for it.
//! - An ability on the stack is a number its seat's scripts give it as it goes on the stack,
//!   from 1 and never used again: a view's stack does not name its abilities.
//! - A player is a [`Side`].
//!
//! # The table and replay
//!
//! The *table* ([`Table`]) is the part of a [`PlayerView`] a script accounts for: `objects`,
//! `battlefield`, `stack`, each player's `life`, `mana`, `library` and `graveyard`, and the
//! turn's `number`, `active` and `step`. Its stack is the view's made the same for both seats
//! ([`Table::of`]): a spell or ability is on it until it has resolved, also while its
//! resolution asks the viewer something, and a spell only while its card is on the stack.
//! Replaying a script over the table of the view before gives the table of the view it came
//! with, exactly (K24), so a change no step shows is a failed check ([`failure`]), which a
//! debug build runs on every script it builds. The stage holds the same table and shows it
//! after every step, without the view that follows.
//!
//! Each step carries all of what it changes. A step with an object's record replaces that
//! slot's record; one that changes the object's zone also takes it out of its old zone's order
//! and puts it into the new one's at `at`, or at the end (the top of a pile, the last on the
//! battlefield) when `at` is none. An object leaving the stack takes its item with it, and one
//! arriving there brings its `item`. [`Act`] says the rest, verb by verb.
//!
//! Everything else in a view is prompt and chrome, replaced whole when the script ends:
//! `status` and its decision, `stops`, `pending_attackers`, `undone`, the turn's `priority`,
//! `starting` and `flip_winner`, and each player's `lands_played`, `mulligans`, `mulligan` and
//! `emblems`. How the game ended is chrome too: the result shows once the script that ended it
//! has played.
//!
//! # Spans
//!
//! An arrow, a line, a mark and a lift are never recomputed from a view. A step begins one and
//! a later step ends it ([`Span`]), in the same script or in another; between the two the
//! stage keeps it drawn. A view's `handling` is the lift the script begins and ends.
//!
//! A screen holds the spans its scripts have left open ([`Open`]) and its next script ends
//! them. The arrows and lines are the ones its table calls for (`spans.rs`): from each spell
//! and ability on the stack to its targets, from each ability to its source on the
//! battlefield, from each blocker to its attacker, and from each attacker to what it attacks
//! where a planeswalker could be attacked. One ends in a beat of its own before the beat
//! that moves either of its ends, and is not drawn again at a card that changed zones. A
//! script draws no mark: all six are a prompt's, which the session's prompt state will give
//! (plan K26).
//!
//! Spans change no table, so replay cannot hold them. The rule invariants do ([`carried`],
//! `check.rs`), which a debug build runs with the replay check on every script it builds.
//!
//! # Mana
//!
//! Mana is a `streak` from what made it into the spell or ability it pays for, and a `count`
//! of the pool for what stays there; what a pool has lost streaks from its player
//! (`mana.rs`).
//!
//! # Reset
//!
//! A seat's first script, and the script beside an undone view, has [`Script::reset`]: the
//! stage drops its table and every span and takes the view's table as it stands, with no
//! motion. The script's steps are then only the spans to show, begun at once. A stage with no
//! table yet does the same with whatever view it first gets (a page reloaded into a game in
//! progress), whose script was written for a screen that had the view before. A page that shows
//! first a view other than its seat's first has [`Follow`] write that view a reset of its own.
//!
//! # Building
//!
//! A [`Screen`] is what one seat last showed; the session keeps one per seat and asks it for
//! the script beside each view. The events become steps in order, played on the table as
//! they are written (`build.rs`, `moves.rs`, `mana.rs`), each with the spans it ends and
//! begins (`spans.rs`); what the view's table then has that no event told closes the script
//! (`settle.rs`). What an event came from, which events happen
//! together and which stack entry an event is about are guesses, all made in `infer.rs`
//! (see Cause).
//!
//! # Hidden information
//!
//! A script is built from its seat's views and events, and every record in it is that seat's
//! [`ObjectView`]: a card the seat has not seen has no name here either. What the seat may not
//! see has no step, rather than a masked one.
//!
//! # Reveals
//!
//! A card an effect reveals is a `showcase` step: it is held up to be read where it lies, and
//! the stage keeps its face in its "Revealed Cards" pile, which is chrome. The engine says
//! which cards are revealed ([`GameEvent::Reveal`]); a face-down permanent that leaves for a
//! hand or a library is revealed as it goes, which its move says (`moves.rs`). A revealed
//! card whose next move takes it to a zone that shows its face has no step: its flight
//! shows it.
//!
//! # Cause
//!
//! [`Step::cause`] names the spell, ability, permanent or player a step comes from; none is
//! the rules of the game (a turn-based action, a state-based action). The session infers it
//! from today's flat events, and will take it from the interpreter instead (plan K23), which
//! changes no consumer.
//!
//! # Sound
//!
//! A sound follows a step: its verb, with the zones and the reason of a `fly`, `appear` or
//! `vanish`, the direction of a `tap`, what a `count` counts and whether that rose from the
//! table's value, and whose doing it is, by its cause or its record's controller.

mod act;
mod build;
mod check;
mod follow;
mod infer;
mod mana;
mod moves;
mod replay;
mod settle;
mod spans;
mod table;
mod text;
mod verb;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub use act::{Act, Fact, Notice, Stream, Tally};
#[cfg(test)]
pub(crate) use build::PLANTED;
pub use build::Screen;
pub use check::{Open, carried};
pub use follow::Follow;
pub use replay::{failure, replay};
pub(crate) use spans::walkers;
pub use table::{Entry, Piles, Table, Turn};
pub(crate) use text::wire;
pub use verb::{Arrow, Mark, Span, Verb};

use crate::protocol::Side;
#[cfg(doc)]
use crate::protocol::{GameEvent, ObjectView, PlayerView};

/// What one seat's screen does between the view before and the view this came with.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Script {
    /// The stage starts over from the view's table, with no motion: a seat's first view, and
    /// an undone one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reset: Option<Reset>,
    /// In order; the steps of one beat happen together.
    pub beats: Vec<Vec<Step>>,
}

/// A fresh start from a view.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Reset {
    /// The numbers of the abilities on the view's stack, bottom first.
    pub abilities: Vec<u32>,
}

/// One verb applied to one thing, and what it came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(rename = "ScriptStep")]
pub struct Step {
    pub act: Act,
    /// The spell, ability, permanent or player responsible; none for the rules of the game.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub cause: Option<Subject>,
}

/// A card, an ability on the stack or a player: what a step is about, an end of an arrow, or
/// a cause.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Subject {
    /// A card or token, by slot. A spell on the stack is its card.
    Object {
        slot: u16,
    },
    /// An ability on the stack, by the number its `emerge` gave it.
    Ability {
        id: u32,
    },
    Player {
        player: Side,
    },
}
