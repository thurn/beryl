//! A battlefield row that scrolls (docs/beryl-3d.md, "Objects and placement"): a row of more
//! cards than fit with their least strip shows as many as fit, scrolls under the wheel and a
//! drag, and shows how many cards are off each end.

use app_client::Input;
use app_client::driver::{Duration, Mode, Page, Recording};
use app_client::{Client, Config};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, PlayerView, RowName, Side, Why, ZoneName};
use beryl_protocol::script::{Act, Script, Step, StepTiming};
use beryl_protocol::testing::{self, ScriptBuilder, ViewBuilder, step, timing};
use game_probe::agent::steps::rest_equals_placement;
use glam::Vec2;
use kit_frame::frame::{Frame, Place, Role};
use kit_probe::agent::Rule;
use kit_scene::hit::uncovered;
use kit_ui::input::{Button, Size};
use kit_ui::input::{Insets, Modifiers, Pointer, PointerKind, PointerPhase};

const WINDOW: Size = Size {
    width: 1440.0,
    height: 900.0,
};

/// One notch of a wheel turned down, toward a row's right end.
const NOTCH_DOWN: Vec2 = Vec2::new(0.0, -16.0);

/// A table whose viewer has `count` different lands, "Land 0" first. The lands' row is the
/// shortest, so it is the first to scroll.
fn lands(count: usize) -> ViewBuilder {
    let mut view = ViewBuilder::new();
    for index in 0..count {
        view.card(ZoneName::Battlefield, Side::You, &format!("Land {index}"))
            .types = vec![CardType::Land];
    }
    view.script(ScriptBuilder::reset().build());
    view
}

fn page(view: &ViewBuilder) -> Page {
    let messages = vec![FixtureMessage::game_update(&view.update())];
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        crate::REGULAR,
        WINDOW,
    )
    .expect("the page opens");
    page.still();
    page
}

/// The viewer's lands drawn in `frame`, left to right, each with its element's index.
fn row(frame: &Frame) -> Vec<(usize, u32)> {
    let mut cards: Vec<(f32, usize, u32)> = (frame.resolved.iter().enumerate())
        .filter(|(_, element)| {
            element.role == Role::Card
                && element.opacity > 0.0
                && element.place
                    == Place::Card {
                        zone: ZoneName::Battlefield,
                        side: Side::You,
                        row: Some(RowName::Lands),
                    }
        })
        .map(|(index, element)| {
            let name = frame.name(element.name);
            let number = name["Land ".len()..].parse().expect("a land's number");
            let left = element
                .corners
                .iter()
                .map(|c| c.x)
                .fold(f32::INFINITY, f32::min);
            (left, number, u32::try_from(index).expect("few elements"))
        })
        .collect();
    cards.sort_by(|a, b| a.0.total_cmp(&b.0));
    cards
        .into_iter()
        .map(|(_, number, index)| (number, index))
        .collect()
}

/// The number of the first land in view.
fn first(page: &mut Page) -> usize {
    row(page.frame())[0].0
}

/// The count shown off the viewer's lands' `end` ("left" or "right"), if any.
fn count(page: &mut Page, end: &str) -> Option<usize> {
    let prefix = format!("Your lands {end} ");
    let frame = page.frame();
    frame
        .resolved
        .iter()
        .find_map(|element| frame.name(element.name).strip_prefix(&prefix)?.parse().ok())
}

/// A point of the row where a press reaches a card: the middle of the uncovered part of the
/// card at `at` in the row, counting from the left.
fn on_card(page: &mut Page, at: usize) -> Vec2 {
    let frame = page.frame();
    let index = row(frame)[at].1;
    let rect = uncovered(frame, index).expect("part of the card is uncovered");
    Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0)
}

fn ms(milliseconds: u64) -> Duration {
    Duration::from_millis(milliseconds)
}

#[test]
fn a_row_too_long_to_fit_shows_a_count_at_its_right_end() {
    let mut page = page(&lands(40));
    let shown = row(page.frame()).len();
    assert!(shown < 40 - 3, "{shown} of 40 lands in view");
    assert_eq!(first(&mut page), 0);
    assert_eq!(count(&mut page, "right"), Some(40 - shown));
    assert_eq!(count(&mut page, "left"), None);
}

#[test]
fn each_notch_of_the_wheel_scrolls_the_row_one_group() {
    let mut page = page(&lands(40));
    let right = count(&mut page, "right").expect("cards off the right end");
    for _ in 0..3 {
        let point = on_card(&mut page, 2);
        page.wheel(point, NOTCH_DOWN);
    }
    page.still();
    assert_eq!(first(&mut page), 3);
    assert_eq!(count(&mut page, "left"), Some(3));
    assert_eq!(count(&mut page, "right"), Some(right - 3));
}

