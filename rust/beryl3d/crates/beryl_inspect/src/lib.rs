//! Beryl 3D's inspector: reads a scene and reports it as text, with no GPU.
//!
//! It also holds every recording the tests and tools read ([`recordings`]), packed into one
//! file per state of the sources they are recorded from ([`pack`]).

pub mod gate;
pub mod pack;
mod recorder;
pub mod recordings;
pub mod stress;
