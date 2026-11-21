//! Beryl 3D's scene: poses, layout and animation as pure data, with no GPU, window, clock or
//! thread.

pub mod agent;
pub mod art;
pub mod beat;
mod client;
pub mod command;
#[cfg(feature = "driver")]
pub mod driver;
pub mod effects;
pub mod element;
pub mod faces;
pub mod hud;
pub mod input;
pub mod inspect;
pub mod layout;
pub mod light;
pub mod manual;
pub mod motion;
pub mod navigation;
pub mod notice;
pub mod play;
pub mod pointer;
#[cfg(feature = "driver")]
pub mod probe;
pub mod prompts;
pub mod screens;
pub mod settings;
pub mod sound;
pub mod table;
#[cfg(feature = "driver")]
pub mod testing;
pub mod verbs;
pub mod widgets;

pub use client::{BugPress, Client, Config, Dismiss, Input, Press};
pub use kit_base::{atlas, data, geometry, paint, pose, tokens, typography};
pub use kit_frame::frame;
pub use kit_text::text;
