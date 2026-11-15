//! Building the script: what one seat's screen does between two views
//! ([`beryl_protocol::script`] describes the script itself). A [`Screen`] is what one seat
//! last showed; the session keeps one per seat and asks it for the script beside each view.

mod attack;
mod build;
mod check;
mod follow;
mod hear;
mod mana;
mod moves;
mod settle;
mod spans;
mod stack;
mod staging;

pub use beryl_protocol::script::*;
#[cfg(test)]
pub(crate) use build::PLANTED;
pub use build::Screen;
pub use check::{Open, carried};
pub use follow::Follow;
pub use hear::{Cause, Copied, Heard, Move, Names, Said};
