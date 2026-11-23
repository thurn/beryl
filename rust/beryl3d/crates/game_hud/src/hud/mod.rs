//! The heads-up display of the game screen: the players' life totals and details, the steps
//! of the turn, the notices `announce` steps show, the step banner and the game log.

pub mod anchors;
pub mod banner;
pub mod counts;
pub mod full_control;
mod input;
pub use input::HudIn;
pub mod inspector;
pub mod log;
pub mod notices;
pub mod orb_marks;
pub mod phase;
pub mod piles;
pub mod placement_scene;
pub use counts::players;
pub mod rows;
pub mod steps;
pub mod viewer;

/// A feature whose elements are placed as floats, outside the screen layout.
pub trait FloatFeature: kit_scene::Feature {
    fn floats() -> impl Iterator<Item = kit_ui::element::FloatId>;
}
