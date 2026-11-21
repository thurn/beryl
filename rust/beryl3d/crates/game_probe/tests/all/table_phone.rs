//! The table on the phone layout (docs/beryl-3d.md, "Space and camera" and "Sizes"): the
//! camera fitted to the battlefield between the strips and the hand, the bands each part of the
//! table stands in, and every card at rest keeping a 44-point square to press.

use beryl_protocol::protocol::{CardType, ObjectId, RowName, Side, ZoneName};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_play::play::Show;
use game_table::profiles::phone::{Band, battlefield};
use game_table::table::{Held, REFERENCE, Table, TrayKind, Window};
use glam::Vec2;
use kit_frame::frame::{Frame, Place, Resolved, Role};
use kit_probe::agent::checks::{on_screen, reachable};
use kit_probe::agent::{Context, Layout};
use kit_ui::input::{Insets, Size};

use crate::common::{before, draw, empty_frame, frame};

/// The three phone windows the interface tests run in.
const PHONES: [(f32, f32); 3] = [(393.0, 759.0), (375.0, 647.0), (430.0, 932.0)];

/// The safe area the phone interface tests give every phone window: a status bar and a home
/// bar.
const SAFE: Insets = Insets {
    top: 44.0,
    right: 0.0,
    bottom: 34.0,
    left: 0.0,
};

const MS: u64 = 1_000_000;

fn phone(width: f32, height: f32) -> Window {
    Window {
        size_class: None,
        size: Size { width, height },
        scale: 3.0,
        safe_area: SAFE,
    }
}

/// A phone window whose safe area is a different distance in from every edge.
fn uneven(width: f32, height: f32) -> Window {
    Window {
        safe_area: Insets {
            top: 47.0,
            right: 20.0,
            bottom: 21.0,
            left: 12.0,
        },
        ..phone(width, height)
    }
}

/// The table, alone, that a show draws for `view` at rest in `window`: no flat layer, whose
/// controls the game screen lays out.
fn frame_in(window: Window, view: &ViewBuilder) -> Frame {
    let mut view = view.clone();
    view.script(ScriptBuilder::reset().build());
    let mut show = Show::new(Table::new(crate::test_data::data(), window));
    show.push(Box::new(view.update()), Vec::new());
    frame(&mut show, &mut Frame::with_capacity(4096, 4096, 65_536), 0)
}

fn context(window: Window) -> Context {
    Context {
        recording: "phone".to_owned(),
        seat: Side::You,
        layout: Layout::Phone,
        audit: crate::test_data::data().audit,
        checks: Layout::Phone.checks(&crate::test_data::data().profiles, window.size),
        size: window.size,
        safe_area: window.safe_area,
        at: 0,
    }
}

/// The faults `check` finds on cards.
fn card_faults(
    frame: &Frame,
    window: Window,
    check: fn(&Frame, &Context) -> Vec<kit_probe::agent::Fault>,
) -> Vec<String> {
    check(frame, &context(window))
        .into_iter()
        .map(|fault| fault.message)
        .filter(|message| message.starts_with("card "))
        .collect()
}

/// The box around an element's corners: left, top, right, bottom.
fn bounds(element: &Resolved) -> [f32; 4] {
    let xs = element.corners.map(|c| c.x);
    let ys = element.corners.map(|c| c.y);
    [
        xs.into_iter().fold(f32::INFINITY, f32::min),
        ys.into_iter().fold(f32::INFINITY, f32::min),
        xs.into_iter().fold(f32::NEG_INFINITY, f32::max),
        ys.into_iter().fold(f32::NEG_INFINITY, f32::max),
    ]
}

fn centre(element: &Resolved) -> Vec2 {
    element.corners.iter().copied().sum::<Vec2>() / 4.0
}

fn cards_at(frame: &Frame, place: Place) -> Vec<&Resolved> {
    (frame.resolved.iter())
        .filter(|element| element.role == Role::Card && element.place == place)
        .filter(|element| element.opacity > 0.0)
        .collect()
}

