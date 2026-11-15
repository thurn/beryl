//! Beryl 3D's inspector: reads a scene and reports it as text, with no GPU.
//!
//! It also holds every recording the tests and tools read ([`recordings`]), packed into one
//! file per state of the sources they are recorded from ([`pack`]), and plays them through the
//! test driver by name ([`pages`]).

pub mod gate;
pub mod pack;
pub mod pages;
mod recorder;
pub mod recordings;
pub mod stress;
