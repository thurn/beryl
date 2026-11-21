//! Validated tuning data supplied by the host, and the constants awaiting conversion.

pub mod profiles;
mod tuning;
pub mod verbs;

pub use tuning::{Audit, Data};

include!(concat!(env!("OUT_DIR"), "/data.rs"));