fn battlefield_place(side: Side, row: RowName) -> Place {
    Place::Card {
        zone: ZoneName::Battlefield,
        side,
        row: Some(row),
    }
}

#[test]
fn the_required_rectangle_lies_inside_the_battlefield_at_every_phone_size() {
    for (width, height) in PHONES {
        for window in [phone(width, height), uneven(width, height)] {
            let phone_layout = layout(window);
            let camera = phone_layout.fit(window);
            let field = battlefield(window);
            assert!(camera.required_fits(), "{window:?}");
            for corner in camera.required_corners() {
                assert!(
                    corner.x >= field.x - 0.01
                        && corner.x <= field.x + field.width + 0.01
                        && corner.y >= field.y - 0.01
                        && corner.y <= field.y + field.height + 0.01,
                    "{window:?}: {corner} lies outside {field:?}"
                );
            }
            let inset = window.safe_area;
            assert!(field.x >= inset.left && field.y >= inset.top);
            assert!(field.x + field.width <= width - inset.right);
            assert!(field.y + field.height <= height - inset.bottom);
        }
    }
}

/// At 393 by 759 the camera puts the required rectangle where the web interface's phone layout
/// has its battlefield: from the foot of the opponent's strip (44 + 48) to the head of the
/// viewer's (759 - 121 - 60), inside the 8-point side gutters, each edge within 4 points.
#[test]
fn the_camera_fills_the_web_battlefield_at_393_by_759() {
    let window = phone(393.0, 759.0);
    let camera = layout(window).fit(window);
    let corners = camera.required_corners();
    let measured = [
        corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min),
        corners.iter().map(|c| c.y).fold(f32::INFINITY, f32::min),
        corners
            .iter()
            .map(|c| c.x)
            .fold(f32::NEG_INFINITY, f32::max),
        corners
            .iter()
            .map(|c| c.y)
            .fold(f32::NEG_INFINITY, f32::max),
    ];
    let target = [8.0, 92.0, 385.0, 578.0];
    let field = battlefield(window);
    let edges = [
        field.x,
        field.y,
        field.x + field.width,
        field.y + field.height,
    ];
    assert!(
        edges
            .iter()
            .zip(target)
            .all(|(edge, target)| (edge - target).abs() < 0.01),
        "the battlefield is the web's: {edges:?}"
    );
    for (edge, (measured, target)) in ["left", "top", "right", "bottom"]
        .into_iter()
        .zip(measured.into_iter().zip(target))
    {
        assert!(
            (measured - target).abs() <= 4.0,
            "{edge}: measured {measured}, target {target} ({measured:?})"
        );
    }
}

#[test]
fn the_bands_follow_one_another_without_overlapping() {
    for (width, height) in PHONES {
        for window in [phone(width, height), uneven(width, height)] {
            let bands = phone_bands(window).expect("a phone window");
            let order: [Band; 8] = [
                bands.opponent_strip,
                bands.opponent_back,
                bands.opponent_creatures,
                bands.middle,
                bands.your_creatures,
                bands.your_back,
                bands.your_strip,
                bands.hand,
            ];
            assert!((order[0].top - window.safe_area.top).abs() < 0.01);
            assert!((order[7].bottom() - height).abs() < 0.01);
            for pair in order.windows(2) {
                assert!(
                    (pair[0].bottom() - pair[1].top).abs() < 0.01,
                    "{window:?}: {pair:?}"
                );
            }
            for band in order {
                assert!(band.height > 0.0, "{window:?}: {band:?}");
            }
            let middle = layout(window)
                .centred_stack_line()
                .expect("the phone stack");
            assert!((middle - bands.middle.middle()).abs() < 0.01, "{window:?}");
        }
    }
    assert_eq!(phone_bands(Window::default()), None);
}

