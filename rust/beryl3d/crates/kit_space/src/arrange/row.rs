//! An ordered row of pre-grouped items. The profile can center the whole row when it fits,
//! reserve a right landing edge for hidden items, and link its scale to neighboring rows.

use glam::Vec2;
use kit_base::geometry::{GroupWindow, ScrollAxis};

use super::{Camera, Edge};

mod anchor;
mod strip;

/// An item tucked under the face of a group.
pub struct Tuck {
    pub key: usize,
    pub width: f32,
    pub edge: Edge,
}

/// A group with one face, any identical copies behind it, and items tucked under it.
pub struct Group {
    pub width: f32,
    /// Number of identical items, including the face.
    pub copies: usize,
    /// The change from a group outside this class to one inside it can use a wider gap.
    pub gap_class: bool,
    /// Preferred centre along the row when the groups all fit without overlap.
    pub anchor: Option<f32>,
    pub tucks: Vec<Tuck>,
    /// Distance each tuck rises in the plane per step from the face.
    pub tuck_rise: f32,
}

/// An item of a group, in back-to-front order when emitted by the solver.
pub enum Member {
    Copy(usize),
    Tuck(usize),
}

/// The pose components of one arranged item. The caller supplies the row plane and the
/// item's orientation in that plane.
pub struct Placed {
    pub group: usize,
    pub member: Member,
    pub centre: Vec2,
    pub layer: usize,
    pub visible: bool,
    /// The size of the pile on its face, 0 on copies, and 1 on tucked items.
    pub copies: usize,
}

/// A scroll window and the number of primary items off either end.
pub struct Window {
    pub groups: usize,
    pub scroll: GroupWindow,
    pub before: usize,
    pub after: usize,
}

/// Left and right edges and optional centre for an ordered row, in its local millimetres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Row {
    left: f32,
    right: f32,
    centre: Option<f32>,
    strip: f32,
    limit: f32,
    gap: f32,
    class_gap: f32,
    pile_step: Vec2,
    tuck_step: f32,
}

impl Row {
    /// Fit the row's exposed strip to the camera once, before measuring or placing its groups.
    /// `depth` is the shortest face's extent perpendicular to the row.
    #[must_use]
    pub fn new(
        [left, right]: [f32; 2],
        centre: Option<f32>,
        y: f32,
        camera: &Camera,
        (depth, press_points): (f32, f32),
    ) -> Self {
        Self {
            left,
            right,
            centre,
            strip: strip::least_strip(camera, [left, right], y, depth, press_points),
            limit: right,
            gap: 0.0,
            class_gap: 0.0,
            pile_step: Vec2::ZERO,
            tuck_step: 0.0,
        }
    }

    /// Set the ordinary and class-transition gaps, offsets behind a face, and farthest
    /// right edge an item outside the scroll window may rest at.
    #[must_use]
    pub fn with_spacing(
        self,
        (gap, class_gap): (f32, f32),
        pile_step: Vec2,
        tuck_step: f32,
        limit: f32,
    ) -> Self {
        Self {
            limit,
            gap,
            class_gap,
            pile_step,
            tuck_step,
            ..self
        }
    }

    /// Express the row in units before the display scale is applied to its items.
    #[must_use]
    pub fn at_scale(self, scale: f32) -> Self {
        Self {
            right: self.unscaled(self.right, scale),
            centre: self.centre.map(|centre| self.unscaled(centre, scale)),
            strip: self.strip / scale,
            limit: self.unscaled(self.limit, scale),
            ..self
        }
    }

    /// Convert a table coordinate to the row's local coordinate at the given display scale.
    #[must_use]
    pub fn unscaled(self, x: f32, scale: f32) -> f32 {
        self.left + (x - self.left) / scale
    }

    fn room(self) -> f32 {
        self.right - self.left
    }

    fn start(self, width: f32) -> f32 {
        self.centre.map_or(self.left, |centre| {
            (centre - width / 2.0)
                .min(self.right - width)
                .max(self.left)
        })
    }

    fn gap_between(self, before: bool, after: bool) -> f32 {
        if !before && after {
            self.class_gap
        } else {
            self.gap
        }
    }

