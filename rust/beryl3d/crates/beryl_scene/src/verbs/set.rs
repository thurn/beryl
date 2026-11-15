//! `set`: a part of the table is now this, and nothing moves. The subject is at its new place
//! as the step begins.

use super::{StepIn, StepOut};

pub const BUILT: bool = true;

pub fn play(_step: &StepIn<'_>, out: &mut StepOut<'_>) {
    out.instant();
}
