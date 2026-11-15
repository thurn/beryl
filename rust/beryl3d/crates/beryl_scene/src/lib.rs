//! Beryl 3D's scene: poses, layout and animation as pure data, with no GPU, window, clock or
//! thread.

mod client;
pub mod command;
pub mod data;
pub mod frame;
pub mod input;
pub mod pose;

pub use client::{Client, Config};
