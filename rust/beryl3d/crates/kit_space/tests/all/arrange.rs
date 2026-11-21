use glam::{Quat, Vec2, Vec3};
use kit_base::data::{audit, space::layout::phone};
use kit_base::geometry::{CARD_HEIGHT, CARD_WIDTH, Insets, Rect, Size, Window};
use kit_space::arrange::{
    Alignment, Container, Edge, Group, Item, Limits, Overflow, Policy, Region, Solved, Under,
    arrange,
};
use kit_space::camera::{Camera, CameraData, DESKTOP_CAMERA};

fn row(alignment: Alignment) -> Policy {
    Policy::Row {
        alignment,
        gap: 0.0,
        group_gap: 0.0,
        tuck_step: Vec2::ZERO,
    }
}

fn rectangles(count: usize, size: Vec2) -> Vec<Item> {
    (0..count)
        .map(|key| Item {
            key: key as u64,
            size,
            pressable: true,
            group: None,
            under: None,
        })
        .collect()
}

fn profiles() -> [(Camera, f32); 2] {
    let tall = Window {
        size_class: None,
        size: Size {
            width: 393.0,
            height: 759.0,
        },
        scale: 2.0,
        safe_area: Insets {
            top: 47.0,
            bottom: 34.0,
            ..Insets::default()
        },
    };
    let width = tall.size.width - phone::bands::SIDE * 2.0;
    let hand = (width * phone::bands::HAND_SHARE).min(phone::bands::HAND_WIDTH);
    let hand = (hand * (CARD_HEIGHT / CARD_WIDTH) * phone::bands::HAND_SHOWN).round();
    let top = tall.safe_area.top + phone::bands::OPPONENT_STRIP;
    let bottom = tall.size.height - hand - tall.safe_area.bottom - phone::bands::YOU_STRIP;
    let compact = Camera::fit_inside(
        CameraData {
            pitch_degrees: phone::camera::PITCH_DEGREES,
            fov_degrees: phone::camera::FOV_DEGREES,
            target: phone::camera::TARGET,
            required: phone::camera::REQUIRED,
        },
        tall,
        Rect {
            x: phone::bands::SIDE,
            y: top,
            width,
            height: bottom - top,
        },
    );
    [
        (
            Camera::fit(DESKTOP_CAMERA, Window::default()),
            audit::TARGET_POINTS,
        ),
        (compact, audit::PHONE_TARGET_POINTS),
    ]
}

fn facing(camera: &Camera, size: Vec2) -> Region {
    let [right, up, towards] = camera.axes();
    Region {
        centre: camera.data.target,
        size: size / camera.points_per_millimetre(camera.data.target),
        rotation: Quat::from_mat3(&glam::Mat3::from_cols(right, up, towards)),
    }
}

fn natural(camera: &Camera, points: Vec2) -> Vec2 {
    points / camera.points_per_millimetre(camera.data.target)
}

fn container(camera: &Camera, policy: Policy, limits: Limits, points: Vec2) -> Container {
    Container {
        region: facing(camera, points),
        policy,
        limits,
    }
}

#[test]
fn overflow_uses_the_first_enabled_stage_that_fits() {
    let camera = Camera::fit(DESKTOP_CAMERA, Window::default());
    let mut items = rectangles(4, natural(&camera, Vec2::splat(60.0)));
    let mut container = container(
        &camera,
        row(Alignment::Start),
        Limits {
            pile_identical: true,
            overlap: true,
            least_scale: 0.7,
            scroll: true,
            ..Limits::default()
        },
        Vec2::splat(400.0),
    );
    for item in &mut items[..2] {
        item.group = Some(Group {
            key: 1,
            identical: Some(1),
        });
    }
    let layout = |container: &Container| arrange(container, &items, &camera, 0);
    let width = |points| natural(&camera, Vec2::splat(points)).x;

    assert_eq!(layout(&container).overflow, Overflow::FullSize);
    container.region.size.x = width(200.0);
    let piled = layout(&container);
    assert_eq!(piled.overflow, Overflow::Piled);
    assert_eq!((piled.items[0].count, piled.items[1].count), (2, 0));
    assert!(!piled.items[1].in_view);

    container.region.size.x = width(115.0);
    assert_eq!(layout(&container).overflow, Overflow::Overlapped);
    container.region.size.x = width(95.0);
    let shrunk = layout(&container);
    assert_eq!(shrunk.overflow, Overflow::Scaled);
    assert!(shrunk.items[0].pose.scale < 1.0 && shrunk.items[0].pose.scale >= 0.7);

    container.region.size.x = width(70.0);
    let scrolled = layout(&container);
    assert_eq!(scrolled.overflow, Overflow::Scrolled);
    assert_eq!((scrolled.before, scrolled.after), (0, 1));
    let window = scrolled
        .scroll
        .expect("the solver supplies its group window");
    assert_eq!((window.first, window.shown, window.last), (0, 2, 2));
    let end = arrange(&container, &items, &camera, window.last);
    let window = end.scroll.expect("the solver keeps its window at the end");
    assert_eq!((window.first, window.shown, window.last), (1, 2, 1));
    assert_eq!((end.before, end.after), (2, 0));
    assert_eq!(
        end.items.iter().map(|placed| placed.count).sum::<usize>(),
        items.len()
    );
    assert!(end.items[2..].iter().all(|item| item.in_view));

    container.limits.pile_identical = false;
    container.limits.overlap = false;
    container.limits.least_scale = 1.0;
    container.limits.scroll = false;
    let clipped = arrange(&container, &items, &camera, 2);
    assert_eq!(clipped.overflow, Overflow::Clipped);
    assert_eq!((clipped.before, clipped.after), (0, 3));
    assert_eq!(clipped.items[0].count, 1);
}

