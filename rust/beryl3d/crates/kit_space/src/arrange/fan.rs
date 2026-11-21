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
    if count > 0 {
        // All items in a hand have the same footprint. The capacity bound avoids retrying
        // arbitrarily long lists when the region has room for only a few dozen rectangles.
        for visible in (1..=count.min(fit.capacity())).rev() {
            let start = if container.limits.scroll {
                scroll.min(count - visible)
            } else {
                0
            };
            if let Some(overlapped) = fit.rows(
                &items[start..start + visible],
                container.limits.overlap,
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
        overflow = if container.limits.scroll {
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
    fn capacity(&self) -> usize {
        if self.strip <= 0.0 {
            return usize::MAX;
        }
        let step = self.strip + self.margin;
        let columns = ((self.room.x - self.width) / step).max(0.0) as usize + 1;
        let rows = (self.room.y.max(0.0) / step) as usize + 1;
        columns.saturating_mul(rows)
    }

    fn rows(
        &self,
        items: &[Item],
        overlap: bool,
        placed: &mut Vec<Placed>,
        projected: &mut Vec<Trial>,
    ) -> Option<bool> {
        let count = items.len();
        let measure = self.target > 0.0 && items.iter().any(|item| item.pressable);
        let max_rows = if self.strip > 0.0 && !self.fan.inverted {
            count
        } else {
            1
        };
        for rows in 1..=max_rows {
            // Even with no tilt, each back row needs the press strip and the margin.
            if rows > 1 && (rows - 1) as f32 * (self.strip + self.margin) > self.room.y {
                break;
            }
            let columns = count.div_ceil(rows);
            let spacing = self.spacing(columns, overlap);
            let widest = self.widest(columns);
            if self.strip > 0.0 && spacing > widest {
                continue;
            }
            let spacing = spacing.min(widest);
            let rise = self.rise(columns, spacing);
            if rows > 1 && (rows - 1) as f32 * rise > self.room.y {
                continue;
            }
            // Rows of the same length use the same spacing and rise.
            let full_rows = (count - 1) % rows + 1;
            let full = (columns, spacing, rise);
            let short = if full_rows < rows {
                let length = columns - 1;
                let spacing = self.spacing(length, overlap).min(self.widest(length));
                (length, spacing, self.rise(length, spacing))
            } else {
                full
            };
            placed.clear();
            projected.clear();
            if measure {
                projected.reserve(count);
            } else {
                placed.reserve(count);
            }
            let mut parts = items;
            let mut overlapped = rows > 1 && rise < self.fan.height * 2.0;
            for row in 0..rows {
                let (length, spacing, rise) = if row < full_rows { full } else { short };
                let (row_items, rest) = parts.split_at(length);
                parts = rest;
                let above = (rows - 1 - row) as f32 * rise;
                let middle = (length as f32 - 1.0) / 2.0;
                overlapped |= length > 1 && spacing < self.width;
                for (column, item) in row_items.iter().enumerate() {
                    let pose = self.screen_pose(item, (column as f32 - middle) * spacing, above);
                    if measure {
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
            if measure {
                if !self.witness(items, projected) {
                    continue;
                }
                // The press test uses screen geometry. Construct world poses only for
                // the trial that leaves every pressable card reachable.
                placed.extend(items.iter().zip(projected.iter()).enumerate().map(
                    |(layer, (item, trial))| {
                        self.place(item, trial.pose, layer, trial.press_square)
                    },
                ));
            }
            return Some(overlapped);
        }
        placed.clear();
        None
    }

    fn spacing(&self, columns: usize, overlap: bool) -> f32 {
        if columns < 2 {
            return 0.0;
        }
        let preferred =
            (self.fan.step * self.width).min(self.fan.span * self.width / (columns - 1) as f32);
        if !overlap {
            return preferred.max(self.width);
        }
        if self.strip <= 0.0 {
            return preferred;
        }
        let mut spacing = preferred.max(self.strip);
        // A square needs a wider strip when the adjoining edge leans. Recompute the
        // lean at the new spacing, then check the actual projection in `witness`.
        for _ in 0..4 {
            let next =
                preferred.max(self.strip * (1.0 + self.slant(columns, spacing)) + self.margin);
            if next.to_bits() == spacing.to_bits() {
                break;
            }
            spacing = next;
        }
        spacing
    }

    fn widest(&self, columns: usize) -> f32 {
        (self.room.x - self.width).max(0.0) / columns.saturating_sub(1).max(1) as f32
    }

    fn slant(&self, columns: usize, spacing: f32) -> f32 {
        let radius = (self.fan.radius * self.width).max(f32::EPSILON);
        let edge = (columns as f32 - 1.0) * spacing / 2.0;
        ((edge / radius).min(1.0).asin() + self.fan.bias.abs())
            .min(80.0_f32.to_radians())
            .tan()
    }

    fn rise(&self, columns: usize, spacing: f32) -> f32 {
        self.strip + 2.0 * spacing * self.slant(columns, spacing) + self.margin
    }

    fn screen_pose(&self, item: &Item, x: f32, rise: f32) -> ScreenPose {
        let radius = (self.fan.radius * self.width).max(f32::EPSILON);
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

    fn witness(&self, items: &[Item], mut projected: &mut [Trial]) -> bool {
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
}
