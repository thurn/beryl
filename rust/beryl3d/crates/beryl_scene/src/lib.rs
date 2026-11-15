//! Beryl 3D's scene: poses, layout and animation as pure data, with no GPU, window, clock or
//! thread.

pub mod agent;
pub mod beat;
mod client;
pub mod command;
pub mod data;
#[cfg(feature = "driver")]
pub mod driver;
pub mod element;
pub mod faces;
pub mod frame;
pub mod input;
pub mod layout;
pub mod motion;
pub mod play;
pub mod pose;
pub mod table;
#[cfg(feature = "driver")]
pub mod testing;
pub mod text;
pub mod tokens;
pub mod verbs;

pub use client::{Client, Config};