/// A table with both sides' rows, every card tapped, so that each row takes its tapped
/// footprint.
fn tapped_board() -> ViewBuilder {
    let mut view = ViewBuilder::new();
    for side in [Side::You, Side::Opponent] {
        for (name, types) in [
            ("Plains", vec![CardType::Land]),
            ("Island", vec![CardType::Land]),
            ("Serra Angel", vec![CardType::Creature]),
            ("Wall of Omens", vec![CardType::Creature]),
            ("Portable Hole", vec![CardType::Artifact]),
        ] {
            let card = view.card(ZoneName::Battlefield, side, name);
            card.types = types;
            card.tapped = true;
        }
    }
    view
}

#[test]
fn each_row_stands_in_its_band_and_no_row_covers_another() {
    for (width, height) in PHONES {
        let window = phone(width, height);
        let bands = phone_bands(window).expect("a phone window");
        let frame = frame_in(window, &tapped_board());
        let rows = [
            (Side::Opponent, RowName::Lands, bands.opponent_back),
            (Side::Opponent, RowName::Others, bands.opponent_back),
            (Side::Opponent, RowName::Creatures, bands.opponent_creatures),
            (Side::You, RowName::Creatures, bands.your_creatures),
            (Side::You, RowName::Others, bands.your_back),
            (Side::You, RowName::Lands, bands.your_back),
        ];
        let mut boxes = Vec::new();
        for (side, row, band) in rows {
            let cards = cards_at(&frame, battlefield_place(side, row));
            assert!(!cards.is_empty(), "{width}x{height} {side:?} {row:?}");
            for card in cards {
                let [_, top, _, bottom] = bounds(card);
                assert!(
                    top >= band.top - 0.5 && bottom <= band.bottom() + 0.5,
                    "{width}x{height} {side:?} {row:?}: {top}..{bottom} leaves {band:?}"
                );
                boxes.push(((side, row), bounds(card)));
            }
        }
        for (i, (a_row, a)) in boxes.iter().enumerate() {
            for (b_row, b) in &boxes[i + 1..] {
                let apart = a[2] <= b[0] || b[2] <= a[0] || a[3] <= b[1] || b[3] <= a[1];
                assert!(
                    apart || a_row == b_row,
                    "{width}x{height}: {a_row:?} {a:?} overlaps {b_row:?} {b:?}"
                );
            }
        }
        assert_eq!(card_faults(&frame, window, reachable), Vec::<String>::new());
        assert_eq!(card_faults(&frame, window, on_screen), Vec::<String>::new());
    }
}

#[test]
fn crowded_rows_scroll_and_every_card_in_view_can_be_pressed() {
    let mut view = ViewBuilder::new();
    for index in 0..14 {
        view.card(ZoneName::Battlefield, Side::You, &format!("Land {index}"))
            .types = vec![CardType::Land];
        view.card(ZoneName::Battlefield, Side::You, &format!("Bear {index}"))
            .types = vec![CardType::Creature];
        view.card(
            ZoneName::Battlefield,
            Side::Opponent,
            &format!("Rock {index}"),
        )
        .types = vec![CardType::Artifact];
    }
    for (width, height) in PHONES {
        let window = phone(width, height);
        let frame = frame_in(window, &view);
        for (side, row) in [
            (Side::You, RowName::Lands),
            (Side::You, RowName::Creatures),
            (Side::Opponent, RowName::Others),
        ] {
            let shown = cards_at(&frame, battlefield_place(side, row));
            assert!(
                shown.len() > 1 && shown.len() < 14,
                "{width}x{height} {side:?} {row:?} shows {} of 14 and scrolls",
                shown.len()
            );
        }
        assert_eq!(card_faults(&frame, window, reachable), Vec::<String>::new());
        assert_eq!(card_faults(&frame, window, on_screen), Vec::<String>::new());
    }
}

