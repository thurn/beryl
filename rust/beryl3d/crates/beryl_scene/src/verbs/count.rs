//! `count`, not built yet: an instant change. The subject is at its new place as the step begins.

use super::{StepIn, StepOut};

pub const BUILT: bool = false;

pub fn play(_step: &StepIn<'_>, out: &mut StepOut<'_>) {
    out.instant();
}
