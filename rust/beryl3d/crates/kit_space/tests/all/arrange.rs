#[path = "../../../../test_profiles.rs"]
mod test_profiles;

use test_profiles::SHIPPED;

use glam::{Quat, Vec2, Vec3};
use kit_base::data::profiles::Profiles;
use kit_base::geometry::{Insets, Rect, Size, Window};
use kit_space::arrange::{
    Alignment, CentredFan, Container, Edge, Fan, Group, Item, Limits, Overflow, Policy, Region,
    Solved, Under, arrange,
};
use kit_space::camera::Camera;

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

fn reference_camera() -> Camera {
    let profiles = Profiles::parse(&SHIPPED, crate::card_data()).unwrap();
    Camera::fit(profiles.fallback().table.camera, Window::default())
}

fn profiles() -> [(Camera, f32); 2] {
    let profiles = Profiles::parse(&SHIPPED, crate::card_data()).unwrap();
    let compact = profiles.for_size(Size {
        width: 393.0,
        height: 759.0,
    });
    let tall = Window {
        scale: 2.0,
        safe_area: Insets {
            top: 47.0,
            ..compact.test_safe_area()
        },
        ..compact.usual_window()
    };
    let bands = compact.table.bands.unwrap();
    let width = tall.size.width - bands.side * 2.0;
    let hand = (width * bands.hand_share).min(bands.hand_width);
    let hand = (hand * (crate::card().y / crate::card().x) * bands.hand_shown).round();
    let top = tall.safe_area.top + bands.opponent_strip;
    let bottom = tall.size.height - hand - tall.safe_area.bottom - bands.you_strip;
    let area = Rect {
        x: bands.side,
        y: top,
        width,
        height: bottom - top,
    };
    let regular = profiles.fallback();
    [
        (
            Camera::fit(regular.table.camera, Window::default()),
            regular.checks.target_points,
        ),
        (
            Camera::fit_inside(compact.table.camera, tall, area),
            compact.checks.target_points,
        ),
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
    let camera = reference_camera();
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
    let camera = reference_camera();
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
    let camera = reference_camera();
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
    let camera = reference_camera();
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
                Policy::Fan(Fan::Fixed {
                    alignment,
                    radius: natural(&camera, Vec2::splat(720.0)).x,
                    max_lean: 0.48,
                    gap: 0.0,
                }),
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
                Policy::Fan(Fan::Fixed {
                    alignment,
                    radius: natural(&camera, Vec2::splat(3000.0)).x,
                    max_lean: 0.35,
                    gap: 0.0,
                }),
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

#[test]
fn a_centred_fan_wraps_before_scrolling_and_keeps_its_press_squares() {
    let items = rectangles(60, Vec2::new(crate::card().x, crate::card().y));
    for ((camera, target), scrolls) in profiles().into_iter().zip([false, true]) {
        let window = camera.window.size;
        let anchor = camera.screen_pose(Vec2::new(0.0, -0.62), 300.0, Quat::IDENTITY, 0.5);
        let units = anchor.scale * crate::card().y;
        let fan = Container {
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
            })),
            limits: Limits {
                press_points: target,
                overlap: true,
                scroll: true,
                ..Limits::default()
            },
        };
        let start = arrange(&fan, &items, &camera, 0);
        verify(&items, &start, &camera, target);
        assert_eq!(start.before, 0);
        assert!(start.items.iter().filter(|item| item.in_view).count() > 6);
        // A middle card in a back row stands higher than either end of the front row.
        let back = camera.to_screen(start.items[2].pose.position);
        let front = camera.to_screen(
            start
                .items
                .iter()
                .rfind(|item| item.in_view)
                .unwrap()
                .pose
                .position,
        );
        assert!(front.y - back.y > target);
        if scrolls {
            assert_eq!(start.overflow, Overflow::Scrolled);
            let end = arrange(&fan, &items, &camera, items.len());
            verify(&items, &end, &camera, target);
            assert_eq!((end.before, end.after), (start.after, 0));
            assert!(
                end.items[..end.before]
                    .iter()
                    .all(|item| !item.in_view && item.pose == end.items[end.before].pose)
            );
        }
    }
}

