//! One battlefield row at rest (docs/beryl-3d.md, "Objects and placement"). A row lays its
//! groups out at full size while they fit its width, then overlaps them down to the layout's
//! least uncovered strip. If that is still too long, it shows the groups in the scroll window.
//! The other cards of a merged permanent and the cards it holds in exile peek out from under
//! it, as in the web interface's `tuck` (`ui/src/game/battlefield.ts`).

use std::f32::consts::FRAC_1_SQRT_2;

use beryl_protocol::protocol::{CardType, FaceName, Facing, ObjectId, ObjectView, ZoneName};
use glam::{Quat, Vec2, Vec3};

use super::place::host_slot;
use super::{CARD_HEIGHT, CARD_WIDTH, Camera, LAYER, Layout, RowArea};
use crate::data::space::card::PHASED_OUT_OPACITY;
use crate::pose::Pose;

mod change;
mod names;
mod single;
pub(crate) use change::RowChange;
pub(crate) use names::NameIds;
pub(super) use names::SameName;
use single::{beyond_right, lay_single_cards, spacing, window_size};

/// One card of a laid-out row: its pose, and whether it is drawn.
pub(super) struct Laid<'a> {
    pub object: &'a ObjectView,
    pub pose: Pose,
    pub visible: bool,
    /// The group's size on its top card, and 0 on the copies beneath it (`Rest::copies`).
    pub copies: u16,
}

/// Which of a row's groups are in view: `shown` of its `groups` from `first`, with `before`
/// cards scrolled off its left end and `after` off its right end. A row that fits shows every
/// group from the first. Its numbers are kept in `u16`s, which hold any row of a table, because a
/// window for each row is kept in every script's view at rest: as `usize`s they would make each
/// waiting script larger than 1024 bytes, past which a growing `Vec` of them reallocates more
/// often.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct RowWindow {
    groups: u16,
    first: u16,
    shown: u16,
    before: u16,
    after: u16,
}

impl RowWindow {
    /// A window of `shown` of `groups` groups from `first`, with `before` and `after` cards off
    /// the two ends.
    fn new(groups: usize, first: usize, shown: usize, (before, after): (usize, usize)) -> Self {
        let small = |number: usize| u16::try_from(number).unwrap_or(u16::MAX);
        Self {
            groups: small(groups),
            first: small(first),
            shown: small(shown),
            before: small(before),
            after: small(after),
        }
    }

    /// The first group in view.
    #[must_use]
    pub fn first(&self) -> usize {
        usize::from(self.first)
    }

    /// How many cards are off the left end.
    #[must_use]
    pub fn before(&self) -> usize {
        usize::from(self.before)
    }

    /// How many cards are off the right end.
    #[must_use]
    pub fn after(&self) -> usize {
        usize::from(self.after)
    }

    /// Whether the row has cards off either end, and so scrolls.
    #[must_use]
    pub fn scrolls(&self) -> bool {
        self.before + self.after > 0
    }

    /// The first group it may show: from 0 to the last from which every group to the end fits.
    #[must_use]
    pub fn clamp(&self, first: isize) -> usize {
        let last = usize::from(self.groups.saturating_sub(self.shown));
        usize::try_from(first.max(0)).unwrap_or(0).min(last)
    }
}

/// How a row is laid out: the least strip in millimetres, the layout's spacing, and the row's
/// scroll offset in groups.
pub(super) struct RowRules<'a> {
    pub layout: &'a Layout,
    pub strip: f32,
    pub scroll: usize,
}

/// The cards that lie under a row's permanents (`place::under`), each with the permanent it
/// lies under: the other cards of a merged permanent and the cards a permanent holds in exile,
/// which peek out to its left, and the Auras and Equipment attached to it, which peek out to
/// its right, as the web interface's `tuck` draws them.
pub(super) struct Beneath<'a> {
    cards: Vec<(ObjectId, &'a ObjectView)>,
}