#[test]
fn the_hand_runs_off_the_bottom_with_every_top_edge_in_reach() {
    for count in [1, 5, 9, 14] {
        let mut view = ViewBuilder::new();
        for index in 0..count {
            view.card(ZoneName::Hand, Side::You, &format!("Card {index}"));
        }
        for (width, height) in PHONES {
            let window = phone(width, height);
            let bands = phone_bands(window).expect("a phone window");
            let frame = frame_in(window, &view);
            let hand = Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                row: None,
            };
            let cards = cards_at(&frame, hand);
            let label = format!("{count} cards at {width}x{height}");
            // A hand of up to nine cards fits under the hand's ceiling in every phone window; a
            // larger one shows as many as fit, at least nine, and scrolls.
            assert!(
                cards.len() == count || (count > 9 && (9..count).contains(&cards.len())),
                "{label}: {} shown",
                cards.len()
            );
            // The front row runs off the window's bottom, and every card's top stays clear of
            // the top safe inset.
            assert!(
                cards.iter().any(|card| bounds(card)[3] > height),
                "{label}: the hand is partly offscreen"
            );
            for card in &cards {
                let [left, top, right, _] = bounds(card);
                assert!(top >= window.safe_area.top, "{label}: {top}");
                assert!(
                    left >= -1.0 && right <= width + 1.0,
                    "{label}: {left}..{right}"
                );
            }
            if count <= 5 {
                for card in &cards {
                    assert!(
                        bounds(card)[1] >= bands.hand.top,
                        "{label}: one row stands in the hand band"
                    );
                }
            }
            assert_eq!(
                card_faults(&frame, window, reachable),
                Vec::<String>::new(),
                "{label}"
            );
        }
    }
}

/// A hand of several rows rises toward the table, and stops where the viewer's lands and other
/// permanents, tapped, still keep a square to press above it: the cards past what fits under
/// that height scroll instead of covering the back row, and then the creatures.
#[test]
fn a_hand_of_several_rows_leaves_every_back_row_card_reachable() {
    for count in [9, 14, 30] {
        let mut view = ViewBuilder::new();
        for index in 0..count {
            view.card(ZoneName::Hand, Side::You, &format!("Card {index}"));
        }
        for index in 0..4 {
            for (name, types) in [
                (format!("Land {index}"), vec![CardType::Land]),
                (format!("Relic {index}"), vec![CardType::Artifact]),
                (format!("Bear {index}"), vec![CardType::Creature]),
            ] {
                let card = view.card(ZoneName::Battlefield, Side::You, &name);
                card.types = types;
                card.tapped = index % 2 == 0;
            }
        }
        for (width, height) in PHONES {
            let window = phone(width, height);
            let frame = frame_in(window, &view);
            assert_eq!(
                card_faults(&frame, window, reachable),
                Vec::<String>::new(),
                "{count} cards at {width}x{height}"
            );
        }
    }
}

/// A hand of more cards than one fan holds stands its back row in the viewer's strip, where a
/// life total in the strip's middle would split that row's uncovered part into pieces too
/// narrow to press. The table raises the viewer's total over the hand, never onto their
/// creatures, and every streak or strike at the viewer aims where the total stands.
#[test]
fn the_viewers_life_total_rises_over_a_hand_that_reaches_into_its_strip() {
    for count in [1, 5, 9, 14] {
        let mut view = ViewBuilder::new();
        for index in 0..count {
            view.card(ZoneName::Hand, Side::You, &format!("Card {index}"));
        }
        view.script(ScriptBuilder::reset().build());
        for (width, height) in PHONES {
            let window = phone(width, height);
            let bands = phone_bands(window).expect("a phone window");
            let mut show = Show::new(Table::new(crate::test_data::data(), window));
            show.push(Box::new(view.update()), Vec::new());
            let frame = frame(&mut show, &mut Frame::with_capacity(4096, 4096, 65_536), 0);
            let hand = Place::Card {
                zone: ZoneName::Hand,
                side: Side::You,
                row: None,
            };
            let hand_top = (cards_at(&frame, hand).iter())
                .map(|card| bounds(card)[1])
                .fold(f32::INFINITY, f32::min);
            let table = show.table();
            let life = table
                .camera()
                .to_screen(table.player_pose(Side::You).position);
            let label = format!("{count} cards at {width}x{height}: the total at {life}");
            assert!((life.x - width / 2.0).abs() < 0.5, "{label}");
            if hand_top >= bands.your_strip.bottom() {
                assert!(
                    (life.y - bands.your_strip.middle()).abs() < 0.5,
                    "{label} stays in the strip's middle over one row"
                );
            } else {
                assert!(life.y < bands.your_strip.middle(), "{label} rises");
                assert!(
                    life.y < hand_top - 6.0 || life.y < bands.your_back.top + 20.0,
                    "{label} stands over the hand, whose top is at {hand_top}"
                );
            }
            assert!(
                life.y > bands.your_back.top,
                "{label} stays off the creatures, under {}",
                bands.your_back.top
            );
        }
    }
}