#[test]
fn a_centred_fan_keeps_its_cards_within_the_available_width() {
    for (camera, target) in profiles() {
        let size = Vec2::new(crate::card().x, crate::card().y);
        let items = rectangles(1, size);
        let mut fan = container(
            &camera,
            Policy::Fan(Fan::Centred(CentredFan {
                height: 2.0 * target / camera.window.size.height * (size.y / size.x),
                radius: 14.0,
                step: 0.95,
                span: 3.35,
                bias: 0.0,
                inverted: false,
                depth_step: 1.5,
            })),
            Limits {
                press_points: target,
                overlap: true,
                scroll: true,
                ..Limits::default()
            },
            Vec2::splat(3.0 * target),
        );
        let empty = arrange(&fan, &[], &camera, usize::MAX);
        assert!(empty.items.is_empty());
        assert_eq!((empty.before, empty.after), (0, 0));
        let single = arrange(&fan, &items, &camera, usize::MAX);
        assert!(single.items[0].in_view);
        verify(&items, &single, &camera, target);

        let many = rectangles(100, size);
        let start = arrange(&fan, &many, &camera, 0);
        let end = arrange(&fan, &many, &camera, usize::MAX);
        assert_eq!(start.overflow, Overflow::Scrolled);
        assert!(start.items[0].in_view);
        assert!(end.items[99].in_view);
        assert_eq!((start.before, end.after), (0, 0));
        assert_eq!(start.after, end.before);
        verify(&many, &start, &camera, target);
        verify(&many, &end, &camera, target);

        // A full-size card is twice the press target. The safe area can hold it,
        // but this narrower region cannot, and the centred policy does not shrink it.
        fan.region.size.x *= 0.5;
        let narrow = arrange(&fan, &items, &camera, 0);
        assert!(!narrow.items[0].in_view);
        assert_eq!((narrow.before, narrow.after), (0, 1));
        assert_eq!(narrow.overflow, Overflow::Clipped);
        assert!(narrow.scroll.is_none());
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

#[test]
fn a_wrapped_cascade_scrolls_when_its_press_squares_fill_the_region() {
    use kit_space::arrange::{AnchoredCascade, Cascade};

    let region = AnchoredCascade {
        foot: Vec2::new(350.0, 0.0),
        ceiling: 160.0,
        left: 0.0,
        size: Vec2::new(90.0, 126.0),
        reach: 100.0,
        pitch: 30.0,
        least_scale: 0.4,
    };
    let cascade = Cascade {
        direction: Vec2::new(-1.0, 1.0),
        step: 35.0,
    };
    let first = cascade.anchored(region, 24.0, (200, 0));
    assert_eq!(first.overflow, Overflow::Scrolled);
    assert!(first.window.shown > 1);
    assert!(first.item(0).1);

    let last = cascade.anchored(region, 24.0, (200, usize::MAX));
    assert_eq!(last.window.first, 200 - first.window.shown);
    assert!(last.item(199).1);
    assert!(!last.item(0).1);
    assert_eq!(last.item(0).0, last.item(last.window.first).0);

    for result in [first, last] {
        let (low, high) = result.bounds().expect("the window shows cards");
        assert!(low.x >= region.left && low.y >= region.foot.y);
        assert!(high.y <= region.ceiling);
        assert!(result.size.min_element() >= 24.0);
        let indices = result.window.first..result.window.first + result.window.shown;
        for (offset, index) in indices.clone().enumerate() {
            let at = result.item(index).0;
            // The upper left square of each face lies outside every face in front of it.
            let corner = at + Vec2::new(-result.size.x, result.size.y) / 2.0;
            for ahead in indices.clone().take(offset) {
                let front = result.item(ahead).0;
                let low = front - result.size / 2.0;
                let high = front + result.size / 2.0;
                assert!(corner.x + 24.0 <= low.x || corner.y - 24.0 >= high.y);
            }
        }
    }
}