    /// The proportion of its requested width the row has room for.
    #[must_use]
    pub fn fit(self, groups: impl IntoIterator<Item = Group>) -> f32 {
        let (mut widths, mut gaps) = (0.0, 0.0);
        let mut previous = None;
        for group in groups {
            widths += Pack::new(&group, self).width;
            if let Some(class) = previous {
                gaps += self.gap_between(class, group.gap_class);
            }
            previous = Some(group.gap_class);
        }
        let width = widths + gaps;
        if width > 0.0 {
            self.room() / width
        } else {
            1.0
        }
    }

    fn full_width(self, groups: &[Group], widths: impl Iterator<Item = f32>) -> f32 {
        widths.sum::<f32>()
            + groups
                .windows(2)
                .map(|pair| self.gap_between(pair[0].gap_class, pair[1].gap_class))
                .sum::<f32>()
    }
}

/// Both the smallest scale this row will take and the smallest scale it can demand
/// of the other rows. A value of zero allows the row to demand its whole fit.
#[derive(Clone, Copy)]
pub struct ScaleLimit {
    pub minimum: f32,
    pub shared_minimum: f32,
}

/// The rows with no independent floor follow the shared scale. A row with a floor of its own
/// may shrink further when it needs room, while its pull on the shared scale stops at
/// `shared_minimum`.
#[must_use]
pub fn linked_scales<const SIDES: usize, const ROWS: usize>(
    fits: [[f32; ROWS]; SIDES],
    minimum_shared: f32,
    limits: [Option<ScaleLimit>; ROWS],
) -> [[f32; ROWS]; SIDES] {
    let shared = fits
        .iter()
        .flat_map(|side| side.iter().zip(limits))
        .fold(1.0_f32, |shared, (&fit, limit)| {
            shared.min(limit.map_or(fit, |limit| fit.max(limit.shared_minimum)))
        })
        .max(minimum_shared);
    fits.map(|side| {
        std::array::from_fn(|i| {
            limits[i].map_or(shared, |limit| shared.min(side[i]).max(limit.minimum))
        })
    })
}

struct Pack {
    width: f32,
    left_peek: f32,
    tuck_step: f32,
    full_advance: bool,
}

impl Pack {
    fn new(group: &Group, row: Row) -> Self {
        let width = group.width + row.pile_step.x * gaps(group.copies);
        if group.tucks.is_empty() {
            return Self {
                width,
                left_peek: 0.0,
                tuck_step: 0.0,
                full_advance: false,
            };
        }
        let peek = |step, edge| {
            group
                .tucks
                .iter()
                .filter(|tuck| tuck.edge == edge)
                .enumerate()
                .map(|(i, tuck)| (i + 1) as f32 * step + (tuck.width - group.width) / 2.0)
                .fold(0.0, f32::max)
        };
        let narrower = group
            .tucks
            .iter()
            .map(|tuck| (group.width - tuck.width) / 2.0)
            .fold(0.0, f32::max);
        // Keep the exposed strip wide enough when the tucked item is narrower than the face.
        // A group too wide for the entire row cannot scroll into view, so close its tucks up
        // before measuring the space it needs.
        let fixed = group.width + peek(0.0, Edge::Start) + peek(0.0, Edge::End);
        let fits = (row.room() - fixed) / group.tucks.len() as f32;
        let tuck_step = row.tuck_step.max(row.strip + narrower).min(fits).max(0.0);
        let left_peek = peek(tuck_step, Edge::Start);
        Self {
            width: width + left_peek + peek(tuck_step, Edge::End),
            left_peek,
            tuck_step,
            // The next face would cover the exposed edges on this side first.
            full_advance: group.tucks.iter().any(|tuck| tuck.edge == Edge::End),
        }
    }

    fn place(
        &self,
        group: &Group,
        index: usize,
        row: Row,
        left: f32,
        (visible, layer): (bool, &mut usize),
        placed: &mut impl FnMut(Placed),
    ) {
        let face = left + self.left_peek + group.width / 2.0;
        let mut emit = |member, centre, copies| {
            placed(Placed {
                group: index,
                member,
                centre,
                layer: *layer,
                visible,
                copies,
            });
            *layer += 1;
        };
        for copy in (1..group.copies).rev() {
            emit(
                Member::Copy(copy),
                Vec2::new(face, 0.0) + row.pile_step * copy as f32,
                0,
            );
        }
        for (edge, sign) in [(Edge::Start, -1.0), (Edge::End, 1.0)] {
            let on_edge = |tuck: &&Tuck| tuck.edge == edge;
            let count = group.tucks.iter().filter(on_edge).count();
            for (index, tuck) in group.tucks.iter().rev().filter(on_edge).enumerate() {
                let reach = (count - index) as f32;
                emit(
                    Member::Tuck(tuck.key),
                    Vec2::new(
                        face + sign * reach * self.tuck_step,
                        reach * group.tuck_rise,
                    ),
                    1,
                );
            }
        }
        emit(Member::Copy(0), Vec2::new(face, 0.0), group.copies);
    }
}