#[test]
fn scaling_finds_a_narrow_interval_between_size_and_press_limits() {
    let camera = Camera::fit(DESKTOP_CAMERA, Window::default());
    let items = rectangles(1, natural(&camera, Vec2::splat(40.0)));
    let container = container(
        &camera,
        Policy::Spot,
        Limits {
            least_scale: 0.5,
            scroll: true,
            ..Limits::default()
        },
        Vec2::splat(24.8),
    );
    let result = arrange(&container, &items, &camera, 0);
    assert_eq!(result.overflow, Overflow::Scaled);
    assert!(result.items[0].in_view);
    verify(&items, &result, &camera, 24.0);
}

#[test]
fn cascade_can_step_further_than_one_item_width() {
    let camera = Camera::fit(DESKTOP_CAMERA, Window::default());
    let items = rectangles(2, natural(&camera, Vec2::splat(40.0)));
    let step = natural(&camera, Vec2::splat(80.0)).x;
    let container = container(
        &camera,
        Policy::Cascade {
            direction: Vec2::X,
            step,
        },
        Limits::default(),
        Vec2::new(240.0, 100.0),
    );
    let result = arrange(&container, &items, &camera, 0);
    assert_eq!(result.overflow, Overflow::FullSize);
    let expected = result.items[0].pose.position + container.region.rotation * (Vec3::X * step);
    let actual = camera.to_screen(result.items[1].pose.position);
    assert!(actual.distance(camera.to_screen(expected)) < 0.001);
}

#[test]
fn policies_give_tucks_stacks_columns_and_a_single_spot() {
    let camera = Camera::fit(DESKTOP_CAMERA, Window::default());
    let size = natural(&camera, Vec2::splat(64.0));
    let mut items = rectangles(4, size);
    let limits = Limits {
        scroll: true,
        ..Limits::default()
    };
    let mut container = container(
        &camera,
        Policy::Row {
            alignment: Alignment::Middle,
            gap: 0.0,
            group_gap: 0.0,
            tuck_step: natural(&camera, Vec2::new(34.0, 0.0)),
        },
        limits,
        Vec2::new(500.0, 260.0),
    );
    items[1].under = Some(Under {
        host: 0,
        edge: Edge::Start,
    });
    items[2].under = Some(Under {
        host: 0,
        edge: Edge::End,
    });
    let tucked = arrange(&container, &items, &camera, 0);
    assert_eq!(tucked.overflow, Overflow::FullSize);
    assert!(tucked.items[1].layer < tucked.items[0].layer);
    assert!(tucked.items[2].layer < tucked.items[0].layer);
    assert!(tucked.items[1].pose.position.x < tucked.items[0].pose.position.x);
    assert!(tucked.items[2].pose.position.x > tucked.items[0].pose.position.x);
    verify(&items, &tucked, &camera, limits.press_points);
    let without_tuck = arrange(&container, &[items[0], items[2], items[3]], &camera, 0);
    assert_eq!(tucked.items[0].pose, without_tuck.items[0].pose);

    container.policy = Policy::Pile { offset: Vec2::ONE };
    items[1].pressable = false;
    items[2].pressable = false;
    let pile = arrange(&container, &items, &camera, 0);
    assert_eq!((pile.items[0].count, pile.after), (3, 1));
    assert!(pile.items[0].layer > pile.items[2].layer);

    container.policy = Policy::Cascade {
        direction: Vec2::new(1.0, -0.5),
        step: 36.0,
    };
    let cascade = arrange(&container, &items, &camera, 0);
    assert_eq!(cascade.overflow, Overflow::FullSize);
    assert!(cascade.items[1].pose.position.x > cascade.items[0].pose.position.x);
    assert!(
        camera.to_screen(cascade.items[1].pose.position).y
            > camera.to_screen(cascade.items[0].pose.position).y
    );
    verify(&items, &cascade, &camera, limits.press_points);

    container.policy = Policy::Grid {
        columns: 2,
        gap: Vec2::ONE,
    };
    let grid = arrange(&container, &items, &camera, 0);
    assert_eq!(grid.overflow, Overflow::FullSize);
    assert!(grid.items[0].pose.position.x < grid.items[1].pose.position.x);
    assert!(
        camera.to_screen(grid.items[0].pose.position).y
            < camera.to_screen(grid.items[2].pose.position).y
    );

    container.policy = Policy::Spot;
    let spot = arrange(&container, &items, &camera, 2);
    assert_eq!((spot.before, spot.after), (2, 1));
    assert_eq!(spot.items[2].pose.position, container.region.centre);
}

