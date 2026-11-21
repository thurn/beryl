//! Lifted groups fan in one row. Larger groups wrap into a grid of individually exposed cards.

use std::ops::Range;

use glam::{Quat, Vec2, Vec3};
use kit_base::{geometry::Rect, pose::Pose};

use super::press::Bounds;
use super::{Camera, Grid};

/// The plane and anchor of a lifted group. The grid keeps the same facing as the fan.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiftedPlane {
    /// Face the camera. The anchor's x is measured in window heights from the middle,
    /// its y is in normalized device coordinates, and height is a fraction of the window.
    Facing { at: Vec2, depth: f32, height: f32 },
    /// Lie flat at the anchor's table height. The direction is either right or left.
    /// A wrapped group stays clear of the card it was lifted beside, when supplied.
    Table {
        anchor: Pose,
        direction: f32,
        depth_step: f32,
        beside: Option<Pose>,
    },
}

/// A centred fan with a preferred number of columns. Beyond one row, the grid fits
/// the group's cards within the given screen band, clear of the safe insets.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiftedFan {
    pub columns: usize,
    /// Centre spacing as a fraction of a card's width.
    pub step: f32,
    /// Drop in normalized device coordinates and lean in radians per column from the middle.
    pub drop: f32,
    pub lean: f32,
}

impl LiftedFan {
    /// Resting poses in input order. The band is a range of y in normalized device coordinates.
    pub fn poses(
        self,
        camera: &Camera,
        count: usize,
        plane: LiftedPlane,
        band: Range<f32>,
    ) -> impl Iterator<Item = Pose> + '_ {
        let columns = self.columns.max(1);
        let grid = (count > columns).then(|| {
            let anchor = self.fan_pose(camera, plane, 0.0, 0.0);
            let room = band_rect(camera, plane, band);
            let (grid, region) = fit_grid(camera, anchor, room, count, columns);
            grid.boxes(count, region, true)
                .map(move |cell| grid_pose(camera, plane, anchor, cell))
        });
        let fan_count = if grid.is_some() { 0 } else { count };
        let middle = (fan_count as f32 - 1.0) / 2.0;
        let fan = (0..fan_count).map(move |column| {
            self.fan_pose(camera, plane, column as f32 - middle, column as f32 + 1.0)
        });
        grid.into_iter().flatten().chain(fan)
    }

    fn fan_pose(self, camera: &Camera, plane: LiftedPlane, offset: f32, order: f32) -> Pose {
        match plane {
            LiftedPlane::Facing { at, depth, height } => {
                let aspect = camera.window.size.width / camera.window.size.height;
                let card = camera.data.card;
                let width = height * 2.0 * (card.width / card.height) / aspect;
                let ndc = Vec2::new(
                    at.x / aspect + offset * self.step * width,
                    at.y - offset.abs() * self.drop,
                );
                camera.screen_pose(
                    ndc,
                    depth - order,
                    Quat::from_rotation_z(-offset * self.lean),
                    height,
                )
            }
            LiftedPlane::Table {
                anchor,
                direction,
                depth_step,
                ..
            } => Pose {
                position: anchor.position
                    + Vec3::new(
                        offset * direction * self.step * camera.data.card.width * anchor.scale,
                        0.0,
                        depth_step * order,
                    ),
                ..anchor
            },
        }
    }
}

fn band_rect(camera: &Camera, plane: LiftedPlane, band: Range<f32>) -> Rect {
    let window = camera.window;
    let size = window.size;
    let inset = window.safe_area;
    let top = ((1.0 - band.end) * size.height / 2.0).max(inset.top);
    let bottom = ((1.0 - band.start) * size.height / 2.0).min(size.height - inset.bottom);
    let mut room = Rect {
        x: inset.left,
        y: top,
        width: size.width - inset.left - inset.right,
        height: bottom - top,
    };
    if let LiftedPlane::Table {
        beside: Some(source),
        ..
    } = plane
    {
        let bounds = Bounds::of(camera.card_box(&source).0);
        let limit = room.x + room.width;
        let to_left = bounds.min.x.clamp(room.x, limit) - room.x;
        let right_edge = bounds.max.x.clamp(room.x, limit);
        let to_right = limit - right_edge;
        if to_left > to_right {
            room.width = to_left;
        } else if to_right > 0.0 {
            room.x = right_edge;
            room.width = to_right;
        }
    }
    room
}

/// Fit the largest non-overlapping grid, then keep its centre as close to the fan's
/// anchor as the band permits.
fn fit_grid(
    camera: &Camera,
    anchor: Pose,
    room: Rect,
    count: usize,
    columns: usize,
) -> (Grid, Rect) {
    let bounds = Bounds::of(camera.card_box(&anchor).0);
    let size = bounds.max - bounds.min;
    let aspect = size.y / size.x;
    let available = Vec2::new(room.width, room.height);
    let gap = 4.0_f32.min(available.min_element().max(0.0) / count as f32);
    let grid = (1..=columns)
        .map(|columns| {
            let cells = Vec2::new(columns as f32, count.div_ceil(columns) as f32);
            let width = Grid::fitting_width(available, cells, Vec2::splat(gap), aspect);
            Grid::new(columns, Vec2::new(width, width * aspect), Vec2::splat(gap))
        })
        .max_by(|a, b| a.item.x.total_cmp(&b.item.x))
        .expect("a lifted group has at least one column");
    let grid = Grid::new(grid.columns, grid.item.min(size), Vec2::splat(gap));
    let extent = grid.size(count);
    let edge = Vec2::new(room.x, room.y);
    let limit = (edge + available - extent).max(edge);
    let corner = (camera.to_screen(anchor.position) - extent / 2.0).clamp(edge, limit);
    (
        grid,
        Rect {
            x: corner.x,
            y: corner.y,
            width: extent.x,
            height: extent.y,
        },
    )
}

fn grid_pose(camera: &Camera, plane: LiftedPlane, mut anchor: Pose, cell: Rect) -> Pose {
    let start = Vec2::new(cell.x, cell.y);
    let size = Vec2::new(cell.width, cell.height);
    let centre = start + size / 2.0;
    match plane {
        LiftedPlane::Facing { height, .. } => {
            let delta = centre - camera.to_screen(anchor.position);
            camera.move_in_screen(&mut anchor, delta);
            anchor.scale *= cell.height / (height * camera.window.size.height);
        }
        LiftedPlane::Table { .. } => {
            // Perspective changes the projected size and shape across the table. Fit the
            // four projected corners at each cell, while the card stays flat at its anchor height.
            anchor.position = camera.on_plane(centre, anchor.position.z);
            let fits = |scale| {
                let bounds = Bounds::of(camera.card_box(&Pose { scale, ..anchor }).0);
                bounds.min.cmpge(start).all() && bounds.max.cmple(start + size).all()
            };
            if !fits(anchor.scale) {
                let (mut low, mut high) = (0.0, anchor.scale);
                for _ in 0..12 {
                    let scale = f32::midpoint(low, high);
                    if fits(scale) {
                        low = scale;
                    } else {
                        high = scale;
                    }
                }
                anchor.scale = low;
            }
        }
    }
    anchor
}