fn gaps(count: usize) -> f32 {
    count.saturating_sub(1) as f32
}

/// Arrange the row, starting the scroll window at `offset` groups if it does not fit.
/// All items get rests, including those off either end.
pub fn arrange(
    row: Row,
    groups: &[Group],
    offset: usize,
    mut placed: impl FnMut(Placed),
) -> Window {
    let packs: Vec<Pack> = groups.iter().map(|group| Pack::new(group, row)).collect();
    let full = row.full_width(groups, packs.iter().map(|pack| pack.width));
    let room = row.room();
    let mut layer = 0;
    let mut counts = (0, 0);
    let (first, shown) = if full <= room || groups.is_empty() {
        if groups.iter().any(|group| group.anchor.is_some()) {
            let widths: Vec<f32> = packs.iter().map(|pack| pack.width).collect();
            let (order, lefts) = anchor::arrange(groups, &widths, full, row);
            for (index, left) in order.into_iter().zip(lefts) {
                packs[index].place(
                    &groups[index],
                    index,
                    row,
                    left,
                    (true, &mut layer),
                    &mut placed,
                );
            }
            return window(groups.len(), 0, groups.len(), counts);
        }
        let mut left = row.start(full);
        for (index, (group, pack)) in groups.iter().zip(&packs).enumerate() {
            pack.place(group, index, row, left, (true, &mut layer), &mut placed);
            let gap = groups
                .get(index + 1)
                .map_or(0.0, |next| row.gap_between(group.gap_class, next.gap_class));
            left += pack.width + gap;
        }
        (0, groups.len())
    } else {
        let (first, shown, step) = scroll_window(row, &packs, offset);
        let end = first + shown;
        let mut left = row.left;
        for (index, (group, pack)) in groups.iter().zip(&packs).enumerate() {
            let at = if index < first {
                counts.0 += group.copies;
                row.left
            } else if index >= end {
                counts.1 += group.copies;
                (row.right + row.gap).min(row.limit.max(row.right) - pack.width)
            } else {
                let at = left;
                left += if pack.full_advance { pack.width } else { step };
                at
            };
            pack.place(
                group,
                index,
                row,
                at,
                ((first..end).contains(&index), &mut layer),
                &mut placed,
            );
        }
        (first, shown)
    };
    window(groups.len(), first, shown, counts)
}

fn scroll_window(row: Row, packs: &[Pack], offset: usize) -> (usize, usize, f32) {
    let room = row.room();
    let widest = packs.iter().map(|pack| pack.width).fold(0.0, f32::max);
    let mut shown = (((room - widest) / row.strip).floor() as usize)
        .saturating_add(1)
        .clamp(1, packs.len());
    loop {
        let first = offset.min(packs.len() - shown);
        let last = first + shown - 1;
        let (whole, overlapped) =
            packs[first..last]
                .iter()
                .fold((0.0, 0), |(whole, count), pack| {
                    if pack.full_advance {
                        (whole + pack.width, count)
                    } else {
                        (whole, count + 1)
                    }
                });
        let remaining = room - packs[last].width - whole;
        let step = if overlapped > 0 {
            (remaining / overlapped as f32).max(0.0)
        } else {
            0.0
        };
        if shown == 1
            || (overlapped > 0 && step >= row.strip)
            || (overlapped == 0 && whole + packs[last].width <= room)
        {
            return (first, shown, step);
        }
        shown -= 1;
    }
}

fn window(groups: usize, first: usize, shown: usize, (before, after): (usize, usize)) -> Window {
    Window {
        groups,
        scroll: GroupWindow {
            first,
            shown,
            last: groups - shown,
            axis: ScrollAxis::Horizontal,
        },
        before,
        after,
    }
}