#[test]
fn the_stack_stands_in_the_middle_band_with_the_top_item_right_and_in_front() {
    for count in [1, 2, 4, 7, 20] {
        let mut view = ViewBuilder::new();
        for index in 0..count {
            view.card(ZoneName::Stack, Side::You, &format!("Spell {index}"));
        }
        view.card(ZoneName::Battlefield, Side::You, "Bear").types = vec![CardType::Creature];
        for (width, height) in PHONES {
            let window = phone(width, height);
            let bands = phone_bands(window).expect("a phone window");
            let frame = frame_in(window, &view);
            let stack = Place::Card {
                zone: ZoneName::Stack,
                side: Side::You,
                row: None,
            };
            let items = cards_at(&frame, stack);
            let label = format!("a stack of {count} at {width}x{height}");
            // One row, about the middle band, the top item at the right. A stack too long for
            // the row shows its top items and scrolls.
            let drawn = items.len();
            assert!(drawn > 0 && drawn <= count, "{label}: {drawn} drawn");
            if count <= 4 {
                assert_eq!(drawn, count, "{label}: a short stack shows every item");
            }
            assert!(count < 20 || drawn < count, "{label}: a long stack scrolls");
            if drawn < count {
                let names: Vec<&str> = items.iter().map(|item| frame.name(item.name)).collect();
                let top = format!("Spell {}", count - 1);
                assert!(names.contains(&top.as_str()), "{label}: {names:?}");
            }
            let mut lefts: Vec<f32> = items.iter().map(|item| bounds(item)[0]).collect();
            for item in &items {
                let y = centre(item).y;
                assert!(
                    (y - bands.middle.middle()).abs() < 1.0,
                    "{label}: {y} is off the middle band {:?}",
                    bands.middle
                );
            }
            let sorted = {
                let mut sorted = lefts.clone();
                sorted.sort_by(f32::total_cmp);
                sorted
            };
            lefts.dedup();
            assert_eq!(lefts.len(), drawn, "{label}: no two items share a place");
            assert!(sorted.windows(2).all(|pair| pair[1] - pair[0] >= 44.0));
            assert_eq!(
                card_faults(&frame, window, reachable)
                    .into_iter()
                    .filter(|message| message.starts_with("card \"Spell"))
                    .collect::<Vec<_>>(),
                Vec::<String>::new(),
                "{label}"
            );
            assert_eq!(
                card_faults(&frame, window, on_screen),
                Vec::<String>::new(),
                "{label}"
            );
        }
    }
}

#[test]
fn a_stack_item_leaves_every_tapped_creature_under_it_reachable() {
    let mut view = ViewBuilder::new();
    view.card(ZoneName::Stack, Side::You, "Spell");
    for side in [Side::You, Side::Opponent] {
        for index in 0..4 {
            let card = view.card(ZoneName::Battlefield, side, &format!("Bear {index}"));
            card.types = vec![CardType::Creature];
            card.tapped = true;
        }
    }
    let stack = Place::Card {
        zone: ZoneName::Stack,
        side: Side::You,
        row: None,
    };
    for (width, height) in PHONES {
        let window = phone(width, height);
        let frame = frame_in(window, &view);
        let item = bounds(cards_at(&frame, stack)[0]);
        // At 375 by 647, the shortest middle band, the item stands over both creatures' rows,
        // so the test judges what it covers.
        for side in [Side::You, Side::Opponent]
            .into_iter()
            .filter(|_| height < 700.0)
        {
            let row = cards_at(&frame, battlefield_place(side, RowName::Creatures));
            assert_eq!(row.len(), 4, "{width}x{height} {side:?}");
            assert!(
                row.iter().any(|card| {
                    let [left, top, right, bottom] = bounds(card);
                    left < item[2] && item[0] < right && top < item[3] && item[1] < bottom
                }),
                "{width}x{height}: the item {item:?} covers part of the {side:?} row"
            );
        }
        // At least as tall as the middle band, which is no shorter than the touch target.
        assert!(item[3] - item[1] >= 44.0, "{width}x{height}: {item:?}");
        assert_eq!(
            card_faults(&frame, window, reachable),
            Vec::<String>::new(),
            "{width}x{height}"
        );
    }
}