#[test]
fn seeded_rows_and_fans_leave_each_pressable_rectangle_a_projected_square() {
    let mut seed = Seed(0x949a_5660);
    for (camera, target) in profiles() {
        let mut witnessed = [0; 2];
        for count in (0..=300).step_by(3).chain([1, 2, 5, 7]) {
            let mut items = rectangles(count, Vec2::ZERO);
            for item in &mut items {
                item.size = natural(
                    &camera,
                    Vec2::new(seed.between(40.0, 114.0), seed.between(52.0, 140.0)),
                );
                item.pressable = !seed.next().is_multiple_of(6);
            }
            let alignments = [Alignment::Start, Alignment::Middle, Alignment::End];
            let alignment = alignments[seed.next() as usize % alignments.len()];
            let points = Vec2::new(
                seed.between(115.0, camera.window.size.width * 0.94),
                seed.between(110.0, 405.0),
            );
            let limits = Limits {
                press_points: target,
                overlap: true,
                least_scale: seed.between(0.68, 1.0),
                scroll: true,
                ..Limits::default()
            };
            for (index, policy) in [
                row(alignment),
                Policy::Fan {
                    alignment,
                    radius: natural(&camera, Vec2::splat(720.0)).x,
                    max_lean: 0.48,
                    gap: 0.0,
                },
            ]
            .into_iter()
            .enumerate()
            {
                let mut container = container(&camera, policy, limits, points);
                container.region.rotation *= Quat::from_rotation_x(seed.between(-0.42, 0.34));
                let scroll = seed.next() as usize % (count + 1);
                let result = arrange(&container, &items, &camera, scroll);
                assert_eq!(result, arrange(&container, &items, &camera, scroll));
                verify(&items, &result, &camera, target);
                witnessed[index] += result
                    .items
                    .iter()
                    .filter(|placed| placed.press_square.is_some())
                    .count();
                let visible = result.items.iter().filter(|placed| placed.in_view).count();
                assert_eq!(visible + result.before + result.after, items.len());
            }
        }
        assert!(
            witnessed.into_iter().all(|count| count > 100),
            "{witnessed:?}"
        );
    }
}

#[test]
fn seeded_full_size_layouts_keep_the_prefix_before_a_removal() {
    let mut seed = Seed(0x31bd_14e2);
    for (camera, target) in profiles() {
        for alignment in [Alignment::Start, Alignment::Middle, Alignment::End] {
            for policy in [
                row(alignment),
                Policy::Fan {
                    alignment,
                    radius: natural(&camera, Vec2::splat(3000.0)).x,
                    max_lean: 0.35,
                    gap: 0.0,
                },
            ] {
                let container = container(
                    &camera,
                    policy,
                    Limits {
                        press_points: target,
                        ..Limits::default()
                    },
                    Vec2::new(camera.window.size.width * 0.9, 340.0),
                );
                for _ in 0..32 {
                    let mut items = rectangles(3, Vec2::ZERO);
                    for item in &mut items {
                        item.size = natural(
                            &camera,
                            Vec2::new(seed.between(55.0, 64.0), seed.between(63.0, 80.0)),
                        );
                    }
                    let before = arrange(&container, &items, &camera, 0);
                    assert_eq!(
                        before.overflow,
                        Overflow::FullSize,
                        "{policy:?} in {:?}",
                        camera.window.size
                    );
                    let removed = 1 + seed.next() as usize % 2;
                    items.remove(removed);
                    let after = arrange(&container, &items, &camera, 0);
                    assert_eq!(after.overflow, Overflow::FullSize);
                    for (before, after) in before.items.iter().zip(&after.items).take(removed) {
                        assert_eq!(before.pose, after.pose);
                    }
                }
            }
        }
    }
}

