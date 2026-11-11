//! Prompts as the session holds and sends them (plan K10 and K26, `docs/project-50k.md`,
//! Target architecture, Prompts and Presentation).
//!
//! The engine asks one pick at a time, and each pick's decision says which prompt it belongs
//! to: a [`Purpose`](beryl_engine::card::Purpose), its [`Shape`](beryl_engine::card::Shape)
//! and the shape's parameters. The session folds a run of picks into one prompt and holds
//! that prompt's state. The UI draws what it is sent and sends back clicks; it decides
//! nothing, so the desktop and the phone behave alike and a prompt's behavior is a Rust test.
//!
//! # State
//!
//! A [`PromptView`] is a prompt as its seat sees it. It says what the decision's pick belongs
//! to: the shape, the purpose, the part of the shape the pick answers and the record's
//! parameters, by public slot and relative to the viewer. And it is the prompt's whole state:
//!
//! - its title and its buttons, each with its label and whether it can be clicked, worded by
//!   one table keyed by purpose;
//! - the [`Mark`](crate::script::Mark) on each card, ability and player, from the closed list
//!   scripts draw on, at most one each;
//! - the shape's own state ([`ShapeView`]): the cards picked, the piles and their order, an
//!   amount, the payment so far.
//!
//! The view that opens a decision carries the prompt's first state. It rides the view, so a
//! reloaded page takes up a prompt as it stood.
//!
//! The session holds every shape's state. A prompt it cannot hold (a payment whose plans
//! tap the same permanents) arrives with no marks, no buttons and no state, and the UI words
//! its own buttons from the prompt's [`Words`] and answers by candidate.
//!
//! # Words
//!
//! Every title and label comes from one table keyed by purpose (`wording.rs`), filled from
//! the record and the view. A new purpose is one entry of the purposes' list and one of the
//! table; nothing on the wire names it but its string.
//!
//! # Clicks
//!
//! A click is a command ([`ClickRequest`]) naming what was clicked: a card, an ability or a
//! player; a button; a card dropped into a pile at a place; a number. Its reply
//! ([`ClickReply`]) is the prompt's whole new state, or says the click answered the prompt.
//! A click changes nothing but the prompt, so nothing else is sent until it answers.
//!
//! A click that answers sends the engine the run's picks, one as each decision of the run
//! arrives, through the same path as any answer: the choice log records the picks and never
//! the clicks. The first pick is the player's answer and the rest are its continuations, so
//! an undo takes back the whole prompt and a replay needs no prompt state.
//!
//! The state is the game's (`held.rs`): made as the view of a decision is drawn, changed by
//! each click, and gone with the decision, whether it is answered, taken back or undone. It is
//! a function of the open decision and the clicks since, so it is never saved, and a game
//! resumed or replayed to a decision opens its prompt afresh. A view drawn again, as a change
//! of stops draws it, and the view a UI takes a game up from show the prompt as the clicks
//! left it. A click names the state it was made on and is refused on any other; one on a
//! prompt whose state the session does not hold is refused too.
//!
//! One module holds each shape's state (`pick.rs`, `order.rs`, `split.rs`, `yes_no.rs`,
//! `number.rs`, `mode.rs`, `catalog.rs`, `divide.rs`, `pay.rs`, `declare.rs`): what it opens
//! as, what each click does to it, the marks and buttons it shows, and the picks it answers
//! with. What a prompt opens with that only the engine can say is found by playing answers
//! ahead on a copy of the game (`ahead.rs`); a payment asks the game again after each click,
//! so a click is made in the game its prompt is open in.
//!
//! # Text
//!
//! A [`PromptView`] prints as a few lines of text (`text.rs`), for the hand-written
//! expectations of tests (plan K24).

mod ahead;
mod catalog;
mod declare;
mod divide;
mod held;
mod mode;
mod number;
mod open;
mod order;
mod pay;
mod pick;
mod split;
mod text;
mod view;
mod wording;
mod yes_no;

pub(crate) use ahead::forced;
pub use held::{Clicked, Held, Run, add_opening_state};
pub(crate) use open::{halt_prompt, prompt_view};
pub use order::trigger_to_stack;
pub use view::{
    ButtonId, Click, ClickReply, ClickRequest, Lies, Marked, Pair, PartName, Pile, PlaceName,
    PromptButton, PromptView, ShapeName, ShapeView, Share, Words,
};
