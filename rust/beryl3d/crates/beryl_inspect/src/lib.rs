//! Beryl 3D's inspector: reads a scene and reports it as text, with no GPU.
//!
//! It also holds every recording the tests and tools read ([`recordings`]), packed into one
//! file per state of the sources they are recorded from ([`pack`]), and plays them through the
//! test driver by name ([`pages`]). [`cli`] is the text commands of `just b3d`.

pub mod cli;
mod corpus;
pub mod faults;
pub mod gate;
pub mod pack;
pub mod pages;
pub mod recordings;
pub mod runner;
pub mod scene;
pub mod screens;
mod stories;
pub mod stress;
