//! The heads-up display of the game screen: the players' life totals and details, the steps
//! of the turn, the notices `announce` steps show, the step banner and the game log.

pub mod banner;
pub mod full_control;
mod input;
pub use input::HudIn;
pub mod log;
pub mod phase;
pub mod piles;
pub mod placement_scene;
pub mod players;
pub mod rows;
pub mod steps;

mod message;
pub use message::GamePress;

pub use game_play::hud::{counts, notices};
