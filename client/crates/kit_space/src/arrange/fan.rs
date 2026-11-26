//! Centred rows of rectangles hanging from a circular arc along a screen edge.

use glam::{Quat, Vec2};
use kit_base::geometry::{GroupWindow, ScrollAxis};

use super::press::{Bounds, PressArea, Quad, Rectangle, safe_area};
use super::{Camera, Container, Item, Overflow, Placed, Solved};

/// A screen-facing fan whose anchor is the inner edge of the middle item in the front row.
/// The region's width is the available width in the anchor's plane. Its height is the room
/// for back rows above that edge. Rectangles may extend out of the window along the outer edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CentredFan {
    /// The tallest item as a fraction of the window's height.
    pub height: f32,
    /// Radius, preferred centre spacing and maximum centre span in widths of the widest item.
    pub radius: f32,
    pub step: f32,
    pub span: f32,
    /// The fan's lean in radians.
    pub bias: f32,
    /// Draw the items upside down, with their arc along the top of the screen.
    pub inverted: bool,
    /// How much nearer the eye each successive item rests, in millimetres.
    pub depth_step: f32,
    /// The leftmost any item of a row above the front row may reach, measured from the anchor
    /// in halves of the window's height, the units of the region's size divided by the height
    /// of a card at the anchor's depth that fills half the window. Back rows then centre their
    /// items between this limit and the region's right edge. The front row takes the first
    /// items and the rows behind it the newest, each row behind as long as it may be. `None`
    /// lays every row about the anchor, the first items in the farthest row. Only an
    /// overlapping fan that wraps into rows uses it.
    pub back_left: Option<f32>,
}

pub(super) fn arrange(
    container: &Container,
    fan: CentredFan,
    items: &[Item],
    camera: &Camera,
    scroll: usize,
) -> Solved {
    let size = items
        .iter()
        .fold(Vec2::ZERO, |size, item| size.max(item.size));
    let anchor = container.region.centre;
    let depth = camera.depth(anchor);
    let units = camera
        .screen_pose(Vec2::ZERO, depth, Quat::IDENTITY, 0.5)
        .scale
        * camera.data.card.height;
    let ndc = camera.project(anchor).truncate();
    let window = camera.window.size;
    let target = container.limits.press_points;
    let safe = safe_area(camera);
    let fit = Fit {
        fan,
        camera,
        depth,
        ndc,
        width: size.x * fan.height * 2.0 / size.y.max(f32::EPSILON),
        height: size.y,
        room: container.region.size / units,
        strip: target * 2.0 / window.height,
        // A little spare room leaves a square where two turned edges meet.
        margin: 16.0 / window.height,
        target,
        safe,
    };
    let count = items.len();
    let mut first = 0;
    let mut shown = 0;
    let mut placed = Vec::new();
    let mut projected = Vec::new();
    let mut overflow = Overflow::FullSize;
    let overlap = container.limits.overlap;
    if count > 0 {
        // All items in a hand have the same footprint. The capacity bound avoids retrying
        // arbitrarily long lists when the region has room for only a few dozen rectangles.
        for visible in (1..=count.min(fit.capacity(overlap))).rev() {
            let start = if container.limits.scroll {
                scroll.min(count - visible)
            } else {
                0
            };
            if let Some(overlapped) = fit.rows(
                &items[start..start + visible],
                overlap,
                &mut placed,
                &mut projected,
            ) {
                first = start;
                shown = visible;
                if overlapped {
                    overflow = Overflow::Overlapped;
                }
                break;
            }
        }
    }
    let after = count - first - shown;
    if shown < count {
        overflow = if container.limits.scroll && shown > 0 {
            Overflow::Scrolled
        } else {
            Overflow::Clipped
        };
    }
    let anchor_pose = camera.screen_pose(ndc, depth, Quat::IDENTITY, fan.height);
    let before_pose = placed.first().map_or(anchor_pose, |item| item.pose);
    let after_pose = placed.last().map_or(anchor_pose, |item| item.pose);
    let hidden = |item: &Item, pose| Placed {
        key: item.key,
        pose,
        in_view: false,
        count: 1,
        layer: 0,
        press_square: None,
    };
    let placed = if shown == count {
        placed
    } else {
        items[..first]
            .iter()
            .map(|item| hidden(item, before_pose))
            .chain(placed)
            .chain(
                items[first + shown..]
                    .iter()
                    .map(|item| hidden(item, after_pose)),
            )
            .collect()
    };
    Solved {
        items: placed,
        overflow,
        before: first,
        after,
        scroll: (overflow == Overflow::Scrolled).then_some(GroupWindow {
            first,
            shown,
            last: count - shown,
            axis: ScrollAxis::Horizontal,
        }),
    }
}