#[test]
fn a_drag_scrolls_one_group_for_each_step_of_travel() {
    let mut page = page(&lands(40));
    let from = on_card(&mut page, 4);
    page.drag_between(from, from - Vec2::new(96.0, 0.0));
    page.still();
    assert_eq!(first(&mut page), 2);
    // Back the other way, past the start, stops at the left end.
    let from = on_card(&mut page, 1);
    page.drag_between(from, from + Vec2::new(400.0, 0.0));
    page.still();
    assert_eq!(first(&mut page), 0);
    assert_eq!(count(&mut page, "left"), None);
}

#[test]
fn keys_scroll_the_row_last_pressed_by_whole_groups() {
    use kit_ui::input::KeyCode;

    let mut page = page(&lands(40));
    let point = on_card(&mut page, 1);
    for phase in [PointerPhase::Down, PointerPhase::Up] {
        page.give_input(Input::Pointer(Pointer {
            id: 0,
            kind: PointerKind::Mouse,
            phase,
            position: point,
            button: Button::Primary,
            wheel: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }));
    }
    page.key(KeyCode::ArrowRight);
    page.still();
    assert_eq!(first(&mut page), 1);
    page.key(KeyCode::End);
    page.still();
    assert_eq!(count(&mut page, "right"), None);
    page.key(KeyCode::Home);
    page.still();
    assert_eq!(first(&mut page), 0);
}

#[test]
fn the_wheel_scrolls_a_group_window_supplied_by_the_arrangement_solver() {
    use game_table::table::REFERENCE;
    use glam::Quat;
    use kit_base::geometry::Window;
    use kit_space::arrange::{Container, Item, Limits, Policy, Region, arrange};
    use kit_space::camera::Camera;
    use kit_ui::scroll::{ScrollGesture, ScrollInput, Viewport};

    let camera = Camera::fit(REFERENCE.camera, Window::default());
    let container = Container {
        region: Region {
            centre: camera.data.target,
            size: Vec2::new(100.0, 120.0),
            rotation: Quat::IDENTITY,
        },
        policy: Policy::Grid {
            columns: 1,
            gap: Vec2::ZERO,
        },
        limits: Limits {
            scroll: true,
            ..Limits::default()
        },
    };
    let items: Vec<_> = (0..5)
        .map(|key| Item {
            key,
            size: Vec2::splat(50.0),
            pressable: false,
            group: None,
            under: None,
        })
        .collect();
    let initial = arrange(&container, &items, &camera, 0);
    assert_eq!((initial.before, initial.after), (0, 3));
    let initial = Viewport::groups(initial.scroll.expect("the grid scrolls"));

    let mut gesture = ScrollGesture::default();
    let wheel = wheel_at(Vec2::ZERO, NOTCH_DOWN);
    let ScrollInput::Claimed(Some(((), offset))) =
        gesture.pointer(&wheel, 0, Some(((), initial)), |()| None)
    else {
        panic!("one notch should advance one group");
    };
    assert_eq!(offset, Vec2::Y);
    let scrolled = arrange(&container, &items, &camera, offset.y as usize);
    assert_eq!((scrolled.before, scrolled.after), (1, 2));
}

#[test]
fn scrolling_past_the_end_stops_at_it() {
    let mut page = page(&lands(40));
    let shown = row(page.frame()).len();
    let point = on_card(&mut page, 0);
    page.wheel(point, NOTCH_DOWN * 100.0);
    page.still();
    assert_eq!(first(&mut page), 40 - shown);
    assert_eq!(count(&mut page, "right"), None);
    assert_eq!(count(&mut page, "left"), Some(40 - shown));
    let point = on_card(&mut page, 0);
    page.wheel(point, NOTCH_DOWN);
    assert!(!page.position().2, "nothing moves at the end");
    assert_eq!(first(&mut page), 40 - shown);
}

/// The least distance in the window between the left edges of two neighbouring cards of the
/// viewer's lands, which is the width of the strip each keeps uncovered.
fn least_spacing(frame: &Frame) -> f32 {
    let lefts: Vec<f32> = (row(frame).iter())
        .map(|&(_, index)| {
            let corners = frame.resolved[index as usize].corners;
            corners.iter().map(|c| c.x).fold(f32::INFINITY, f32::min)
        })
        .collect();
    lefts
        .windows(2)
        .map(|pair| pair[1] - pair[0])
        .fold(f32::INFINITY, f32::min)
}

/// How many cards the reachable check finds with no 24 point square to press.
fn unreachable(page: &mut Page) -> usize {
    (page.faults().iter())
        .filter(|fault| fault.rule == Rule::Reachable)
        .count()
}