impl<'a> Beneath<'a> {
    /// The cards of `under` that lie under one of `permanents`, each with the permanent it lies
    /// under.
    pub(super) fn new(under: Vec<&'a ObjectView>, permanents: &[&'a ObjectView]) -> Self {
        let find = |slot: ObjectId| permanents.iter().copied().find(|p| p.slot == slot);
        let cards = under
            .into_iter()
            .filter_map(|card| Some((host_slot(card, find)?, card)))
            .collect();
        Self { cards }
    }

    /// The cards under the permanent in `host` that peek out to its left (`left`) or to its
    /// right, in the order the row lists them.
    fn of(&self, host: ObjectId, left: bool) -> impl Iterator<Item = &'a ObjectView> + '_ {
        self.cards
            .iter()
            .filter(move |(holder, card)| *holder == host && peeks_left(card) == left)
            .map(|(_, card)| *card)
    }

    /// Whether any card lies under the permanent in `host`.
    fn holds(&self, host: ObjectId) -> bool {
        self.cards.iter().any(|(holder, _)| *holder == host)
    }

    /// How far the cards under `host` that peek out to its left (`left`) or right reach past
    /// its edge, in millimetres: each lies `step` millimetres further out than the one before
    /// it (`peek_step`).
    fn peek(&self, host: &ObjectView, step: f32, left: bool) -> f32 {
        self.of(host.slot, left)
            .enumerate()
            .map(|(index, card)| {
                (index + 1) as f32 * step + (card_width(card) - card_width(host)) / 2.0
            })
            .fold(0.0, f32::max)
    }
}

/// How far, in millimetres, each card under a permanent peeks out past the one before it: the
/// web interface's step, `beneath_step` of a card's width, or the least strip that leaves an
/// overlapped card of the row reachable where that is wider, as it is in a small window, so
/// that a press can still reach a card tucked under another.
fn peek_step(rules: &RowRules) -> f32 {
    (rules.layout.beneath_step.x * CARD_WIDTH).max(rules.strip)
}

/// Whether a card under a permanent peeks out to its left: every card but an Aura or Equipment
/// attached to it, which peeks out to its right, as the web interface's `tuck` has it.
fn peeks_left(card: &ObjectView) -> bool {
    card.zone != ZoneName::Battlefield
}

/// A row's cards gathered into groups laid out side by side.
enum Groups<'a, 'b> {
    /// Each card a group of its own.
    Single(&'b [&'a ObjectView]),
    /// Each group's cards a run of `cards`, its top card first; `ends` holds where each run
    /// ends.
    Piles {
        cards: Vec<&'a ObjectView>,
        ends: Vec<usize>,
    },
}

impl<'a> Groups<'a, '_> {
    fn len(&self) -> usize {
        match self {
            Groups::Single(cards) => cards.len(),
            Groups::Piles { ends, .. } => ends.len(),
        }
    }

    /// The groups, in order.
    fn iter(&self) -> impl Iterator<Item = &[&'a ObjectView]> {
        (0..self.len()).map(|index| match self {
            Groups::Single(cards) => &cards[index..=index],
            Groups::Piles { cards, ends } => {
                let start = index.checked_sub(1).map_or(0, |before| ends[before]);
                &cards[start..ends[index]]
            }
        })
    }
}

