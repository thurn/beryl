//! The tree printout, locators, hit testing and the frame checks, on frames built by hand.

use beryl_protocol::protocol::{RowName, Side, ZoneName};
use beryl_scene::agent::checks::{
    KeptArrow, blank_cards, check, duplicates, keep_arrows, lines_hold, named, on_screen, overlap,
    phone_sizes, reachable,
};
use beryl_scene::agent::locate::LocateError;
use beryl_scene::agent::text_overlap::text_overlap;
use beryl_scene::agent::tree::line;
use beryl_scene::agent::{Context, Fault, Layout, Rule, TreeOptions, hit, locate, tree, uncovered};
use beryl_scene::frame::{
    ElementId, Frame, FrameBuilder, NameRef, Place, Resolved, Role, StateBits,
};
use beryl_scene::input::{Insets, Rect, Size, Tier};
use glam::Vec2;

const LANDS: Place = Place::Card {
    zone: ZoneName::Battlefield,
    side: Side::You,
    row: Some(RowName::Lands),
};

/// One element to put in a frame.
struct Spec {
    name: &'static str,
    element: Resolved,
}

fn element(role: Role, name: &'static str, id: u32, place: Place, rect: Rect) -> Spec {
    let (x, y, right, bottom) = (rect.x, rect.y, rect.x + rect.width, rect.y + rect.height);
    Spec {
        name,
        element: Resolved {
            id: ElementId(id),
            parent: u32::MAX,
            role,
            name: NameRef::default(),
            place,
            corners: [
                Vec2::new(x, bottom),
                Vec2::new(right, bottom),
                Vec2::new(right, y),
                Vec2::new(x, y),
            ],
            depth: 0.0,
            opacity: 1.0,
            clip: None,
            interactive: false,
            state: StateBits::default(),
            face: (role == Role::Card).then_some(1),
            copies: 1,
            really: NameRef::default(),
            note: NameRef::default(),
        },
    }
}