#[test]
fn every_card_in_view_keeps_its_strip_after_a_scroll() {
    let mut page = page(&lands(40));
    let spacing = least_spacing(page.frame());
    // The row is packed to the least strip, which leaves every card a 24 point square to press
    // even at the row's ends, where the camera's perspective slants the cards' edges most.
    assert_eq!(unreachable(&mut page), 0);
    for notches in [5.0, 13.0] {
        let point = on_card(&mut page, 1);
        page.wheel(point, NOTCH_DOWN * notches);
        page.still();
        let after = least_spacing(page.frame());
        assert!(
            after >= spacing - 0.5,
            "{after} apart after, {spacing} before"
        );
        assert_eq!(unreachable(&mut page), 0);
        let frame = page.frame();
        for (number, index) in row(frame) {
            assert!(
                uncovered(frame, index).is_some(),
                "land {number} is covered"
            );
        }
    }
}

#[test]
fn a_card_slides_to_its_new_place_and_one_coming_into_view_fades_in() {
    let mut page = page(&lands(40));
    let last = *row(page.frame()).last().expect("cards in view");
    let point = on_card(&mut page, 1);
    page.wheel(point, NOTCH_DOWN);
    page.step(ms(60));
    let frame = page.frame();
    let coming = frame
        .resolved
        .iter()
        .find(|element| frame.name(element.name) == format!("Land {}", last.0 + 1))
        .expect("the next land is drawn as it comes into view");
    assert!(
        coming.opacity > 0.0 && coming.opacity < 1.0,
        "{}",
        coming.opacity
    );
    page.still();
    assert_eq!(first(&mut page), 1);
}

#[test]
fn a_row_that_fits_ignores_the_wheel() {
    let mut page = page(&lands(3));
    assert_eq!(count(&mut page, "right"), None);
    let before = row(page.frame());
    let point = on_card(&mut page, 1);
    page.wheel(point, NOTCH_DOWN * 3.0);
    assert!(!page.position().2, "nothing moves");
    assert_eq!(row(page.frame()), before);
}

fn wheel_at(point: Vec2, wheel: Vec2) -> Pointer {
    Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase: PointerPhase::Wheel,
        position: point,
        button: Button::None,
        wheel,
        modifiers: Modifiers::default(),
    }
}

/// `view` with its last land tapped, and the script that taps it.
fn tapping_the_last(view: &PlayerView) -> GameUpdate {
    let mut after = view.clone();
    let object = after.objects.last_mut().expect("a land");
    object.tapped = true;
    let object = object.clone();
    let mut script: Script = ScriptBuilder::default().beat([Act::Tap { object }]).build();
    script.timing = vec![timing(400.0)];
    after.script = Some(script);
    GameUpdate {
        session: beryl_protocol::protocol::SessionId(1),
        view: after,
    }
}

#[test]
fn a_script_played_while_a_row_scrolls_ends_where_placement_rests_each_card() {
    let mut client = Client::new(
        crate::test_data::data(),
        Config {
            elements: 4096,
            draws: 4096,
            name_bytes: 1 << 18,
        },
    );
    client.record();
    client.handle(Input::Surface {
        size: WINDOW,
        scale: 2.0,
        safe_area: Insets::default(),
    });
    let view = lands(40);
    client.handle(Input::Update(Box::new(view.update())));
    let frame = client.frame(0).clone();
    let index = row(&frame)[1].1;
    let rect = uncovered(&frame, index).expect("an uncovered card");
    let point = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    client.handle(Input::Pointer(wheel_at(point, NOTCH_DOWN * 4.0)));
    client.frame(1_000_000);
    // The last land is out of view; a script taps it, and the row scrolls while it plays.
    client.handle(Input::Update(Box::new(tapping_the_last(&view.build()))));
    client.frame(400_000_000);
    client.frame(500_000_000);
    client.handle(Input::Pointer(wheel_at(point, NOTCH_DOWN * 2.0)));
    for at in [550, 600, 700, 900, 1200] {
        client.frame(at * 1_000_000);
    }
    let history = client.history();
    assert!(history.len() >= 2, "{} scripts recorded", history.len());
    for script in history {
        let found = rest_equals_placement(script);
        assert!(found.is_empty(), "{found:?}");
    }
}

