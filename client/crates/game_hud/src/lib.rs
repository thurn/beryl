//! The game screen's features: prompts, life totals, the log, the inspector, the pile viewer, the opening hand, the result.

pub use game_art::art;
pub mod hud;
pub mod inspect;
pub mod labels;
pub mod mana_icons;
pub mod navigation;
pub mod prompts;
pub mod screens;

#[cfg(test)]
#[path = "../../../test_profiles.rs"]
mod test_profiles;