#[test]
fn each_pile_lies_in_its_players_strip() {
    let view = ViewBuilder::sample();
    for (width, height) in PHONES {
        let window = phone(width, height);
        let bands = phone_bands(window).expect("a phone window");
        let frame = frame_in(window, &view);
        let piles: Vec<&Resolved> = (frame.resolved.iter())
            .filter(|element| element.role == Role::Pile)
            .collect();
        assert!(!piles.is_empty());
        for pile in piles {
            let Place::Card { side, .. } = pile.place else {
                panic!("a pile has a side");
            };
            let strip = match side {
                Side::You => bands.your_strip,
                Side::Opponent => bands.opponent_strip,
            };
            let [left, top, right, bottom] = bounds(pile);
            let middle = f32::midpoint(top, bottom);
            assert!(
                middle >= strip.top && middle <= strip.bottom() + 8.0,
                "{width}x{height} {side:?}: {middle} is off {strip:?}"
            );
            assert!(top >= window.safe_area.top, "{width}x{height}: {top}");
            assert!(left >= bands.left - 3.0 && right < width / 2.0);
        }
    }
}

/// A resize in mid-flight, from a desktop window to a phone and from one phone to another,
/// leaves the card where it was at that instant, and it goes on to its place in the new layout.
#[test]
fn a_resize_in_mid_flight_into_and_between_phone_layouts_keeps_the_pose() {
    for (from, to) in [
        (Window::default(), phone(393.0, 759.0)),
        (phone(393.0, 759.0), phone(430.0, 932.0)),
        (phone(375.0, 647.0), Window::default()),
    ] {
        let (start, cards) = before();
        let (drawn, timings) = draw(&start, cards);
        let mut show = Show::new(Table::new(crate::test_data::data(), from));
        show.push(Box::new(start), Vec::new());
        show.push(Box::new(drawn.clone()), timings);
        let mut scratch = empty_frame();
        frame(&mut show, &mut scratch, 0);
        let card = usize::from(cards.top.0);
        frame(&mut show, &mut scratch, 150 * MS);
        let was = show.pose(card, 150 * MS).expect("the card is drawn");
        show.resize(to);
        frame(&mut show, &mut scratch, 150 * MS);
        let now = show.pose(card, 150 * MS).expect("the card is drawn");
        assert!(
            was.position.distance(now.position) < 1e-3
                && was.rotation.dot(now.rotation).abs() > 1.0 - 1e-6
                && (was.scale - now.scale).abs() < 1e-5,
            "{from:?} to {to:?}: the card does not jump: {was:?} became {now:?}"
        );
        assert_eq!(show.table().layout().bands.is_some(), to.phone());
        frame(&mut show, &mut scratch, 400 * MS);
        // The view the show ends on lists the drawn card last in the hand, as it rests.
        let shown = &show.shown().expect("the draw is shown").view;
        let rest = show
            .table()
            .placement(shown, &[])
            .get(cards.top)
            .expect("the drawn card rests in the hand")
            .pose;
        let end = show.pose(card, 400 * MS).expect("the card is drawn");
        assert!(
            end.position.distance(rest.position) < 1e-3,
            "{from:?} to {to:?}: the card ends at its place in the new layout"
        );
    }
}

