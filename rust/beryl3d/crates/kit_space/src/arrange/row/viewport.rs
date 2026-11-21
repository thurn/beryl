//! Selects which packs to show, keeping the rests of every other pack for animation.

use kit_base::geometry::{GroupWindow, ScrollAxis};

use super::super::press::{self, PressArea, Quad};
use super::{Camera, Group, Pack, Placed, Projection, Row, Window, anchor};

/// Allow for rounding when widths and advances are accumulated in row coordinates.
const EDGE_EPSILON: f32 = 0.0001;

pub(super) struct Layout<'a> {
    row: Row,
    groups: &'a [Group],
    packs: &'a [Pack],
    camera: &'a Camera,
    press_points: f32,
    project: &'a dyn Fn(&Placed) -> Projection,
}

impl<'a> Layout<'a> {
    pub(super) fn new(
        row: Row,
        groups: &'a [Group],
        packs: &'a [Pack],
        (camera, press_points): (&'a Camera, f32),
        project: &'a dyn Fn(&Placed) -> Projection,
    ) -> Self {
        Self {
            row,
            groups,
            packs,
            camera,
            press_points,
            project,
        }
    }

    pub(super) fn full(&self, width: f32, emit: &mut impl FnMut(Placed)) -> bool {
        if self.groups.iter().any(|group| group.anchor.is_some()) {
            let widths: Vec<_> = self.packs.iter().map(|pack| pack.width).collect();
            let (order, lefts) = anchor::arrange(self.groups, &widths, width, self.row);
            return self.draw_full(order.into_iter().zip(lefts), emit);
        }
        let lefts =
            self.packs
                .iter()
                .enumerate()
                .scan(self.row.start(width), |left, (index, pack)| {
                    let at = *left;
                    let gap = self.groups.get(index + 1).map_or(0.0, |next| {
                        self.row
                            .gap_between(self.groups[index].gap_class, next.gap_class)
                    });
                    *left += pack.width + gap;
                    Some((index, at))
                });
        self.draw_full(lefts, emit)
    }

    fn draw_full(
        &self,
        positions: impl Iterator<Item = (usize, f32)> + Clone,
        emit: &mut impl FnMut(Placed),
    ) -> bool {
        if !self.fits(positions.clone()) {
            return false;
        }
        let mut layer = 0;
        for (index, left) in positions {
            self.place(index, left, (true, &mut layer), emit);
        }
        true
    }

    pub(super) fn overflow(&self, offset: usize, emit: &mut impl FnMut(Placed)) -> Window {
        // An unfit pack has a hidden rest, but no scroll offset spent on it. A large pile
        // or a host with too many tucks cannot stop a later pack appearing or being scrolled to.
        let eligible: Vec<_> = (0..self.packs.len())
            .filter(|&index| self.fits(std::iter::once((index, self.row.left))))
            .collect();
        let (first, shown, step) = self.select(&eligible, offset);
        let mut visible = self
            .positions(&eligible[first..first + shown], step)
            .peekable();
        let first_visible = visible.peek().map_or(0, |&(index, _)| index);
        let (mut before, mut after) = (0, 0);
        let (mut layer, mut rest_layer) = (0, 0);
        for (index, pack) in self.packs.iter().enumerate() {
            let in_view = visible.next_if(|&(next, _)| next == index);
            let left = if let Some((_, left)) = in_view {
                left
            } else if index < first_visible {
                before += self.groups[index].copies;
                self.row.left
            } else {
                after += self.groups[index].copies;
                (self.row.right + self.row.gap)
                    .min(self.row.limit - pack.width)
                    .max(self.row.left)
            };
            // Hidden rests keep their depth for animation. Their layers cannot raise the
            // visible cards out of the row, even if hundreds of copies were skipped.
            let shown = in_view.is_some();
            let mut hidden = rest_layer;
            self.place(
                index,
                left,
                (shown, if shown { &mut layer } else { &mut hidden }),
                emit,
            );
            rest_layer += self.groups[index].copies + self.groups[index].tucks.len();
        }
        let last = if first + shown == eligible.len() {
            first
        } else {
            eligible.len() - 1
        };
        Window {
            scroll: GroupWindow {
                first,
                shown,
                last,
                axis: ScrollAxis::Horizontal,
            },
            before,
            after,
        }
    }

    fn select(&self, eligible: &[usize], offset: usize) -> (usize, usize, f32) {
        if eligible.is_empty() {
            return (0, 0, 0.0);
        }
        let room = self.row.room();
        let widest = eligible
            .iter()
            .map(|&index| self.packs[index].width)
            .fold(0.0, f32::max);
        let most = (((room - widest) / self.row.strip).floor() as usize)
            .saturating_add(1)
            .clamp(1, eligible.len());
        for shown in (1..=most).rev() {
            let first = offset.min(eligible.len() - shown);
            let indices = &eligible[first..first + shown];
            let (last, preceding) = indices.split_last().expect("the window has a pack");
            let (whole, overlapped) = preceding.iter().fold((0.0, 0), |(whole, count), &index| {
                let pack = &self.packs[index];
                if pack.full_advance {
                    (whole + pack.width, count)
                } else {
                    (whole, count + 1)
                }
            });
            let remaining = room - self.packs[*last].width - whole;
            let step = if overlapped == 0 {
                0.0
            } else {
                remaining / overlapped as f32
            };
            if remaining >= -EDGE_EPSILON
                && (overlapped == 0 || step >= self.row.strip)
                && self.fits(self.positions(indices, step))
            {
                return (first, shown, step);
            }
        }
        (0, 0, 0.0)
    }

    fn positions<'b>(
        &'b self,
        indices: &'b [usize],
        step: f32,
    ) -> impl Iterator<Item = (usize, f32)> + Clone + 'b {
        indices.iter().scan(self.row.left, move |left, &index| {
            let at = *left;
            let pack = &self.packs[index];
            *left += if pack.full_advance { pack.width } else { step };
            Some((index, at))
        })
    }

    fn place(
        &self,
        index: usize,
        left: f32,
        layer: (bool, &mut usize),
        emit: &mut impl FnMut(Placed),
    ) {
        self.packs[index].place(&self.groups[index], index, self.row, left, layer, emit);
    }

    fn fits(&self, positions: impl Iterator<Item = (usize, f32)> + Clone) -> bool {
        // Each pack reserves its own width, even when a later pack starts before its end.
        if positions.clone().any(|(index, left)| {
            left < self.row.left - EDGE_EPSILON
                || left + self.packs[index].width > self.row.right + EDGE_EPSILON
        }) {
            return false;
        }
        // The precomputed strip covers plain faces. Check projected faces when tucks introduce
        // additional occlusion, using the same square witness as the generic layout solver.
        if self.press_points <= 0.0
            || !positions
                .clone()
                .any(|(index, _)| !self.groups[index].tucks.is_empty())
        {
            return true;
        }
        let screen = press::safe_area(self.camera);
        let mut surfaces = Vec::new();
        let mut layer = 0;
        for (index, left) in positions {
            self.place(index, left, (true, &mut layer), &mut |placed| {
                let (corners, pressable) = (self.project)(&placed);
                surfaces.push((Quad::new(corners), pressable));
            });
        }
        surfaces
            .iter()
            .enumerate()
            .all(|(index, (quad, pressable))| {
                !pressable
                    || (quad.has_area()
                        && PressArea::new(quad, &screen, self.press_points)
                            .witness(
                                surfaces[index + 1..]
                                    .iter()
                                    .map(|(other, _)| other)
                                    .filter(|other| other.bounds.overlaps(quad.bounds)),
                            )
                            .is_some())
            })
    }
}
