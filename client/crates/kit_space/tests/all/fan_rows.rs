//! The rows of a centred fan too long for one row, with and without a left limit for its back
//! rows.

use glam::{Quat, Vec2};
use kit_space::arrange::{
    CentredFan, Container, Fan, Limits, Placed, Policy, Region, Solved, arrange,
};
use kit_space::camera::Camera;

use super::arrange::{profiles, projected_face, rectangles, verify};

/// The phone's hand fan at 393 by 759, with `back_left` in points from the window's left edge.
fn hand(camera: &Camera, target: f32, back_left: Option<f32>) -> Container {
    let window = camera.window.size;
    let anchor = camera.screen_pose(Vec2::new(0.0, -0.62), 300.0, Quat::IDENTITY, 0.5);
    let units = anchor.scale * crate::card().y;
    Container {
        region: Region {
            centre: anchor.position,
            size: units * Vec2::new(2.0 * window.width / window.height, 0.55),
            rotation: anchor.rotation,
        },
        policy: Policy::Fan(Fan::Centred(CentredFan {
            height: f32::min(
                100.0 / (crate::card().x / crate::card().y),
                window.height * 0.28,
            ) / window.height,
            radius: 14.0,
            step: 0.95,
            span: 3.35,
            bias: 0.6_f32.to_radians(),
            inverted: false,
            depth_step: 1.5,
            back_left: back_left.map(|left| (left - window.width / 2.0) * 2.0 / window.height),
        })),
        limits: Limits {
            press_points: target,
            overlap: true,
            scroll: true,
            ..Limits::default()
        },
    }
}

/// The visible items in rows, in the order of the items. A row runs left to right, so each
/// step left starts the next row.
fn rows<'a>(solved: &'a Solved, camera: &Camera) -> Vec<Vec<&'a Placed>> {
    let mut rows: Vec<Vec<&Placed>> = Vec::new();
    let mut last = f32::INFINITY;
    for placed in solved.items.iter().filter(|placed| placed.in_view) {
        let x = camera.to_screen(placed.pose.position).x;
        if x < last {
            rows.push(Vec::new());
        }
        rows.last_mut().unwrap().push(placed);
        last = x;
    }
    rows
}

#[test]
fn a_fans_back_rows_stay_right_of_its_left_limit() {
    let (camera, target) = profiles()[1];
    let size = Vec2::new(crate::card().x, crate::card().y);
    let limit = camera.window.size.width / 2.0 - 28.0;
    for count in [6, 7, 8, 9, 14] {
        let items = rectangles(count, size);
        let solved = arrange(&hand(&camera, target, Some(limit)), &items, &camera, 0);
        verify(&items, &solved, &camera, target);
        let rows = rows(&solved, &camera);
        assert!(rows.len() > 1, "{count} cards take more than one row");
        // The front row holds the first cards, and the rows behind it the newest.
        let front = camera.to_screen(rows[0][0].pose.position).y;
        assert!(
            rows[1..]
                .iter()
                .flatten()
                .all(|placed| camera.to_screen(placed.pose.position).y < front),
            "{count} cards: the first row is in front"
        );
        for placed in rows[1..].iter().flatten() {
            let face = projected_face(&items[0], placed, &camera);
            let left = face
                .iter()
                .fold(f32::INFINITY, |left, corner| left.min(corner.x));
            assert!(
                left >= limit - 0.01,
                "{count} cards: back row card {} reaches {left}, left of {limit}",
                placed.key
            );
        }
    }
}

#[test]
fn a_fan_without_a_limit_centres_every_row_on_its_anchor() {
    let (camera, target) = profiles()[1];
    let size = Vec2::new(crate::card().x, crate::card().y);
    let middle = camera.window.size.width / 2.0;
    for count in [6, 8, 9] {
        let items = rectangles(count, size);
        let solved = arrange(&hand(&camera, target, None), &items, &camera, 0);
        verify(&items, &solved, &camera, target);
        for row in rows(&solved, &camera) {
            let ends = [row[0], row[row.len() - 1]]
                .map(|placed| (camera.to_screen(placed.pose.position).x - middle).abs());
            assert!((ends[0] - ends[1]).abs() < 2.0, "{count} cards: {ends:?}");
        }
    }
}