#[test]
fn a_desktop_window_keeps_the_desktop_layout_with_its_rows_fitted() {
    let table = Table::new(crate::test_data::data(), Window::default());
    let fitted = layout(Window::default());
    assert_eq!(*table.layout(), fitted);
    assert!(fitted.bands.is_none() && fitted.fitted.is_some());
    assert_eq!(fitted.camera, REFERENCE.camera);
    assert!(layout(phone(393.0, 759.0)).bands.is_some());
}

/// The box on the window of each card of `held` held up in a tray on `window`, in logical
/// points, and how far it stands from the eye.
fn tray_boxes_on(window: Window, held: &Held) -> Vec<(Vec2, Vec2, f32)> {
    let table = Table::new(crate::test_data::data(), window);
    let camera = table.camera();
    (table.tray_poses(held).cards.iter())
        .map(|(_, pose)| {
            let (corners, depth) = camera.card_box(pose);
            let low = corners.iter().copied().fold(Vec2::MAX, Vec2::min);
            let high = corners.iter().copied().fold(Vec2::MIN, Vec2::max);
            (low, high, depth)
        })
        .collect()
}

fn ids(count: u16, from: u16) -> Vec<ObjectId> {
    (from..from + count).map(ObjectId).collect()
}

/// On the phone, every card a prompt holds up in its tray keeps a 44-point square at its top
/// left that no card in front of it covers, and stays out of the middle band, where the phase
/// stands, and off the viewer's strip, where the prompt's buttons stand: one pile in rows, a
/// pick's fan of more cards than one row holds, two piles in boxes one over the other, and a
/// hand put back onto the library.
#[test]
fn every_card_held_up_on_the_phone_keeps_a_square_to_press() {
    let pick = |count: u16| Held {
        piles: vec![ids(count, 1)],
        rooms: vec![None],
        kind: TrayKind::Boxes,
    };
    let fan = |count: u16| Held {
        piles: vec![ids(2, 1), ids(count - 2, 3)],
        rooms: vec![Some(2), None],
        kind: TrayKind::Fan {
            order: ids(count, 1),
        },
    };
    let split = |first: u16, second: u16| Held {
        piles: vec![ids(first, 1), ids(second, 100)],
        rooms: vec![None, None],
        kind: TrayKind::Boxes,
    };
    let put_back = |hand: u16| Held {
        piles: vec![Vec::new(), ids(hand, 1)],
        rooms: vec![Some(2), None],
        kind: TrayKind::PutBack,
    };
    let trays = [
        pick(1),
        pick(4),
        pick(8),
        pick(12),
        fan(9),
        fan(12),
        split(3, 4),
        split(0, 7),
        put_back(3),
        put_back(7),
    ];
    for (width, height) in PHONES {
        let window = phone(width, height);
        let bands = phone_bands(window).unwrap();
        for held in &trays {
            let boxes = tray_boxes_on(window, held);
            for (index, &(low, high, depth)) in boxes.iter().enumerate() {
                let label = format!("card {index} of {:?} at {width}x{height}", held.piles);
                let square = (low, low + Vec2::splat(44.0));
                assert!(
                    square.1.x <= high.x && square.1.y <= high.y,
                    "{label} is at least 44 points square: {low} to {high}"
                );
                let covered = boxes.iter().any(|&(l, h, d)| {
                    d < depth
                        && square.0.x < h.x
                        && l.x < square.1.x
                        && square.0.y < h.y
                        && l.y < square.1.y
                });
                assert!(!covered, "{label} keeps its top left square uncovered");
                let middle = (bands.middle.top, bands.middle.bottom());
                assert!(
                    high.y <= middle.0 || low.y >= middle.1,
                    "{label} stays out of the middle band {middle:?}: {low} to {high}"
                );
                assert!(
                    high.y <= bands.your_strip.top,
                    "{label} stays off the viewer's strip at {}: {low} to {high}",
                    bands.your_strip.top
                );
            }
        }
    }
}

