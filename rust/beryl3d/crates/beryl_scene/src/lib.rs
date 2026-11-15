//! Beryl 3D's scene: poses, layout and animation as pure data, with no GPU, window, clock or
//! thread.

pub mod agent;
mod client;
pub mod command;
pub mod data;
#[cfg(feature = "driver")]
pub mod driver;
pub mod frame;
pub mod input;
pub mod pose;
pub mod table;
#[cfg(feature = "driver")]
pub mod testing;

pub use client::{Client, Config};
