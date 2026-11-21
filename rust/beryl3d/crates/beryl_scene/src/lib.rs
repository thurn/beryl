//! Beryl 3D's scene: poses, layout and animation as pure data, with no GPU, window, clock or
//! thread.

pub mod agent;
mod client;
pub mod command;
pub mod driver;
pub mod screens;
pub mod settings;

pub use client::{BugPress, Client, Config, Dismiss, Input, Press};
pub use game_hud::{hud, inspect, navigation, prompts};
pub use game_manual::manual;
pub use game_play::{art, beat, light, notice, play, pointer, sound, verbs};
pub use game_probe::{probe, testing};
pub use game_table::{faces, table};
pub use kit_base::{atlas, data, geometry, paint, pose, tokens, typography};
pub use kit_frame::frame;
pub use kit_fx::effects;
pub use kit_space::motion;
pub use kit_text::text;
pub use kit_ui::{element, input, layout, widgets};