/// Lays out `cards`, in order from left to right, in `area` at height `y` on the table. `names`
/// links each card to the one before it with its name, if the caller has them already, and
/// `ids` numbers the cards' names, if the caller keeps numbers. Each card goes to `laid` as it is
/// laid out.
pub(super) fn lay_row<'a>(
    (cards, beneath): (&[&'a ObjectView], &Beneath<'a>),
    (names, ids, stack_slots): (Option<SameName>, Option<&NameIds<'_>>, Option<&[ObjectId]>),
    area: RowArea,
    y: f32,
    rules: &RowRules,
    laid: &mut impl FnMut(Laid<'a>),
) -> RowWindow {
    let room = area.right - area.left;
    let groups =
        stack_slots
            .filter(|_| cards.len() > 1)
            .map_or(Groups::Single(cards), |stack_slots| {
                // On a short row, rule out singletons before allocating the name index.
                if names.is_none()
                    && cards.len() <= 8
                    && !cards
                        .iter()
                        .enumerate()
                        .any(|(index, &card)| cards[..index].iter().any(|other| twins(card, other)))
                {
                    return Groups::Single(cards);
                }
                piles(
                    cards,
                    &names.unwrap_or_else(|| SameName::of(cards, ids)),
                    beneath,
                    stack_slots,
                )
            });
    if let Groups::Single(cards) = groups
        && beneath.cards.is_empty()
    {
        return lay_single_cards(cards, area, y, rules, laid);
    }
    let widths: Vec<f32> = groups
        .iter()
        .map(|g| group_width(g, beneath, rules))
        .collect();
    let full: f32 = widths.iter().sum::<f32>() + rules.layout.gap * gaps(groups.len());
    // An empty row lays out nothing however little room it has. Without this, a row with no
    // room would reach the window below, which has to show at least one of no groups.
    if full <= room || groups.len() == 0 {
        let mut left = if area.centred {
            area.left + (room - full) / 2.0
        } else {
            area.left
        };
        let mut height = 0;
        for (group, width) in groups.iter().zip(&widths) {
            place_group((group, beneath), left, y, &mut height, true, rules, laid);
            left += width + rules.layout.gap;
        }
        return RowWindow::new(groups.len(), 0, groups.len(), (0, 0));
    }
    // Every window of `shown` consecutive groups fits at the least strip, however wide its
    // last group is.
    let widest = widths.iter().copied().fold(0.0, f32::max);
    let shown = window_size(groups.len(), room, widest, rules.strip);
    let first = rules.scroll.min(groups.len() - shown);
    let last_width = widths[first + shown - 1];
    // A group with cards attached to it is followed that much further on, so that what peeks
    // out to its right is left uncovered as its own card is.
    let right = |group: &[&ObjectView]| beneath.peek(group[0], peek_step(rules), false);
    let extra: f32 = groups
        .iter()
        .skip(first)
        .take(shown.saturating_sub(1))
        .map(right)
        .sum();
    let step = spacing(shown, room, last_width, extra);
    let mut height = 0;
    let (mut before, mut after) = (0, 0);
    let mut left = area.left;
    for (index, (group, width)) in groups.iter().zip(&widths).enumerate() {
        let visible = (first..first + shown).contains(&index);
        if index < first {
            before += group.len();
        } else if !visible {
            after += group.len();
        }
        // A group scrolled out of view rests at the nearer end of the row, undrawn.
        let at = if index < first {
            area.left
        } else if visible {
            left
        } else {
            beyond_right(area, rules, *width)
        };
        place_group((group, beneath), at, y, &mut height, visible, rules, laid);
        if visible {
            left += step + right(group);
        }
    }
    RowWindow::new(groups.len(), first, shown, (before, after))
}

/// The least strip, in millimetres, that leaves an overlapped card of a row at height `y` in
/// `area` an uncovered square `side` points on a side, wherever in the row it stands. The strip
/// runs between the card's left edge and its neighbour's. Under the camera's perspective those
/// edges slant toward the middle of the screen, more the farther from it they stand, so at the
/// row's ends a strip `side` points wide at the row's centre line holds no square that large.
/// The strip is the least that holds one at both ends of the row, over the shorter side of a
/// card, which is how tall a tapped card lies.
pub(super) fn least_strip(camera: &Camera, area: RowArea, y: f32, side: f32) -> f32 {
    let flat = side / camera.points_per_millimetre(Vec3::new(0.0, y, 0.0));
    let holds = |strip: f32| {
        [area.left, area.right - strip]
            .into_iter()
            .all(|left| holds_square(camera, (left, left + strip), y, side))
    };
    // Every strip at least `flat` and too narrow to hold the square is narrower than every
    // strip that holds it, so a bisection finds the least to within a hundredth of a millimetre.
    let (mut low, mut high) = (flat, flat);
    while !holds(high) {
        low = high;
        high *= 2.0;
        if high > area.right - area.left {
            return high;
        }
    }
    while high - low > 0.01 {
        let middle = f32::midpoint(low, high);
        if holds(middle) {
            high = middle;
        } else {
            low = middle;
        }
    }
    high
}

/// Whether the part of the table between `left` and `right`, across a card's shorter side
/// centred on `y`, holds a square `side` points on a side on screen. Each edge of the part is a
/// straight line on screen, and points at one height of the table share a height on screen, so
/// the width a band of the screen holds is the least gap between the edges at its top and its
/// bottom.
fn holds_square(camera: &Camera, (left, right): (f32, f32), y: f32, side: f32) -> bool {
    let point = |x: f32, along: f32| camera.to_screen(Vec3::new(x, y + along, 0.0));
    let half = CARD_WIDTH / 2.0;
    let (near_left, far_left) = (point(left, -half), point(left, half));
    let (near_right, far_right) = (point(right, -half), point(right, half));
    let (top, bottom) = (far_left.y.min(near_left.y), far_left.y.max(near_left.y));
    if bottom - top < side {
        return false;
    }
    // Where the line from `from` to `to` crosses height `at` of the screen.
    let across =
        |from: Vec2, to: Vec2, at: f32| from.x + (to.x - from.x) * (at - from.y) / (to.y - from.y);
    let gap = |start: f32| {
        let end = start + side;
        let left = across(near_left, far_left, start).max(across(near_left, far_left, end));
        let right = across(near_right, far_right, start).min(across(near_right, far_right, end));
        right - left
    };
    (0..=SQUARE_BANDS)
        .map(|band| top + (bottom - top - side) * f32::from(band) / f32::from(SQUARE_BANDS))
        .any(|start| gap(start) >= side)
}

/// How many steps `holds_square` tries a band of the screen at, from the part's top to its
/// bottom.
const SQUARE_BANDS: u8 = 16;

/// The number of spaces between `count` things in a line.
fn gaps(count: usize) -> f32 {
    count.saturating_sub(1) as f32
}

/// Puts a group's cards at `left`, the copies beneath peeking out to the right and away from
/// the viewer, and the cards under a permanent peeking out away from the viewer to its left, or
/// to its right for the Auras and Equipment attached to it. Every card lies one `LAYER` above
/// the card placed before it, whether or not the row overlaps: a tapped card is wider than an untapped row's step, and two cards at one height
/// would flicker where they overlap.
fn place_group<'a>(
    (group, beneath): (&[&'a ObjectView], &Beneath<'a>),
    left: f32,
    y: f32,
    height: &mut u32,
    visible: bool,
    rules: &RowRules,
    laid: &mut impl FnMut(Laid<'a>),
) {
    let step = rules.layout.beneath_step;
    let across_step = peek_step(rules);
    let left = left + beneath.peek(group[0], across_step, true);
    // The top card is the group's first; the copies beneath are placed first, lowest.
    for (depth, &object) in group.iter().enumerate().rev() {
        let offset = rules.layout.pile_step * depth as f32;
        let centre = left + card_width(object) / 2.0 + offset.x;
        if !beneath.cards.is_empty() {
            // The cards under a permanent peek out away from the viewer on both sides, as the copies
            // of a pile do, where the web's peek toward their controller. A card nearer the viewer
            // is nearer the camera, which the interface checks and the hit test take for in front,
            // so one peeking out below its permanent would read as covering it. A land keeps them
            // level, as the web keeps them clear of the badges above the lands' row.
            let rise = if object.types.contains(&CardType::Land) {
                0.0
            } else {
                step.y * CARD_HEIGHT
            };
            // The cards to its right, then those to its left, each lower than the one nearer it.
            let right = beneath
                .of(object.slot, false)
                .enumerate()
                .map(|(i, c)| (i, c, 1.0));
            let left_of = beneath
                .of(object.slot, true)
                .enumerate()
                .map(|(i, c)| (i, c, -1.0));
            let under: Vec<(usize, &ObjectView, f32)> = right.chain(left_of).collect();
            for &(index, card, across) in under.iter().rev() {
                let reach = (index + 1) as f32;
                let pose = Pose {
                    position: Vec3::new(
                        centre + across * reach * across_step,
                        y + offset.y + reach * rise,
                        LAYER * *height as f32,
                    ),
                    rotation: Quat::IDENTITY,
                    turn: if face_up(card) { 0.0 } else { 1.0 },
                    ..Pose::default()
                };
                *height += 1;
                laid(Laid {
                    object: card,
                    pose,
                    visible,
                    copies: 1,
                });
            }
        }
        let copies = if depth == 0 {
            u16::try_from(group.len()).unwrap_or(u16::MAX)
        } else {
            0
        };
        laid(permanent(
            object,
            centre,
            y + offset.y,
            *height,
            visible,
            copies,
        ));
        *height += 1;
    }
}

fn permanent(
    object: &ObjectView,
    centre: f32,
    y: f32,
    height: u32,
    visible: bool,
    copies: u16,
) -> Laid<'_> {
    let rotation = if object.tapped {
        Quat::from_xyzw(0.0, 0.0, -FRAC_1_SQRT_2, FRAC_1_SQRT_2)
    } else {
        Quat::IDENTITY
    };
    let pose = Pose {
        position: Vec3::new(centre, y, LAYER * height as f32),
        rotation,
        ..Pose::default()
    };
    Laid {
        object,
        pose: reposed(object, pose),
        visible,
        copies,
    }
}

/// `pose` with what `object`'s own record decides of a permanent's pose in its row and nothing
/// else does: whether it shows its face, and how faded it is. A step that changes only those
/// (`RowChange::Pose`) gives the card its new pose with this, and lays out nothing else.
pub(crate) fn reposed(object: &ObjectView, pose: Pose) -> Pose {
    Pose {
        turn: if face_up(object) { 0.0 } else { 1.0 },
        // A phased-out permanent stays where it lay, faded, as the web interface draws it:
        // it is treated as though it does not exist
        // ([CR 702.26b](https://yawgatog.com/resources/magic-rules/#R70226b)).
        opacity: if object.phased_out {
            PHASED_OUT_OPACITY
        } else {
            1.0
        },
        ..pose
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

/// How wide a group lies: its top card, the copies peeking out beside it and the cards under a
/// merged permanent peeking out to its left.
fn group_width(group: &[&ObjectView], beneath: &Beneath, rules: &RowRules) -> f32 {
    card_width(group[0])
        + rules.layout.pile_step.x * gaps(group.len())
        + beneath.peek(group[0], peek_step(rules), true)
        + beneath.peek(group[0], peek_step(rules), false)
}

/// Whether the viewer sees a card's face: a face-up card whose name they know.
pub(super) fn face_up(object: &ObjectView) -> bool {
    object.facing == Facing::Up && object.name.is_some()
}

/// `cards` with identical permanents gathered into piles, each pile where its first card was.
/// The pile's first card is the one with the lowest slot, which lies on top: the engine offers
/// the lowest slot of interchangeable objects, so what a choice takes leaves from the top.
///
/// Only cards with the same name can be twins, so each card is compared only with the piles of
/// its own name, found through `names`. A row of a thousand different names would otherwise
/// compare every card with every pile.
fn piles<'a, 'b>(
    cards: &'b [&'a ObjectView],
    names: &SameName,
    beneath: &Beneath,
    stack_slots: &[ObjectId],
) -> Groups<'a, 'b> {
    if !names.repeats() {
        return Groups::Single(cards);
    }
    // The index in `cards` of the first card of each card's pile.
    let mut tops: Vec<usize> = (0..cards.len()).collect();
    let plain = |card: &ObjectView| {
        plain(card) && !beneath.holds(card.slot) && stack_slots.binary_search(&card.slot).is_err()
    };
    for (index, &card) in cards.iter().enumerate() {
        if !plain(card) {
            continue;
        }
        // Twins is an equivalence: a card that twins any card of a pile twins its first card,
        // and of the piles of one name at most one takes it.
        let mut earlier = names.before(index);
        while let Some(other) = earlier {
            if plain(cards[other]) && twins(cards[other], card) {
                tops[index] = tops[other];
                break;
            }
            earlier = names.before(other);
        }
    }
    if tops.iter().enumerate().all(|(index, &top)| index == top) {
        return Groups::Single(cards);
    }
    // Each pile's cards as one run, the piles in the order of their first cards: count each
    // pile's cards, then put each card at its pile's next free place.
    let mut ends: Vec<usize> = Vec::with_capacity(cards.len());
    let mut pile_of = vec![0; cards.len()];
    for index in 0..cards.len() {
        if tops[index] == index {
            pile_of[index] = ends.len();
            ends.push(0);
        }
        ends[pile_of[tops[index]]] += 1;
    }
    let mut total = 0;
    for end in &mut ends {
        total += *end;
        *end = total;
    }
    let mut next: Vec<usize> = std::iter::once(0).chain(ends.iter().copied()).collect();
    let mut gathered = cards.to_vec();
    for (index, &card) in cards.iter().enumerate() {
        let pile = pile_of[tops[index]];
        gathered[next[pile]] = card;
        next[pile] += 1;
    }
    let mut start = 0;
    for &end in &ends {
        if end - start > 1 {
            gathered[start..end].sort_by_key(|object| object.slot.0);
        }
        start = end;
    }
    Groups::Piles {
        cards: gathered,
        ends,
    }
}

/// Whether a permanent may share a pile: nothing on it or about it that a pile would hide.
pub(super) fn plain(object: &ObjectView) -> bool {
    face_up(object)
        && object.face != FaceName::Down
        && object.note.as_deref().is_none_or(str::is_empty)
        && !object.doesnt_untap
        && object.named_counters.is_empty()
        && object.counters.is_empty()
        && object.damage == 0
        && object.combat.is_none()
        && object.attached_to.is_none()
        && !object.phased_out
        && object.really.as_deref().is_none_or(str::is_empty)
        && object.chosen.as_deref().is_none_or(str::is_empty)
}

/// Whether two permanents are alike in everything the table shows of them.
pub(super) fn twins(a: &ObjectView, b: &ObjectView) -> bool {
    a.slot != b.slot && a.name.is_some() && same_pile_face(a, b)
}

fn same_pile_face(a: &ObjectView, b: &ObjectView) -> bool {
    a.name == b.name
        && a.facing == b.facing
        && a.zone == b.zone
        && a.owner == b.owner
        && a.controller == b.controller
        && a.token == b.token
        && a.tapped == b.tapped
        && a.summoning_sick == b.summoning_sick
        && a.types == b.types
        && a.pt == b.pt
}
