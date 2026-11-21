//! Validated tuning data supplied by the host, and the constants awaiting conversion.

pub mod effect;
pub mod lighting;
pub mod playback;
pub mod pointer;
pub mod presentation;
pub mod profiles;
mod tuning;
pub mod verbs;

pub use tuning::{Audit, Data};

include!(concat!(env!("OUT_DIR"), "/data.rs"));
