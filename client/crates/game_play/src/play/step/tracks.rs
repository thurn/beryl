//! Helpers of `Motion` that work on whole tracks and placements: adding a segment to a track,
//! finding where two placements differ, and numbering the abilities on a table's stack.

use beryl_protocol::protocol::StackItem;
use beryl_protocol::script::Table as Played;

use game_table::table::Placement;
use kit_base::pose::Segment;
use kit_space::motion::Track;

/// Adds `segment` to the track of the object in `slot`, growing `tracks` to hold it.
pub(crate) fn add(tracks: &mut Vec<Track>, slot: usize, segment: Segment) {
    if tracks.len() <= slot {
        tracks.resize_with(slot + 1, Track::default);
    }
    // No segment of the object begins after this one (`Motion::push` cuts them), so any that
    // ended before this one starts no longer decides a pose and can go to make room.
    if tracks[slot].push(segment, segment.start).is_err() {
        // Every segment it holds is still under way as this one starts, which only a script that
        // moves one object eight times at once can do. The track starts over from this segment,
        // which decides the object's pose from its start on.
        tracks[slot] = Track::default();
        let _ = tracks[slot].push(segment, segment.start);
    }
}

/// The first difference between the resting places `a` and `b` give, as a line naming the
/// slot (or the abilities) and both places; `None` when they give every slot and every ability
/// the same one.
pub(super) fn difference(a: &Placement, b: &Placement) -> Option<String> {
    if a.abilities != b.abilities {
        return Some(format!(
            "the abilities rest at {:?}, not {:?}",
            a.abilities, b.abilities
        ));
    }
    (0..a.rests.len().max(b.rests.len())).find_map(|slot| {
        let (a, b) = (
            a.rests.get(slot).copied().flatten(),
            b.rests.get(slot).copied().flatten(),
        );
        (a != b).then(|| format!("slot {slot} rests at {a:?}, not {b:?}"))
    })
}

/// The numbers of the abilities on `played`'s stack, bottom first.
pub(crate) fn numbers_of(played: &Played) -> Vec<u32> {
    played
        .stack
        .iter()
        .filter(|entry| matches!(entry.item, StackItem::Ability { .. }))
        .filter_map(|entry| entry.id)
        .collect()
}
