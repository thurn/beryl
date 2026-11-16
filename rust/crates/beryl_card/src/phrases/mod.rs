//! Reusable compositions of the card vocabulary, indexed by their Oracle wording.
//!
//! A phrase only combines existing vocabulary. Escalate a clause that needs a new
//! variant or rules implementation. Start each phrase's doc comment with its quoted
//! wording and name each parameter it renders in braces. Add a rendering example to
//! `render/phrase_tests.rs` so the test checks the wording and catches duplicates.

mod costs;
mod effects;
mod filters;
mod targets;
mod triggers;

pub use costs::*;
pub use effects::*;
pub use filters::*;
pub use targets::*;
pub use triggers::*;
