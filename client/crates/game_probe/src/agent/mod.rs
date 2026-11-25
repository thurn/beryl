//! Interface checks and fault reports for game recordings.

mod cards;
mod report;
pub mod steps;

pub use cards::blank_cards;
pub use report::print_fault;

use kit_frame::frame::Frame;
use kit_probe::agent::{Context, Fault, Judged, Rule, checks};

/// Runs the generic frame checks and the checks that need a game frame.
#[must_use]
pub fn check(frame: &Frame, context: &Context) -> Vec<Fault> {
    let mut faults = checks::check(frame, context);
    faults.extend(blank_cards(frame, context));
    faults
}

/// Counts the elements the game-specific frame checks judge.
pub fn count_frame(tally: &mut Judged, frame: &Frame) {
    tally.add(
        Rule::BlankCard,
        cards::face_up(frame).count() + cards::piles(frame).count(),
    );
}