/// On the phone layout a crowded lands row shows two cards and ends in the middle of the
/// screen. When an effect moves the row's first lands to its end, out of view, each is seen for
/// its whole flight, as "every step shown" asks: it flies past the cards in view toward the
/// count of the cards off the row's right end. This failed in three ways. The cards off that end
/// rested under the last card in view, so flights there ended under it. Those cards were also
/// drawn there for the whole script as soon as a card before them left and they dropped one
/// layer. And a card already off that end rose in place there as it flew to the row's end.
#[test]
fn a_card_flying_to_the_hidden_end_of_a_compact_row_is_seen() {
    let size = Size {
        width: 393.0,
        height: 759.0,
    };
    // A third of the lands are tapped, as on a stress board, so that the cards differ in width.
    let mut before = lands(12).update();
    for (index, object) in before.view.objects.iter_mut().enumerate() {
        object.tapped = index % 3 == 0;
    }
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(vec![FixtureMessage::game_update(&before)]).into(),
        Side::You,
        crate::COMPACT,
        size,
    )
    .expect("the page opens");
    page.still();
    let shown = row(page.frame()).len();
    assert!(shown < 4, "the phone shows {shown} of 12 lands");

    // The first four lands move to the end of the battlefield, as an effect moves them, one
    // after another: the first from in view and the rest from in view or off the row's right end.
    let moved_count = 4;
    let mut after = before.view.clone();
    let moved: Vec<_> = after.objects[..moved_count].to_vec();
    after.battlefield.rotate_left(moved_count);
    after.objects.rotate_left(moved_count);
    let end = u16::try_from(after.battlefield.len() - 1).expect("a short row");
    let beat: Vec<Step> = moved
        .into_iter()
        .map(|object| {
            step(Act::Fly {
                from: ZoneName::Battlefield,
                why: Why::Effect,
                object,
                at: Some(end),
                item: None,
                origin: None,
            })
        })
        .collect();
    let flight = StepTiming {
        stagger: 30.0,
        ..timing(420.0)
    };
    after.script = Some(testing::script(None, vec![beat], vec![flight; moved_count]));
    let after = GameUpdate {
        session: before.session,
        view: after,
    };

    let mut messages = vec![
        FixtureMessage::game_update(&before),
        FixtureMessage::game_update(&after),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        crate::COMPACT,
        size,
    )
    .expect("the views can be read");
    let unseen: Vec<String> = (page.check_all(Mode::EveryTick).faults.into_iter())
        .filter(|fault| fault.rule == Rule::StepShown)
        .map(|fault| fault.message)
        .collect();
    assert!(unseen.is_empty(), "{unseen:?}");
}

/// A card that a step moves to the hidden end of a crowded row is not drawn there while it
/// waits for a later step of the same script to take it out again. It used to stand drawn at
/// the row's end, over the card in view beside it: on the phone layout, an evoked creature that
/// entered a full row covered the permanent beside it until it was sacrificed.
#[test]
fn a_card_waiting_at_a_crowded_rows_hidden_end_is_not_drawn() {
    let size = Size {
        width: 393.0,
        height: 759.0,
    };
    let mut builder = lands(12);
    let newcomer = {
        let card = builder.card(ZoneName::Hand, Side::You, "Land 12");
        card.types = vec![CardType::Land];
        card.slot
    };
    let before = builder.update();
    let record = |view: &PlayerView| {
        (view.objects.iter())
            .find(|object| object.slot == newcomer)
            .cloned()
            .expect("the newcomer has a record")
    };
    // In the script's view, the newcomer has entered the battlefield and gone to the graveyard.
    let mut after = before.view.clone();
    after.players.you.graveyard.push(newcomer);
    for object in &mut after.objects {
        if object.slot == newcomer {
            object.zone = ZoneName::Graveyard;
        }
    }
    let mut entered = record(&before.view);
    entered.zone = ZoneName::Battlefield;
    let left = record(&after);
    let fly = |from, why, object| {
        step(Act::Fly {
            from,
            why,
            object,
            at: None,
            item: None,
            origin: None,
        })
    };
    let beats = vec![
        vec![fly(ZoneName::Hand, Why::Play, entered)],
        vec![fly(ZoneName::Battlefield, Why::Sacrifice, left)],
    ];
    let waits = StepTiming {
        delay: 600.0,
        ..timing(300.0)
    };
    after.script = Some(testing::script(None, beats, vec![timing(300.0), waits]));
    let after = GameUpdate {
        session: before.session,
        view: after,
    };
    let mut messages = vec![
        FixtureMessage::game_update(&before),
        FixtureMessage::game_update(&after),
    ];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        crate::COMPACT,
        size,
    )
    .expect("the views can be read");
    // In the wait between the two flights: the first lands 150 ms after its middle, and the
    // second sets out 600 ms after that.
    page.seek(&"step:2.0.0+450ms".parse().expect("a moment"))
        .expect("the first flight is a moment of the recording");
    let frame = page.frame();
    let drawn: Vec<String> = (frame.resolved.iter())
        .filter(|element| element.role == Role::Card && element.opacity > 0.0)
        .map(|element| frame.name(element.name).to_owned())
        .filter(|name| name == "Land 12")
        .collect();
    assert!(drawn.is_empty(), "the newcomer is drawn while it waits");
}
