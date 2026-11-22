//! Opt-in records for explaining layout.

use game_hud::hud::placement_scene::Attempt;

/// Decisions collected while building the current screen.
#[derive(Default)]
pub struct Probe {
    pub attempts: Vec<Attempt>,
}

mod containers;
mod style;
pub use containers::{ContainerInfo, containers};
pub use style::{layout, styles};