struct Fit<'a> {
    fan: CentredFan,
    camera: &'a Camera,
    depth: f32,
    ndc: Vec2,
    width: f32,
    height: f32,
    room: Vec2,
    strip: f32,
    margin: f32,
    target: f32,
    safe: Quad,
}

/// Where one row's item centres may stand across the fan, in the fan's units from the anchor:
/// the distance the centres of its outer items may span, and the middle of that span.
#[derive(Clone, Copy)]
struct RowRoom {
    span: f32,
    centre: f32,
}

/// One laid row: how many items it holds, their centres' spacing, how far each row stands above
/// the next one forward, and the middle of the row from the anchor.
#[derive(Clone, Copy)]
struct RowShape {
    length: usize,
    spacing: f32,
    rise: f32,
    centre: f32,
}

#[derive(Clone, Copy)]
struct ScreenPose {
    ndc: Vec2,
    height: f32,
    angle: f32,
    axis: Vec2,
}

struct Trial {
    pose: ScreenPose,
    rectangle: Rectangle,
    bounds: Bounds,
    press_square: Option<[Vec2; 4]>,
}

// Counts fit in the precision of f32 for any list of rectangles the scene can display.
#[allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
impl Fit<'_> {
    fn capacity(&self, overlap: bool) -> usize {
        // Wrapping does not change the width required for a single card.
        if self.width > self.room.x {
            return 0;
        }
        if overlap && self.strip <= 0.0 {
            return usize::MAX;
        }
        let step = if overlap {
            self.strip + self.margin
        } else {
            self.width
        };
        let columns = (((self.room.x - self.width) / step).max(0.0) as usize).saturating_add(1);
        let rows = ((self.room.y.max(0.0) / self.least_rise(overlap)) as usize).saturating_add(1);
        match self.back_room(overlap) {
            Some(back) => {
                let back_columns = if back.span < 0.0 {
                    0
                } else {
                    ((back.span / step) as usize).saturating_add(1)
                };
                columns.saturating_add((rows - 1).saturating_mul(back_columns))
            }
            None => columns.saturating_mul(rows),
        }
    }

    /// The room of a row centred on the anchor.
    fn centred(&self) -> RowRoom {
        RowRoom {
            span: self.room.x - self.width,
            centre: 0.0,
        }
    }

    /// The room of a back row when the fan has a left limit for them: from the first centre at
    /// which an item's outline, turned as the arc turns it there, stays right of the limit, to
    /// the region's right edge less half an item.
    fn back_room(&self, overlap: bool) -> Option<RowRoom> {
        let left = self.fan.back_left.filter(|_| overlap)?;
        let right = (self.room.x - self.width) / 2.0;
        let half = self.width / 2.0;
        let mut first = left + half;
        // An item right of the anchor turns clockwise about the middle of its top edge, so its
        // top corner comes in by the cosine and its bottom corner swings left by the sine of
        // its full height. The turn grows only slowly with the distance, so a few rounds settle.
        for _ in 0..3 {
            let lean = self.lean_at(first);
            first = left + half * lean.cos() + 2.0 * self.fan.height * lean.sin().max(0.0);
        }
        Some(RowRoom {
            span: right - first,
            centre: f32::midpoint(first, right),
        })
    }

    /// The clockwise turn of an item whose top edge's middle is `x` from the anchor.
    fn lean_at(&self, x: f32) -> f32 {
        (x / self.radius()).clamp(-1.0, 1.0).asin() + self.fan.bias
    }

    fn least_rise(&self, overlap: bool) -> f32 {
        if overlap {
            self.strip + self.margin
        } else {
            2.0 * self.fan.height
        }
    }

    fn rows(
        &self,
        items: &[Item],
        overlap: bool,
        placed: &mut Vec<Placed>,
        projected: &mut Vec<Trial>,
    ) -> Option<bool> {
        let count = items.len();
        let max_rows = if (self.strip > 0.0 || !overlap) && !self.fan.inverted {
            count
        } else {
            1
        };
        let least_rise = self.least_rise(overlap);
        let centred = self.centred();
        let back = self.back_room(overlap);
        for rows in 1..=max_rows {
            if rows > 1 && (rows - 1) as f32 * least_rise > self.room.y {
                break;
            }
            if let Some(back) = back.filter(|_| rows > 1) {
                // Every back row holds `length` items and the front row the rest. The longest
                // back rows come first, so the front row is as short as it may be. The newest
                // items go behind: a drawn card is held up over the right end of the hand, where
                // the back rows lie, and a card still in flight to a back row would land under
                // it, while the first items, in the front row, rest left of it.
                for length in (1..=(count - 1) / (rows - 1)).rev() {
                    let front = count - (rows - 1) * length;
                    let Some(front) = self.overlapping_row(front, centred) else {
                        break;
                    };
                    let Some(back) = self.overlapping_row(length, back) else {
                        continue;
                    };
                    let rise = front.rise.max(back.rise);
                    if (rows - 1) as f32 * rise > self.room.y {
                        continue;
                    }
                    let shape = |row: usize| {
                        let shape = if row + 1 < rows { back } else { front };
                        RowShape { rise, ..shape }
                    };
                    if let Some(overlapped) =
                        self.trial(items, (rows, true), shape, overlap, placed, projected)
                    {
                        return Some(overlapped);
                    }
                }
                continue;
            }
            let columns = count.div_ceil(rows);
            let spacing = self.spacing(columns, overlap, centred);
            let widest = Self::widest(columns, centred);
            if (self.strip > 0.0 || !overlap) && spacing > widest {
                continue;
            }
            let spacing = spacing.min(widest);
            let rise = if overlap {
                self.rise(columns, spacing, centred)
            } else {
                self.separated_rise(columns, spacing)
                    .min(self.room.y / (rows - 1).max(1) as f32)
            };
            if rows > 1 && (rows - 1) as f32 * rise > self.room.y {
                continue;
            }
            // Rows of the same length use the same spacing and rise.
            let full_rows = (count - 1) % rows + 1;
            let full = RowShape {
                length: columns,
                spacing,
                rise,
                centre: 0.0,
            };
            let short = if full_rows < rows {
                let length = columns - 1;
                let spacing = self
                    .spacing(length, overlap, centred)
                    .min(Self::widest(length, centred));
                let rise = if overlap {
                    self.rise(length, spacing, centred)
                } else {
                    rise
                };
                RowShape {
                    length,
                    spacing,
                    rise,
                    centre: 0.0,
                }
            } else {
                full
            };
            let shape = |row: usize| if row < full_rows { full } else { short };
            if let Some(overlapped) =
                self.trial(items, (rows, false), shape, overlap, placed, projected)
            {
                return Some(overlapped);
            }
        }
        placed.clear();
        None
    }

    /// The shape of an overlapping row of `length` items in `room`, or none if they do not fit.
    fn overlapping_row(&self, length: usize, room: RowRoom) -> Option<RowShape> {
        if length == 0 || room.span < 0.0 {
            return None;
        }
        let spacing = self.spacing(length, true, room);
        let widest = Self::widest(length, room);
        (spacing <= widest).then(|| RowShape {
            length,
            spacing,
            rise: self.rise(length, spacing, room),
            centre: room.centre,
        })
    }

    /// Lays `items` in `rows` rows, the farthest first, each shaped as `shape` gives it by its
    /// index from the farthest, and keeps the result if the rows pass the fan's tests. Returns
    /// whether the items overlap. The farthest row takes the first items, or with `newest_behind`
    /// the last ones, so that the front row holds the first items and the rows behind it the
    /// newest. Either way each row lies behind the rows in front of it.
    fn trial(
        &self,
        items: &[Item],
        (rows, newest_behind): (usize, bool),
        shape: impl Fn(usize) -> RowShape,
        overlap: bool,
        placed: &mut Vec<Placed>,
        projected: &mut Vec<Trial>,
    ) -> Option<bool> {
        let count = items.len();
        let measure = self.target > 0.0 && items.iter().any(|item| item.pressable);
        // Rows laid out of the items' order are put back in that order from the outlines.
        let project = measure || !overlap || newest_behind;
        placed.clear();
        projected.clear();
        if project {
            projected.reserve(count);
        } else {
            placed.reserve(count);
        }
        // The index in `items` of the first item of each row, in the order the rows are laid.
        let first = |row: usize| {
            let before: usize = (0..row).map(|row| shape(row).length).sum();
            if newest_behind {
                count - before - shape(row).length
            } else {
                before
            }
        };
        // The item laid as `layer`: the items are laid the farthest row first, and their layers
        // follow that order.
        let laid = |layer: usize| {
            let (mut row, mut skipped) = (0, 0);
            while layer - skipped >= shape(row).length {
                skipped += shape(row).length;
                row += 1;
            }
            first(row) + layer - skipped
        };
        // The layer of the item at `index`, the inverse of `laid`.
        let layer_of = |index: usize| {
            let (mut row, mut skipped) = (0, 0);
            while !(first(row)..first(row) + shape(row).length).contains(&index) {
                skipped += shape(row).length;
                row += 1;
            }
            skipped + index - first(row)
        };
        let mut overlapped = rows > 1 && shape(0).rise < self.fan.height * 2.0;
        for row in 0..rows {
            let RowShape {
                length,
                spacing,
                rise,
                centre,
            } = shape(row);
            let row_items = &items[first(row)..first(row) + length];
            let above = (rows - 1 - row) as f32 * rise;
            let middle = (length as f32 - 1.0) / 2.0;
            overlapped |= length > 1 && spacing < self.width;
            for (column, item) in row_items.iter().enumerate() {
                let pose =
                    self.screen_pose(item, centre + (column as f32 - middle) * spacing, above);
                if project {
                    let rectangle = self.rectangle(pose, item.size);
                    projected.push(Trial {
                        pose,
                        rectangle,
                        bounds: rectangle.bounds(),
                        press_square: None,
                    });
                } else {
                    placed.push(self.place(item, pose, placed.len(), None));
                }
            }
        }
        if !overlap && !Self::disjoint(projected) {
            return None;
        }
        if measure && !self.witness((0..count).map(|layer| &items[laid(layer)]), projected) {
            return None;
        }
        if project {
            // Construct world poses only after the screen geometry passes the trial, in the
            // order of `items`, which callers read them in.
            placed.extend(items.iter().enumerate().map(|(index, item)| {
                let layer = if newest_behind {
                    layer_of(index)
                } else {
                    index
                };
                let trial = &projected[layer];
                self.place(item, trial.pose, layer, trial.press_square)
            }));
        }
        Some(overlap && overlapped)
    }

    fn spacing(&self, columns: usize, overlap: bool, room: RowRoom) -> f32 {
        if columns < 2 {
            return 0.0;
        }
        let preferred =
            (self.fan.step * self.width).min(self.fan.span * self.width / (columns - 1) as f32);
        if !overlap {
            let widest = Self::widest(columns, room);
            if self.width > widest {
                return self.width;
            }
            let mut spacing = preferred.max(self.width);
            // The arc turns each card about its top edge. Two neighbours can lean inwards
            // by up to a full card height each. Try the room's limit if it is narrower.
            for _ in 0..4 {
                let lean = self
                    .lean(columns, spacing, room)
                    .min(std::f32::consts::FRAC_PI_2);
                spacing =
                    preferred.max(self.width + 4.0 * self.fan.height * lean.sin() + self.margin);
                if spacing >= widest {
                    return widest;
                }
            }
            return spacing;
        }
        if self.strip <= 0.0 {
            return preferred;
        }
        let mut spacing = preferred.max(self.strip);
        // A square needs a wider strip when the adjoining edge leans. Recompute the
        // lean at the new spacing, then check the actual projection in `witness`.
        for _ in 0..4 {
            let next = preferred
                .max(self.strip * (1.0 + self.slant(columns, spacing, room)) + self.margin);
            if next.to_bits() == spacing.to_bits() {
                break;
            }
            spacing = next;
        }
        spacing
    }

    fn widest(columns: usize, room: RowRoom) -> f32 {
        room.span.max(0.0) / columns.saturating_sub(1).max(1) as f32
    }

    fn radius(&self) -> f32 {
        (self.fan.radius * self.width).max(f32::EPSILON)
    }

    /// The largest turn of an item in a row, at its end farthest from the anchor.
    fn lean(&self, columns: usize, spacing: f32, room: RowRoom) -> f32 {
        let edge = room.centre.abs() + (columns as f32 - 1.0) * spacing / 2.0;
        (edge / self.radius()).min(1.0).asin() + self.fan.bias.abs()
    }

    fn slant(&self, columns: usize, spacing: f32, room: RowRoom) -> f32 {
        self.lean(columns, spacing, room)
            .min(80.0_f32.to_radians())
            .tan()
    }

    fn rise(&self, columns: usize, spacing: f32, room: RowRoom) -> f32 {
        self.strip + 2.0 * spacing * self.slant(columns, spacing, room) + self.margin
    }

    fn separated_rise(&self, columns: usize, spacing: f32) -> f32 {
        let radius = self.radius();
        let edge = ((columns as f32 - 1.0) * spacing / 2.0).min(radius);
        let sink = radius - (radius * radius - edge * edge).max(0.0).sqrt();
        let lean = self
            .lean(columns, spacing, self.centred())
            .min(std::f32::consts::FRAC_PI_2);
        // Leave room for the whole face, its tilted corners and the drop at the row's ends.
        2.0 * self.fan.height + self.width * lean.sin() + sink + self.margin
    }

    fn screen_pose(&self, item: &Item, x: f32, rise: f32) -> ScreenPose {
        let radius = self.radius();
        let along = x.clamp(-radius, radius);
        let sink = radius - (radius * radius - along * along).max(0.0).sqrt();
        let (up, turn) = if self.fan.inverted {
            (-1.0, std::f32::consts::PI)
        } else {
            (1.0, 0.0)
        };
        let angle = -up * ((along / radius).asin() + self.fan.bias);
        let height = self.fan.height * item.size.y / self.height.max(f32::EPSILON);
        let rotation = Vec2::from_angle(angle);
        let centre = Vec2::new(x, self.ndc.y - up * (sink - rise))
            + rotation.rotate(Vec2::new(0.0, -up * height));
        let window = self.camera.window.size;
        let ndc = Vec2::new(
            centre.x / (window.width / window.height) + self.ndc.x,
            centre.y,
        );
        ScreenPose {
            ndc,
            height,
            angle: angle + turn,
            axis: rotation * Vec2::new(up, -up),
        }
    }

    fn rectangle(&self, pose: ScreenPose, size: Vec2) -> Rectangle {
        // The fan lies in the camera's plane, so its outline is independent of layer depth.
        let window = self.camera.window.size;
        Rectangle {
            centre: Vec2::new(
                f32::midpoint(pose.ndc.x, 1.0) * window.width,
                f32::midpoint(1.0, -pose.ndc.y) * window.height,
            ),
            half: size * (pose.height * window.height / (2.0 * size.y)),
            axis: pose.axis,
        }
    }

    fn place(
        &self,
        item: &Item,
        screen: ScreenPose,
        layer: usize,
        press_square: Option<[Vec2; 4]>,
    ) -> Placed {
        // Preserve the normal plane spacing, then approach a quarter of the anchor depth so an
        // unusually deep fan still keeps every layer in front of the previous one.
        let step = self.fan.depth_step * layer as f32;
        let straight = self.depth / 2.0;
        let excess = (step - straight).max(0.0);
        let depth = self.depth - step.min(straight) - excess / (1.0 + excess / (straight / 2.0));
        let mut pose = self.camera.screen_pose(
            screen.ndc,
            depth,
            Quat::from_rotation_z(screen.angle),
            screen.height,
        );
        pose.scale *= self.camera.data.card.height / item.size.y;
        Placed {
            key: item.key,
            pose,
            in_view: true,
            count: 1,
            layer,
            press_square,
        }
    }

    fn witness<'i>(
        &self,
        items: impl IntoIterator<Item = &'i Item>,
        mut projected: &mut [Trial],
    ) -> bool {
        for item in items {
            let (trial, later) = projected
                .split_first_mut()
                .expect("each item has a projected outline");
            if item.pressable {
                let over = later
                    .iter()
                    .filter(|other| other.bounds.overlaps(trial.bounds))
                    .map(|other| other.rectangle);
                trial.press_square =
                    PressArea::in_rectangle(trial.rectangle, trial.bounds, &self.safe, self.target)
                        .witness_rectangles(over);
                if trial.press_square.is_none() {
                    return false;
                }
            }
            projected = later;
        }
        true
    }

    fn disjoint(projected: &[Trial]) -> bool {
        projected.iter().enumerate().all(|(index, trial)| {
            projected[..index].iter().all(|other| {
                !trial.bounds.overlaps(other.bounds) || !trial.rectangle.overlaps(other.rectangle)
            })
        })
    }
}
