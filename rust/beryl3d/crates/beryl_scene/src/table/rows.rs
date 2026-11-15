//! One battlefield row at rest (docs/beryl-3d.md, "Objects and placement"). A row lays its
//! cards out at full size while they fit its width, then overlaps them down to the layout's
//! least uncovered strip, then stacks identical permanents into piles, and when even that is too
//! long, shows as many as fit from a scroll offset and leaves the rest undrawn.

use std::f32::consts::FRAC_PI_2;

use beryl_session::protocol::{Facing, ObjectView};
use glam::{Quat, Vec3};

use super::{CARD_HEIGHT, CARD_WIDTH, LAYER, Layout, RowArea};
use crate::pose::Pose;

/// One card of a laid-out row: its pose, and whether it is drawn.
pub(super) struct Laid<'a> {
    pub object: &'a ObjectView,
    pub pose: Pose,
    pub visible: bool,
}

/// How a row is laid out: the least strip in millimetres, the layout's spacing, and the row's
/// scroll offset in groups.
pub(super) struct RowRules<'a> {
    pub layout: &'a Layout,
    pub strip: f32,
    pub scroll: usize,
}

/// Lays out `cards`, in order from left to right, in `area` at height `y` on the table.
pub(super) fn lay_row<'a>(
    cards: &[&'a ObjectView],
    area: RowArea,
    y: f32,
    rules: &RowRules,
) -> Vec<Laid<'a>> {
    let room = area.right - area.left;
    let alone: Vec<Vec<&ObjectView>> = cards.iter().map(|&card| vec![card]).collect();
    let groups = if fits_overlapped(&alone, room, rules) {
        alone
    } else {
        piles(cards)
    };
    let widths: Vec<f32> = groups.iter().map(|g| group_width(g, rules)).collect();
    let full: f32 = widths.iter().sum::<f32>() + rules.layout.gap * gaps(groups.len());
    let mut laid = Vec::with_capacity(cards.len());
    if full <= room {
        let mut left = area.left + (room - full) / 2.0;
        let mut height = 0;
        for (group, width) in groups.iter().zip(&widths) {
            place_group(group, left, y, &mut height, true, rules, &mut laid);
            left += width + rules.layout.gap;
        }
        return laid;
    }
    // Every window of `shown` consecutive groups fits at the least strip, however wide its
    // last group is.
    let widest = widths.iter().copied().fold(0.0, f32::max);
    let shown = ((((room - widest) / rules.strip).floor() as usize) + 1).clamp(1, groups.len());
    let first = rules.scroll.min(groups.len() - shown);
    let last_width = widths[first + shown - 1];
    let step = if shown > 1 {
        (room - last_width) / gaps(shown)
    } else {
        0.0
    };
    let mut height = 0;
    for (index, group) in groups.iter().enumerate() {
        let visible = (first..first + shown).contains(&index);
        // A group scrolled out of view rests at the nearer end of the row, undrawn.
        let place = index.clamp(first, first + shown - 1) - first;
        let left = area.left + step * place as f32;
        place_group(group, left, y, &mut height, visible, rules, &mut laid);
    }
    laid
}

/// Whether every card, alone, fits the row with at least the least strip of each uncovered.
fn fits_overlapped(groups: &[Vec<&ObjectView>], room: f32, rules: &RowRules) -> bool {
    let Some(last) = groups.last() else {
        return true;
    };
    gaps(groups.len()) * rules.strip + group_width(last, rules) <= room
}

/// The number of spaces between `count` things in a line.
fn gaps(count: usize) -> f32 {
    count.saturating_sub(1) as f32
}

/// Puts a group's cards at `left`, the copies beneath peeking out to the right and away from
/// the viewer. Every card lies one `LAYER` above the card placed before it, whether or not the
/// row overlaps: a tapped card is wider than an untapped row's step, and two cards at one height
/// would flicker where they overlap.
fn place_group<'a>(
    group: &[&'a ObjectView],
    left: f32,
    y: f32,
    height: &mut u32,
    visible: bool,
    rules: &RowRules,
    laid: &mut Vec<Laid<'a>>,
) {
    // The top card is the group's first; the copies beneath are placed first, lowest.
    for (depth, &object) in group.iter().enumerate().rev() {
        let offset = rules.layout.pile_step * depth as f32;
        let centre = left + card_width(object) / 2.0 + offset.x;
        let rotation = if object.tapped {
            Quat::from_rotation_z(-FRAC_PI_2)
        } else {
            Quat::IDENTITY
        };
        let pose = Pose {
            position: Vec3::new(centre, y + offset.y, LAYER * *height as f32),
            rotation,
            turn: if face_up(object) { 0.0 } else { 1.0 },
            ..Pose::default()
        };
        *height += 1;
        laid.push(Laid {
            object,
            pose,
            visible,
        });
    }
}

/// How wide a card lies: a tapped card is turned a quarter turn.
fn card_width(object: &ObjectView) -> f32 {
    if object.tapped {
        CARD_HEIGHT
    } else {
        CARD_WIDTH
    }
}

/// How wide a group lies: its top card and the copies peeking out beside it.
fn group_width(group: &[&ObjectView], rules: &RowRules) -> f32 {
    card_width(group[0]) + rules.layout.pile_step.x * gaps(group.len())
}

/// Whether the viewer sees a card's face: a face-up card whose name they know.
pub(super) fn face_up(object: &ObjectView) -> bool {
    object.facing == Facing::Up && object.name.is_some()
}

/// `cards` with identical permanents gathered into piles, each pile where its first card was.
/// The pile's first card is the one with the lowest slot, which lies on top: the engine offers
/// the lowest slot of interchangeable objects, so what a choice takes leaves from the top.
fn piles<'a>(cards: &[&'a ObjectView]) -> Vec<Vec<&'a ObjectView>> {
    let mut groups: Vec<Vec<&ObjectView>> = Vec::new();
    for &card in cards {
        match groups
            .iter_mut()
            .find(|group| plain(card) && plain(group[0]) && twins(group[0], card))
        {
            Some(group) => group.push(card),
            None => groups.push(vec![card]),
        }
    }
    for group in &mut groups {
        group.sort_by_key(|object| object.slot.0);
    }
    groups
}

/// Whether a permanent may share a pile: nothing on it or about it that a pile would hide.
fn plain(object: &ObjectView) -> bool {
    face_up(object)
        && object.note.is_none()
        && !object.doesnt_untap
        && object.named_counters.is_empty()
        && object.counters.is_empty()
        && object.damage == 0
        && object.combat.is_none()
        && object.attached_to.is_none()
}

/// Whether two permanents are alike in everything the table shows of them.
fn twins(a: &ObjectView, b: &ObjectView) -> bool {
    a.slot != b.slot
        && a.name.is_some()
        && a.name == b.name
        && a.facing == b.facing
        && a.owner == b.owner
        && a.controller == b.controller
        && a.token == b.token
        && a.tapped == b.tapped
        && a.summoning_sick == b.summoning_sick
        && a.types == b.types
        && a.pt == b.pt
}
