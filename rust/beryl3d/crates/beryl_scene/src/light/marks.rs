//! Draws the marks that `Lit` holds: the outlines round cards and the marks on players and
//! abilities. Not built yet: it changes no card and writes nothing.

use beryl_protocol::script::Subject;

use super::Lit;
use crate::frame::FrameBuilder;
use crate::play::Scene;
use crate::table::Look;

/// Changes how the card `on` is drawn at `now` for the marks on it in `lit`.
pub fn look(_lit: &Lit, _on: Subject, _now: u64, _look: &mut Look) {}

/// Writes the records of the marks in `lit` as they are at `now`.
pub fn write(_lit: &Lit, _now: u64, _scene: &Scene<'_>, _builder: &mut FrameBuilder<'_>) {}
