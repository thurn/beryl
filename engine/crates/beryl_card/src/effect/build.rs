//! Builders: the words a card file writes an [`Effect`] in.
//!
//! Each is a `const fn`, so a definition stays a `static`. A builder takes a
//! printed number; an instruction whose amount is worked out is written as its [`Effect`]
//! variant, with the [`Amount`] it reads:
//!
//! ```
//! use beryl_card::effect::build::{destroy, sequence, target, you};
//! use beryl_card::{Amount, Effect};
//!
//! // "Destroy target enchantment. You gain life equal to its mana value."
//! const EFFECT: Effect = sequence(&[
//!     destroy(target()),
//!     Effect::GainLife {
//!         player: you(),
//!         amount: Amount::ManaValue(target()),
//!     },
//! ]);
//! ```

mod casting;
mod control;
mod counters;
mod damage;
mod faces;
mod filters;
mod flow;
mod references;
mod tokens;
mod turn;
mod zones;

pub use casting::*;
pub use control::*;
pub use counters::*;
pub use damage::*;
pub use faces::*;
pub use filters::*;
pub use flow::*;
pub use references::*;
pub use tokens::*;
pub use turn::*;
pub use zones::*;