/// A crowded row whose first permanent has an Aura peeking out to its right still leaves every
/// card after it a square to press. The row used to count that peek only where its permanent
/// was the last in view, so on the phone layout a crewed Vehicle with its Aura beside three
/// tapped creatures left the creatures 47 points apart, and each tapped card covered its
/// neighbour.
#[test]
fn an_aura_peeking_out_early_in_a_crowded_row_leaves_every_card_reachable() {
    let mut view = ViewBuilder::new();
    let vehicle = {
        let card = view.card(ZoneName::Battlefield, Side::You, "Vehicle");
        card.types = vec![CardType::Artifact, CardType::Creature];
        card.slot
    };
    let aura = view.card(ZoneName::Battlefield, Side::You, "Aura");
    aura.types = vec![CardType::Enchantment];
    aura.attached_to = Some(vehicle);
    for index in 0..3 {
        let card = view.card(ZoneName::Battlefield, Side::You, &format!("Bear {index}"));
        card.types = vec![CardType::Creature];
        card.tapped = true;
    }
    for (width, height) in PHONES {
        let window = phone(width, height);
        let frame = frame_in(window, &view);
        assert_eq!(
            card_faults(&frame, window, reachable),
            Vec::<String>::new(),
            "{width}x{height}"
        );
    }
}

/// An Aura attached to a tapped creature peeks out past the creature's right edge far enough to
/// press, in a row with room to spare. The untapped Aura is narrower than the tapped creature,
/// so at the plain peek step it showed only 37 points past the creature's edge on the phone.
#[test]
fn an_aura_on_a_tapped_creature_peeks_out_far_enough_to_press() {
    let mut view = ViewBuilder::new();
    let creature = {
        let card = view.card(ZoneName::Battlefield, Side::You, "Spirit");
        card.types = vec![CardType::Creature];
        card.tapped = true;
        card.slot
    };
    let aura = view.card(ZoneName::Battlefield, Side::You, "Aura");
    aura.types = vec![CardType::Enchantment];
    aura.attached_to = Some(creature);
    view.card(ZoneName::Battlefield, Side::You, "Bear").types = vec![CardType::Creature];
    for (width, height) in PHONES {
        let window = phone(width, height);
        let frame = frame_in(window, &view);
        assert_eq!(
            card_faults(&frame, window, reachable),
            Vec::<String>::new(),
            "{width}x{height}"
        );
    }
}

/// In a crowded row, the group after a permanent with an Aura attached stands past the Aura's
/// edge instead of overlapping the permanent. The Aura shows only past the permanent's right
/// edge, so a neighbour that overlapped the permanent left it 36 points on the phone.
#[test]
fn a_crowded_row_leaves_an_attached_aura_a_square_to_press() {
    let mut view = ViewBuilder::new();
    let host = {
        let card = view.card(ZoneName::Battlefield, Side::You, "Host");
        card.types = vec![CardType::Creature];
        card.slot
    };
    for index in 0..5 {
        view.card(ZoneName::Battlefield, Side::You, &format!("Bear {index}"))
            .types = vec![CardType::Creature];
    }
    let aura = view.card(ZoneName::Battlefield, Side::You, "Aura");
    aura.types = vec![CardType::Enchantment];
    aura.attached_to = Some(host);
    for (width, height) in PHONES {
        let window = phone(width, height);
        let frame = frame_in(window, &view);
        assert!(
            cards_at(&frame, battlefield_place(Side::You, RowName::Creatures)).len() > 2,
            "{width}x{height} shows the Aura's permanent and more than one creature"
        );
        assert_eq!(
            card_faults(&frame, window, reachable),
            Vec::<String>::new(),
            "{width}x{height}"
        );
    }
}

fn phone_bands(window: Window) -> Option<game_table::profiles::TableBands> {
    game_table::profiles::Profile::for_window(&crate::test_data::data().profiles, window).bands()
}

fn layout(window: Window) -> game_table::table::Layout {
    game_table::profiles::Profile::for_window(&crate::test_data::data().profiles, window).layout()
}
