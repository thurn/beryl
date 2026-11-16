//! A script compiled a few steps at a time, set beside the same script compiled whole, for the
//! tests that hold the two to the same poses (`compile::Compiling`).

use beryl_protocol::protocol::PlayerView;
use beryl_protocol::script::Table as Played;

pub use super::compile::STEPS_PER_ADVANCE;
use super::compile::{Compiling, compile};
use crate::motion::Track;
use crate::pose::Pose;
use crate::table::Table;

/// What compiling a script a few steps at a time did, frame by frame.
#[derive(Clone, Debug, Default)]
pub struct InParts {
    /// For each time, how many steps were compiled before the frame at that time.
    pub steps: Vec<usize>,
    /// For each time, every object's pose at that time by slot, as the steps compiled by then
    /// leave it.
    pub parts: Vec<Vec<Option<Pose>>>,
    /// For each time, every object's pose at that time by slot, from the script compiled whole.
    pub whole: Vec<Vec<Option<Pose>>>,
}

/// Compiles `after`'s script, played over `table` from `before`, whole and also a few steps
/// at a time, as frames at each of `times` (in nanoseconds on the script's clock, in order)
/// would, each compiling at least `least` steps (`Compiling::advance`), and gives every
/// object's pose at each time from both. An object on no track stands where it rests in
/// `before`.
#[must_use]
pub fn compile_in_parts(
    table: &Table,
    (before, after): (&PlayerView, &PlayerView),
    (times, least): (&[u64], usize),
) -> InParts {
    let played = Played::of(before);
    let timings = after
        .script
        .as_ref()
        .map_or(&[][..], |script| &script.timing);
    let rests = table.placement(before, &[]);
    let slots = (before.objects.iter().chain(&after.objects))
        .map(|object| usize::from(object.slot.0) + 1)
        .max()
        .unwrap_or(0);
    let poses = |tracks: &[Track], t: u64| -> Vec<Option<Pose>> {
        (0..slots)
            .map(|slot| match tracks.get(slot) {
                Some(track) if !track.segments().is_empty() => Some(track.pose_at(t)),
                _ => rests
                    .rests
                    .get(slot)
                    .copied()
                    .flatten()
                    .map(|rest| rest.pose),
            })
            .collect()
    };
    let (whole, _) = compile(Some((before, &played)), after, timings, table, false, false);
    let mut compiling =
        Compiling::new(Some((before, &played)), after, timings, table, false, false);
    let mut parts = InParts::default();
    for &t in times {
        parts.steps.push(compiling.advance(t, least));
        parts.parts.push(poses(compiling.tracks(), t));
        parts.whole.push(poses(&whole.tracks, t));
    }
    assert!(compiling.done(), "the times reach every step");
    parts
}