#[test]
fn overlapping_rows_keep_the_prefix_before_a_removal() {
    let mut seed = Seed(0x5144_b8f1);
    for (camera, target) in profiles() {
        let container = container(
            &camera,
            row(Alignment::Start),
            Limits {
                press_points: target,
                overlap: true,
                ..Limits::default()
            },
            Vec2::new(286.0, 180.0),
        );
        for _ in 0..32 {
            let mut items = rectangles(5, Vec2::ZERO);
            for item in &mut items {
                item.size = natural(
                    &camera,
                    Vec2::new(seed.between(79.0, 85.0), seed.between(68.0, 87.0)),
                );
            }
            let before = arrange(&container, &items, &camera, 0);
            assert_eq!(before.overflow, Overflow::Overlapped);
            let removed = 1 + seed.next() as usize % 4;
            items.remove(removed);
            let after = arrange(&container, &items, &camera, 0);
            assert_eq!(after.overflow, Overflow::Overlapped);
            for (before, after) in before.items.iter().zip(&after.items).take(removed) {
                assert_eq!(before.pose, after.pose);
            }
        }
    }
}

fn verify(items: &[Item], result: &Solved, camera: &Camera, target: f32) {
    assert_eq!(items.len(), result.items.len());
    let visible: Vec<_> = items
        .iter()
        .zip(&result.items)
        .filter(|(_, place)| place.in_view)
        .map(|(item, place)| {
            let half = item.size / 2.0;
            let quad = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)].map(|(x, y)| {
                camera.to_screen(place.pose.transform().transform_point3(Vec3::new(
                    x * half.x,
                    y * half.y,
                    0.0,
                )))
            });
            (item, place, quad)
        })
        .collect();
    for &(item, placed, quad) in &visible {
        assert_eq!(item.key, placed.key);
        let Some(square) = placed.press_square else {
            assert!(!item.pressable);
            continue;
        };
        let target_edge = square[1] - square[0];
        let projected_edge = (quad[1] - quad[0]) + (quad[2] - quad[3]);
        assert!(target_edge.normalize().dot(projected_edge.normalize()) > 0.9999);
        for i in 0..4 {
            let side = square[(i + 1) % 4] - square[i];
            let next = square[(i + 2) % 4] - square[(i + 1) % 4];
            assert!((side.length() - target).abs() < 0.001);
            assert!(side.normalize().dot(next.normalize()).abs() < 0.0001);
            let point = square[i];
            let inset = camera.window.safe_area;
            assert!(point.x >= inset.left && point.x <= camera.window.size.width - inset.right);
            assert!(point.y >= inset.top && point.y <= camera.window.size.height - inset.bottom);
            assert!(inside(quad, point), "{point:?} outside {quad:?}");
        }
        for (_, blocker, other) in &visible {
            if blocker.layer > placed.layer {
                assert!(
                    separated(square, *other),
                    "item {} occluded by {}",
                    placed.key,
                    blocker.key
                );
            }
        }
    }
}

fn inside(quad: [Vec2; 4], point: Vec2) -> bool {
    let signs = std::array::from_fn::<_, 4, _>(|i| {
        let axis = (quad[(i + 1) % 4] - quad[i]).normalize();
        axis.perp_dot(point - quad[i])
    });
    signs.iter().all(|&distance| distance >= -0.001)
        || signs.iter().all(|&distance| distance <= 0.001)
}

fn separated(first: [Vec2; 4], second: [Vec2; 4]) -> bool {
    [first, second].into_iter().any(|quad| {
        (0..4).any(|i| {
            let axis = (quad[(i + 1) % 4] - quad[i]).perp().normalize();
            let bounds = |points: [Vec2; 4]| {
                points
                    .into_iter()
                    .map(|point| axis.dot(point))
                    .fold((f32::INFINITY, f32::NEG_INFINITY), |(low, high), at| {
                        (low.min(at), high.max(at))
                    })
            };
            let (a, b) = bounds(first);
            let (c, d) = bounds(second);
            b <= c + 0.001 || d <= a + 0.001
        })
    })
}

struct Seed(u32);

impl Seed {
    fn next(&mut self) -> u32 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        self.0 >> 8
    }

    fn between(&mut self, low: f32, high: f32) -> f32 {
        low + (high - low) * (self.next() as f32 / ((1 << 24) as f32))
    }
}
