//! Beryl 3D's scene: poses, layout and animation as pure data, with no GPU, window, clock or
//! thread.

pub mod agent;
pub mod beat;
mod client;
pub mod command;
pub mod data;
#[cfg(feature = "driver")]
pub mod driver;
pub mod effects;
pub mod element;
pub mod faces;
pub mod frame;
pub mod hud;
pub mod input;
pub mod inspect;
pub mod layout;
pub mod light;
pub mod manual;
pub mod motion;
pub mod notice;
pub mod play;
pub mod pointer;
pub mod pose;
pub mod prompts;
pub mod screens;
pub mod settings;
pub mod sound;
pub mod table;
#[cfg(feature = "driver")]
pub mod testing;
pub mod text;
pub mod tokens;
pub mod verbs;
pub mod widgets;

pub use client::{Client, Config};