fn rect(x: f32, y: f32, width: f32, height: f32) -> Rect {
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn card(name: &'static str, id: u32, place: Place, x: f32, y: f32, depth: f32) -> Spec {
    let mut spec = element(Role::Card, name, id, place, rect(x, y, 96.0, 134.0));
    spec.element.depth = depth;
    spec.element.interactive = true;
    spec
}

fn layer(role: Role, name: &'static str, id: u32, at: Rect) -> Spec {
    let mut spec = element(role, name, id, Place::Layer, at);
    spec.element.interactive = matches!(role, Role::Button | Role::Toggle | Role::Field);
    spec
}

impl Spec {
    fn parent(mut self, parent: u32) -> Self {
        self.element.parent = parent;
        self
    }

    fn with(mut self, change: impl FnOnce(&mut Resolved)) -> Self {
        change(&mut self.element);
        self
    }
}

fn frame(specs: Vec<Spec>) -> Frame {
    let mut frame = Frame::with_capacity(specs.len(), 0, 4096);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    for spec in specs {
        builder.resolved(spec.name, spec.element);
    }
    builder.finish();
    frame
}

fn context(layout: Layout) -> Context {
    let size = match layout {
        Layout::Desktop => Size {
            width: 1600.0,
            height: 900.0,
        },
        Layout::Phone => Size {
            width: 375.0,
            height: 647.0,
        },
    };
    Context {
        recording: "pay-default".to_owned(),
        seat: Side::You,
        layout,
        size,
        safe_area: Insets::default(),
        at: 850_000_000,
    }
}

fn desktop() -> Context {
    context(Layout::Desktop)
}

/// The rule and the locators of each fault.
fn summary(faults: &[Fault]) -> Vec<(Rule, Vec<String>)> {
    faults
        .iter()
        .map(|fault| {
            (
                fault.rule,
                fault
                    .elements
                    .iter()
                    .map(|subject| subject.locator.clone())
                    .collect(),
            )
        })
        .collect()
}

fn one(rule: Rule, locators: &[&str]) -> Vec<(Rule, Vec<String>)> {
    vec![(rule, locators.iter().map(|&s| s.to_owned()).collect())]
}

/// A prompt bar with a label and two buttons, over two lands.
fn prompt_frame() -> Frame {
    frame(vec![
        card("Island", 7, LANDS, 412.0, 640.0, 10.0),
        card("Island", 8, LANDS, 516.0, 640.0, 10.0).with(|e| e.state.0 |= StateBits::ELIGIBLE),
        layer(Role::Region, "prompt", 100, rect(520.0, 820.0, 560.0, 64.0)),
        layer(
            Role::Text,
            "Pay {1}{U}",
            101,
            rect(536.0, 838.0, 180.0, 28.0),
        )
        .parent(2),
        layer(
            Role::Button,
            "Auto Pay",
            102,
            rect(880.0, 832.0, 96.0, 40.0),
        )
        .parent(2),
        layer(Role::Button, "Cancel", 103, rect(984.0, 832.0, 80.0, 40.0)).parent(2),
    ])
}

#[test]
fn the_tree_prints_each_element_in_the_doc_format() {
    let frame = prompt_frame();
    let printed = tree(&frame, &TreeOptions::default()).unwrap();
    assert_eq!(
        printed,
        "game\n\
         \x20 table\n\
         \x20   card \"Island\" #7 battlefield.yours.lands 412,640 96x134 interactive\n\
         \x20   card \"Island\" #8 battlefield.yours.lands 516,640 96x134 interactive eligible\n\
         \x20 layer\n\
         \x20   region \"prompt\" #100 520,820 560x64\n\
         \x20     text \"Pay {1}{U}\" #101 536,838 180x28\n\
         \x20     button \"Auto Pay\" #102 880,832 96x40 interactive\n\
         \x20     button \"Cancel\" #103 984,832 80x40 interactive\n"
    );
}

#[test]
fn the_tree_prints_states_covered_and_faded() {
    let frame = frame(vec![
        card("Island", 1, LANDS, 0.0, 0.0, 10.0).with(|e| {
            e.state.0 |= StateBits::TAPPED | StateBits::MOVING;
            e.opacity = 0.75;
        }),
        // Covers the left half of the first card.
        card("Plains", 2, LANDS, -48.0, 0.0, 5.0),
    ]);
    assert_eq!(
        line(&frame, 0),
        "card \"Island\" #1 battlefield.yours.lands 0,0 96x134 interactive tapped moving \
         covered(50%) faded(25%)"
    );
}

#[test]
fn a_crowded_place_prints_three_cards_and_a_count() {
    let frame = frame(
        (0..20)
            .map(|i| card("Island", i, LANDS, i as f32 * 100.0, 600.0, 10.0))
            .collect(),
    );
    let printed = tree(&frame, &TreeOptions::default()).unwrap();
    let lines: Vec<&str> = printed.lines().collect();
    assert_eq!(lines.len(), 6, "{printed}");
    assert_eq!(
        lines[5],
        "    ... 17 more in battlefield.yours.lands (--within battlefield.yours.lands)"
    );
    let whole = |options: TreeOptions| tree(&frame, &options).unwrap().lines().count();
    assert_eq!(
        whole(TreeOptions {
            within: Some("battlefield.yours.lands".to_owned()),
            ..TreeOptions::default()
        }),
        22
    );
    assert_eq!(
        whole(TreeOptions {
            full: true,
            ..TreeOptions::default()
        }),
        22
    );
}

/// A board of 300 cards: every zone of both sides holds some, and the battlefield rows many.
fn board_of_300() -> Frame {
    let mut specs = Vec::new();
    let mut id = 0;
    for side in [Side::You, Side::Opponent] {
        for (zone, row, count) in [
            (ZoneName::Library, None, 40),
            (ZoneName::Hand, None, 7),
            (ZoneName::Battlefield, Some(RowName::Lands), 30),
            (ZoneName::Battlefield, Some(RowName::Creatures), 30),
            (ZoneName::Battlefield, Some(RowName::Others), 20),
            (ZoneName::Graveyard, None, 13),
            (ZoneName::Exile, None, 10),
        ] {
            for i in 0..count {
                let place = Place::Card { zone, side, row };
                let name = if zone == ZoneName::Library {
                    ""
                } else {
                    "Island"
                };
                specs.push(card(name, id, place, i as f32 * 20.0, id as f32, 10.0));
                id += 1;
            }
        }
    }
    assert_eq!(id, 300);
    frame(specs)
}

#[test]
fn the_tree_of_300_cards_is_under_80_lines() {
    let frame = board_of_300();
    let printed = tree(&frame, &TreeOptions::default()).unwrap();
    assert!(printed.lines().count() < 80, "{printed}");
    let full = TreeOptions {
        full: true,
        ..TreeOptions::default()
    };
    assert_eq!(tree(&frame, &full).unwrap().lines().count(), 302);
}

#[test]
fn every_tree_line_locates_its_own_element() {
    let mut boards = vec![prompt_frame(), board_of_300()];
    // Names that need escaping, and an unnamed card.
    boards.push(frame(vec![
        layer(
            Role::Button,
            "Say \"hi\" > bye",
            1,
            rect(0.0, 0.0, 50.0, 50.0),
        ),
        card("", 3, LANDS, 100.0, 100.0, 10.0),
    ]));
    for frame in &boards {
        let options = TreeOptions {
            full: true,
            ..TreeOptions::default()
        };
        for printed in tree(frame, &options).unwrap().lines() {
            let printed = printed.trim();
            if matches!(printed, "game" | "table" | "layer") {
                continue;
            }
            let index = locate(frame, printed).unwrap_or_else(|error| panic!("{error}"));
            assert_eq!(line(frame, index), printed);
        }
    }
}

#[test]
fn locators_look_inside_other_elements() {
    let frame = frame(vec![
        layer(Role::Region, "prompt", 1, rect(0.0, 0.0, 300.0, 60.0)),
        layer(Role::Button, "Cancel", 2, rect(10.0, 10.0, 80.0, 40.0)).parent(0),
        layer(Role::Region, "browser", 3, rect(0.0, 100.0, 300.0, 60.0)),
        layer(Role::Button, "Cancel", 4, rect(10.0, 110.0, 80.0, 40.0)).parent(2),
    ]);
    assert_eq!(
        locate(&frame, "region \"browser\" > button \"Cancel\""),
        Ok(3)
    );
    assert_eq!(locate(&frame, "button #2"), Ok(1));
    assert!(matches!(
        locate(&frame, "button \"Cancel\""),
        Err(LocateError::Ambiguous { count: 2, .. })
    ));
}

#[test]
fn a_locator_matching_two_elements_names_both() {
    let frame = prompt_frame();
    let error = locate(&frame, "card \"Island\"").unwrap_err();
    let message = error.to_string();
    assert!(message.contains("card \"Island\" #7"), "{message}");
    assert!(message.contains("card \"Island\" #8"), "{message}");
    assert!(message.contains("#id"), "{message}");
}

#[test]
fn a_locator_matching_nothing_lists_the_nearest_names() {
    let frame = prompt_frame();
    let Err(LocateError::NotFound { nearest, .. }) = locate(&frame, "button \"Auto Py\"") else {
        panic!("expected no match");
    };
    assert_eq!(nearest[0], "button \"Auto Pay\" #102");
    assert!(matches!(
        locate(&frame, "button Cancel"),
        Err(LocateError::Syntax { .. })
    ));
    assert!(matches!(
        locate(&frame, "widget \"Cancel\""),
        Err(LocateError::Syntax { .. })
    ));
}

#[test]
fn a_hit_finds_the_nearest_element_inside_its_clip() {
    let frame = frame(vec![
        card("Island", 1, LANDS, 0.0, 0.0, 10.0),
        card("Plains", 2, LANDS, 50.0, 0.0, 5.0),
        // A list clipped to its top half.
        layer(Role::List, "browser", 3, rect(300.0, 0.0, 100.0, 100.0))
            .with(|e| e.clip = Some(rect(300.0, 0.0, 100.0, 50.0))),
    ]);
    assert_eq!(hit(&frame, Vec2::new(20.0, 20.0)), Some(0));
    assert_eq!(hit(&frame, Vec2::new(80.0, 20.0)), Some(1));
    assert_eq!(hit(&frame, Vec2::new(350.0, 25.0)), Some(2));
    assert_eq!(hit(&frame, Vec2::new(350.0, 75.0)), None);
    assert_eq!(uncovered(&frame, 0), Some(rect(0.0, 0.0, 50.0, 134.0)));
}

#[test]
fn the_flat_layer_covers_the_table() {
    let frame = frame(vec![
        layer(Role::Region, "prompt", 1, rect(0.0, 0.0, 200.0, 200.0)),
        card("Island", 2, LANDS, 0.0, 0.0, 1.0),
    ]);
    assert_eq!(hit(&frame, Vec2::new(20.0, 20.0)), Some(0));
}

#[test]
fn a_clean_frame_has_no_faults() {
    let frame = prompt_frame();
    assert_eq!(summary(&check(&frame, &desktop())), vec![]);
}

#[test]
fn two_overlapping_cards_in_a_row_are_not_a_fault() {
    let frame = frame(vec![
        card("Island", 1, LANDS, 0.0, 600.0, 10.0),
        card("Island", 2, LANDS, 40.0, 600.0, 9.0),
    ]);
    assert_eq!(summary(&check(&frame, &desktop())), vec![]);
}

#[test]
fn an_element_off_screen_is_a_fault() {
    let frame = frame(vec![layer(
        Role::Button,
        "Done",
        1,
        rect(1550.0, 10.0, 80.0, 40.0),
    )]);
    assert_eq!(
        summary(&on_screen(&frame, &desktop())),
        one(Rule::OnScreen, &["button \"Done\" #1"])
    );
    let mut inset = desktop();
    inset.size.width = 1700.0;
    inset.safe_area.right = 100.0;
    assert_eq!(summary(&on_screen(&frame, &inset)).len(), 1);
}

/// A 375 by 647 phone window whose safe area is a different distance in from every edge, so
/// that a check reading one inset for another fails: x from 12 to 355, y from 44 to 613.
fn inset_phone() -> Context {
    let mut context = context(Layout::Phone);
    context.safe_area = Insets {
        top: 44.0,
        right: 20.0,
        bottom: 34.0,
        left: 12.0,
    };
    context
}

#[test]
fn an_enabled_button_one_point_into_any_inset_is_off_screen() {
    let context = inset_phone();
    let button =
        |x: f32, y: f32| frame(vec![layer(Role::Button, "Done", 1, rect(x, y, 44.0, 44.0))]);
    let fault = one(Rule::OnScreen, &["button \"Done\" #1"]);
    // Touching each edge of the safe area, then one point past it.
    for (inside, past) in [
        ((12.0, 300.0), (11.0, 300.0)),
        ((311.0, 300.0), (312.0, 300.0)),
        ((100.0, 44.0), (100.0, 43.0)),
        ((100.0, 569.0), (100.0, 570.0)),
    ] {
        assert_eq!(
            summary(&on_screen(&button(inside.0, inside.1), &context)),
            vec![]
        );
        assert_eq!(
            summary(&on_screen(&button(past.0, past.1), &context)),
            fault,
            "{past:?}"
        );
    }
    // A disabled button is not a target, so it may sit under the status bar.
    let disabled = frame(vec![
        layer(Role::Button, "Done", 1, rect(100.0, 0.0, 44.0, 44.0))
            .with(|e| e.interactive = false),
    ]);
    assert_eq!(summary(&on_screen(&disabled, &context)), vec![]);
}

/// A frame of one button of `width` by `height` points inside the phone's safe area, with one line
/// of its text at `text` points.
fn sized(width: f32, height: f32, text: f32) -> Frame {
    let mut frame = Frame::with_capacity(1, 1, 4096);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    let spec = layer(Role::Button, "Done", 1, rect(100.0, 300.0, width, height));
    builder.resolved(spec.name, spec.element);
    builder.line(0, rect(104.0, 310.0, 30.0, text), text, 0.0, "");
    builder.finish();
    frame
}

#[test]
fn a_44_point_button_and_11_point_text_pass_the_phone_sizes_and_one_point_less_fails() {
    let context = inset_phone();
    let safe = sized(44.0, 44.0, 11.0);
    assert_eq!(summary(&on_screen(&safe, &context)), vec![]);
    assert_eq!(summary(&phone_sizes(&safe, &context)), vec![]);
    let fault = one(Rule::PhoneSizes, &["button \"Done\" #1"]);
    for (width, height, text) in [(43.0, 44.0, 11.0), (44.0, 43.0, 11.0), (44.0, 44.0, 10.0)] {
        let frame = sized(width, height, text);
        assert_eq!(
            summary(&phone_sizes(&frame, &context)),
            fault,
            "{width}x{height}, {text}-point text"
        );
    }
    // The desktop layout has no such limits.
    assert_eq!(
        summary(&phone_sizes(&sized(30.0, 30.0, 9.0), &desktop())),
        vec![]
    );
}

#[test]
fn overlapping_buttons_are_a_fault() {
    let frame = frame(vec![
        layer(Role::Button, "Auto Pay", 1, rect(188.0, 598.0, 96.0, 44.0)),
        layer(Role::Button, "Cancel", 2, rect(270.0, 598.0, 80.0, 44.0)),
    ]);
    let faults = overlap(&frame, &context(Layout::Phone));
    assert_eq!(
        summary(&faults),
        one(
            Rule::Overlap,
            &["button \"Auto Pay\" #1", "button \"Cancel\" #2"]
        )
    );
    assert_eq!(
        faults[0].print(&context(Layout::Phone)),
        "FAULT overlap  b3d:pay-default/you/375x647 t=850ms\n  \
         button \"Auto Pay\" #1 (188,598 96x44) overlaps button \"Cancel\" #2 (270,598 80x44)\n  \
         show: just b3d tree pay-default --seat you --layout phone --size 375x647 --at 850ms"
    );
}

#[test]
fn a_covered_button_is_a_fault() {
    let frame = frame(vec![
        layer(Role::Button, "Done", 1, rect(0.0, 0.0, 80.0, 40.0)),
        layer(Role::Region, "tooltip", 2, rect(20.0, 0.0, 100.0, 100.0)),
    ]);
    assert_eq!(
        summary(&reachable(&frame, &desktop())),
        one(
            Rule::Reachable,
            &["button \"Done\" #1", "region \"tooltip\" #2"]
        )
    );
}

#[test]
fn a_row_scrolled_partly_out_of_its_list_is_pressed_where_it_shows() {
    // A list over a screen's region shows only the top 2 points of its last row, whose box
    // centre lies over the region below the list.
    let list = rect(0.0, 0.0, 200.0, 100.0);
    let frame = frame(vec![
        layer(Role::Region, "screen", 1, rect(0.0, 0.0, 200.0, 120.0)),
        layer(Role::List, "recordings", 2, list).parent(0),
        layer(Role::Button, "last", 3, rect(0.0, 98.0, 200.0, 28.0))
            .parent(1)
            .with(|e| e.clip = Some(list)),
    ]);
    assert_eq!(summary(&reachable(&frame, &desktop())), vec![]);
}

#[test]
fn a_card_with_too_small_an_uncovered_strip_is_a_fault() {
    // The nearer card leaves a strip of the farther one `strip` points wide.
    let strip_of = |strip: f32| {
        frame(vec![
            card("Island", 1, LANDS, 0.0, 0.0, 10.0),
            card("Plains", 2, LANDS, strip, 0.0, 5.0),
        ])
    };
    assert_eq!(
        summary(&reachable(&strip_of(20.0), &desktop())),
        one(Rule::Reachable, &["card \"Island\" #1"])
    );
    assert_eq!(summary(&reachable(&strip_of(30.0), &desktop())), vec![]);
    assert_eq!(
        summary(&reachable(&strip_of(30.0), &context(Layout::Phone))),
        one(Rule::Reachable, &["card \"Island\" #1"])
    );
    // A card in flight is not held to it.
    let moving = frame(vec![
        card("Island", 1, LANDS, 0.0, 0.0, 10.0).with(|e| e.state.0 |= StateBits::MOVING),
        card("Plains", 2, LANDS, 20.0, 0.0, 5.0),
    ]);
    assert_eq!(summary(&reachable(&moving, &desktop())), vec![]);
}

#[test]
fn duplicate_ids_and_siblings_are_faults() {
    let cards = frame(vec![
        card("Island", 4, LANDS, 0.0, 0.0, 10.0),
        card("Island", 4, LANDS, 200.0, 0.0, 10.0),
        // A flat-layer element may share a number with a card: the ids are counted apart.
        layer(Role::Button, "Done", 4, rect(400.0, 0.0, 80.0, 40.0)),
    ]);
    assert_eq!(
        summary(&duplicates(&cards, &desktop())),
        one(
            Rule::Duplicate,
            &["card \"Island\" #4", "card \"Island\" #4"]
        )
    );
    let siblings = frame(vec![
        layer(Role::Region, "prompt", 1, rect(0.0, 0.0, 300.0, 60.0)),
        layer(Role::Button, "Cancel", 2, rect(10.0, 10.0, 80.0, 40.0)).parent(0),
        layer(Role::Button, "Cancel", 3, rect(100.0, 10.0, 80.0, 40.0)).parent(0),
    ]);
    assert_eq!(
        summary(&duplicates(&siblings, &desktop())),
        one(
            Rule::Duplicate,
            &["button \"Cancel\" #2", "button \"Cancel\" #3"]
        )
    );
    // Two arrows of one kind from one spell are named alike and told apart by their ids.
    let arrows = frame(vec![
        layer(
            Role::Arrow,
            "hostile-target",
            5,
            rect(0.0, 0.0, 300.0, 60.0),
        ),
        layer(
            Role::Arrow,
            "hostile-target",
            6,
            rect(0.0, 100.0, 300.0, 60.0),
        ),
    ]);
    assert_eq!(summary(&duplicates(&arrows, &desktop())), vec![]);
}

#[test]
fn an_unnamed_interactive_element_is_a_fault() {
    let frame = frame(vec![layer(Role::Button, "", 9, rect(0.0, 0.0, 80.0, 40.0))]);
    assert_eq!(
        summary(&named(&frame, &desktop())),
        one(Rule::Named, &["button #9"])
    );
}

#[test]
fn a_known_face_up_card_without_a_face_is_a_fault() {
    let frame = frame(vec![
        card("Island", 1, LANDS, 0.0, 0.0, 10.0).with(|e| e.face = None),
        card("Island", 2, LANDS, 200.0, 0.0, 10.0),
        card("Island", 3, LANDS, 400.0, 0.0, 10.0).with(|e| {
            e.face = None;
            e.state.0 |= StateBits::FACE_DOWN;
        }),
        card("", 4, LANDS, 600.0, 0.0, 10.0).with(|e| e.face = None),
    ]);
    assert_eq!(
        summary(&blank_cards(&frame, &desktop())),
        one(Rule::BlankCard, &["card \"Island\" #1"])
    );
}

/// An arrow from (0, 0) to (300, 0), bending through `bend`.
fn arrow(start: Vec2, bend: f32, end: Vec2) -> Frame {
    frame(vec![
        element(Role::Arrow, "target", 5, LANDS, rect(0.0, 0.0, 0.0, 0.0)).with(|e| {
            e.corners = [start, Vec2::new(100.0, bend), Vec2::new(200.0, bend), end];
        }),
    ])
}

#[test]
fn an_arrow_that_changes_shape_with_its_ends_at_rest_is_a_fault() {
    let (start, end) = (Vec2::ZERO, Vec2::new(300.0, 0.0));
    let mut before: Vec<KeptArrow> = Vec::new();
    keep_arrows(&arrow(start, -50.0, end), &mut before);
    assert_eq!(
        summary(&lines_hold(&before, &arrow(start, -80.0, end), &desktop())),
        one(Rule::LinesHold, &["arrow \"target\" #5"])
    );
    assert_eq!(
        summary(&lines_hold(&before, &arrow(start, -51.0, end), &desktop())),
        vec![]
    );
    // An end moved, so the arrow is redrawn.
    let moved = Vec2::new(320.0, 0.0);
    assert_eq!(
        summary(&lines_hold(
            &before,
            &arrow(start, -80.0, moved),
            &desktop()
        )),
        vec![]
    );
}

#[test]
fn a_card_drawn_in_perspective_is_not_covered_by_its_own_slant() {
    // A trapezoid, as the camera draws a card lying on the table: its box's corners are
    // outside it.
    let frame = frame(vec![card("Island", 1, LANDS, 0.0, 0.0, 10.0).with(|e| {
        e.corners = [
            Vec2::new(0.0, 137.0),
            Vec2::new(117.0, 137.0),
            Vec2::new(100.0, 0.0),
            Vec2::new(17.0, 0.0),
        ];
    })]);
    assert_eq!(
        line(&frame, 0),
        "card \"Island\" #1 battlefield.yours.lands 0,0 117x137 interactive"
    );
    let rect = uncovered(&frame, 0).unwrap();
    assert!(rect.width >= 80.0 && rect.height >= 120.0, "{rect:?}");
    assert_eq!(summary(&check(&frame, &context(Layout::Phone))), vec![]);
}

#[test]
fn reachable_finds_the_same_cards_as_the_largest_free_rectangle_in_a_crowded_row() {
    // A row of overlapping cards: each nearer card hides all but a strip of the one before it,
    // a strip 10 points wide where they are packed and 30 where they are spread. A few cards are
    // drawn behind their neighbours instead, and a panel of the flat layer covers part of the row.
    let mut specs = Vec::new();
    let mut x = 0.0;
    for n in 0..40_u32 {
        let depth = if n % 7 == 3 {
            50.0
        } else {
            40.0 - n as f32 * 0.5
        };
        specs.push(card("Island", n + 1, LANDS, x, 100.0, depth));
        x += if n % 5 < 2 { 30.0 } else { 10.0 };
    }
    specs.push(layer(
        Role::Region,
        "panel",
        99,
        rect(150.0, 0.0, 60.0, 180.0),
    ));
    let frame = frame(specs);
    let side = Size {
        width: 24.0,
        height: 24.0,
    };
    let expected: Vec<String> = (0..40)
        .filter(|&index| beryl_scene::agent::hit::free_rect(&frame, index, side).is_none())
        .map(|index| beryl_scene::agent::tree::locator_of(&frame, index))
        .collect();
    let found: Vec<String> = reachable(&frame, &desktop())
        .iter()
        .map(|fault| fault.elements[0].locator.clone())
        .collect();
    assert!(!expected.is_empty() && expected.len() < 40);
    assert_eq!(found, expected);
}

#[test]
fn the_layout_follows_the_shorter_side_of_the_window() {
    let size = |width, height| Size { width, height };
    for (width, height) in [(393.0, 759.0), (375.0, 647.0), (430.0, 932.0)] {
        assert_eq!(Layout::for_size(size(width, height)), Layout::Phone);
        // Rotated, the same phone is still a phone.
        assert_eq!(Layout::for_size(size(height, width)), Layout::Phone);
    }
    assert_eq!(Layout::for_size(size(499.9, 1200.0)), Layout::Phone);
    assert_eq!(Layout::for_size(size(500.0, 500.0)), Layout::Desktop);
    assert_eq!(Layout::for_size(size(500.0, 1200.0)), Layout::Desktop);
    assert_eq!(Layout::for_size(size(1280.0, 800.0)), Layout::Desktop);
}

/// A frame of a life total, a float of the flat layer, whose text line lies at `life`, beside the
/// text "On the play" and a toggle, and the toggle's own label inside it.
fn life_beside_text(life: Rect) -> Frame {
    let mut frame = Frame::with_capacity(4, 0, 4096);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    let total = layer(Role::Player, "You 20", 1, life);
    let play = layer(
        Role::Text,
        "On the play",
        2,
        rect(600.0, 700.0, 120.0, 24.0),
    );
    let toggle = layer(
        Role::Toggle,
        "Your upkeep",
        3,
        rect(800.0, 700.0, 22.0, 22.0),
    );
    let label = layer(Role::Text, "Upkeep", 4, rect(800.0, 700.0, 22.0, 22.0)).parent(2);
    for spec in [total, play, toggle, label] {
        builder.resolved(spec.name, spec.element);
    }
    builder.line(0, life, 40.0, 0.0, "");
    builder.line(1, rect(600.0, 700.0, 120.0, 24.0), 18.0, 0.0, "");
    builder.line(3, rect(801.0, 701.0, 20.0, 20.0), 12.0, 0.0, "");
    builder.finish();
    frame
}

#[test]
fn a_life_total_over_a_line_of_text_overlaps_it_and_a_label_inside_its_toggle_does_not() {
    let clear = life_beside_text(rect(600.0, 640.0, 60.0, 48.0));
    assert_eq!(summary(&text_overlap(&clear, &desktop())), Vec::new());
    let over = life_beside_text(rect(620.0, 680.0, 60.0, 48.0));
    assert_eq!(
        summary(&text_overlap(&over, &desktop())),
        one(
            Rule::TextOverlap,
            &[r#"player "You 20" #1"#, r#"text "On the play" #2"#]
        )
    );
    let on_toggle = life_beside_text(rect(780.0, 690.0, 40.0, 30.0));
    assert_eq!(
        summary(&text_overlap(&on_toggle, &desktop())),
        vec![
            (
                Rule::TextOverlap,
                vec![
                    r#"player "You 20" #1"#.to_owned(),
                    r#"toggle "Your upkeep" #3"#.to_owned()
                ]
            ),
            (
                Rule::TextOverlap,
                vec![
                    r#"player "You 20" #1"#.to_owned(),
                    r#"text "Upkeep" #4"#.to_owned()
                ]
            ),
        ]
    );
}
