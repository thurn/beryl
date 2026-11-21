//! The bug report sheet, driven through the `Page` test driver: Cmd+B freezes the table and
//! asks the host for its picture, a drag chooses the region, the description and the AI box go
//! to the host with Submit, and the report closes once saved, after which the table plays on
//! from where it froze.

use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, Side, ZoneName};
use beryl_protocol::script::{Act, Table as Played, time_script};
use beryl_scene::Input;
use beryl_scene::agent::{Layout, TreeOptions, tree};
use beryl_scene::command::Request;
use beryl_scene::driver::{Duration, Page, Recording};
use beryl_scene::frame::{Frame, StateBits, Wake};
use beryl_scene::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Rect,
    Size,
};
use beryl_scene::testing::{ScriptBuilder, ViewBuilder};
use beryl_scene::{Client, Config};
use glam::Vec2;

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

const FIELD: &str = "field \"description\"";
const SUBMIT: &str = "button \"Submit\"";

/// A page showing `updates` from your seat in a 1600 by 900 window.
fn page(updates: &[GameUpdate]) -> Page {
    let messages = updates.iter().map(FixtureMessage::game_update).collect();
    Page::new(Recording(messages).into(), Side::You, Layout::Desktop, SIZE)
        .expect("the recording plays")
}

/// The sample table, at rest.
fn sample() -> Page {
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    let mut page = page(&[view.update()]);
    page.still();
    page
}

/// Every request the client has made since the last call.
fn requests(page: &mut Page) -> Vec<Request> {
    std::iter::from_fn(|| page.next_request()).collect()
}

/// The `SaveBug` requests the client has made since the last call to `requests`.
fn saves(page: &mut Page) -> Vec<Request> {
    (requests(page).into_iter())
        .filter(|request| matches!(request, Request::SaveBug { .. }))
        .collect()
}

/// Gives the page the mouse's primary button event `phase` at `at`.
fn mouse(page: &mut Page, phase: PointerPhase, at: Vec2) {
    page.give_input(Input::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position: at,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    }));
}

/// Opens the report with Cmd+B and waits for the region chooser.
fn open(page: &mut Page) {
    requests(page);
    page.chord("Cmd+B");
    page.still();
}

/// The whole window.
fn whole() -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: SIZE.width,
        height: SIZE.height,
    }
}

/// Whether `frame` shows any of the bug report.
fn shows_report(frame: &Frame) -> bool {
    (frame.resolved.iter()).any(|element| frame.name(element.name) == "bug report")
}

#[test]
fn the_two_frames_after_cmd_b_show_nothing_of_the_report() {
    // The host sees `FreezeFrame` after the first frame and reads back the second.
    let mut client = Client::new(Config::LARGEST);
    client.handle(Input::Surface {
        size: SIZE,
        scale: 1.0,
        safe_area: Insets::default(),
    });
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    client.frame(0);
    let command = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    for phase in [KeyPhase::Down, KeyPhase::Up] {
        client.handle(Input::Key(Key {
            code: KeyCode::B,
            phase,
            modifiers: command,
        }));
    }
    for tick in 1..=2 {
        assert!(
            !shows_report(client.frame(tick * 16_000_000)),
            "frame {tick}"
        );
        assert_eq!(
            client.wake(),
            Wake::Now,
            "the host is asked for the next frame"
        );
    }
    assert!(
        shows_report(client.frame(48_000_000)),
        "then the region chooser"
    );
}

#[test]
fn over_a_prompt_the_report_covers_the_whole_window() {
    // The game screen's top element is the prompt bar, no larger than its buttons; the report
    // is anchored to the window's corner rather than laid out inside the bar.
    let mut client = super::flat_layer::client();
    client.handle(super::flat_layer::deciding(super::flat_layer::prompt(
        "Pay {1}{U}.",
    )));
    client.frame(0);
    let command = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    for phase in [KeyPhase::Down, KeyPhase::Up] {
        client.handle(Input::Key(Key {
            code: KeyCode::B,
            phase,
            modifiers: command,
        }));
    }
    let mut printed = String::new();
    for tick in 1..=4 {
        let frame = client.frame(tick * 16_000_000);
        printed = tree(frame, &TreeOptions::default()).expect("a whole tree prints");
    }
    let line = (printed.lines())
        .find(|line| line.contains("region \"bug report\""))
        .unwrap_or_else(|| panic!("the report shows: {printed}"));
    assert!(line.contains(" 0,0 1440x900"), "{line}");
}

#[test]
fn cmd_b_freezes_the_table_then_a_drag_chooses_the_region_and_submit_sends_it() {
    let mut page = sample();
    requests(&mut page);
    page.chord("Cmd+B");
    assert_eq!(requests(&mut page), vec![Request::FreezeFrame]);
    page.still();
    page.find("text \"Drag to select\"");

    mouse(&mut page, PointerPhase::Down, Vec2::new(300.0, 200.0));
    mouse(&mut page, PointerPhase::Move, Vec2::new(500.0, 260.0));
    mouse(&mut page, PointerPhase::Move, Vec2::new(700.0, 400.0));
    let region = page.find("region \"region\"").rect();
    assert_eq!((region.width, region.height), (400.0, 200.0), "{region:?}");
    mouse(&mut page, PointerPhase::Up, Vec2::new(700.0, 400.0));
    page.still();

    assert!(
        page.find(SUBMIT).states().has(StateBits::DISABLED),
        "nothing is described yet"
    );
    page.type_text(FIELD, "the angel never untapped");
    page.still();
    page.press(SUBMIT);
    assert_eq!(
        saves(&mut page),
        vec![Request::SaveBug {
            description: "the angel never untapped".to_owned(),
            ai: false,
            region: Rect {
                x: 300.0,
                y: 200.0,
                width: 400.0,
                height: 200.0,
            },
            window: SIZE,
        }]
    );
    page.find("button \"Saving…\"");
}

#[test]
fn a_click_takes_the_whole_window_and_cmd_return_submits_an_ai_play() {
    let mut page = sample();
    open(&mut page);
    mouse(&mut page, PointerPhase::Down, Vec2::new(800.0, 450.0));
    mouse(&mut page, PointerPhase::Up, Vec2::new(802.0, 451.0));
    page.still();
    page.press("toggle \"AI play\"");
    assert!(
        page.find("toggle \"AI play\"")
            .states()
            .has(StateBits::SELECTED)
    );
    // The description field took the focus as the form opened.
    page.give_input(Input::Text(beryl_scene::input::TextChange::Insert(
        "attacked into a wrath".to_owned(),
    )));
    page.chord("Cmd+Return");
    assert_eq!(
        saves(&mut page),
        vec![Request::SaveBug {
            description: "attacked into a wrath".to_owned(),
            ai: true,
            region: whole(),
            window: SIZE,
        }]
    );
}

#[test]
fn saved_shows_then_closes_and_an_error_lets_the_player_try_again() {
    let mut page = sample();
    open(&mut page);
    page.press("button \"Whole window\"");
    page.still();
    page.type_text(FIELD, "x");
    page.press(SUBMIT);
    page.give_input(Input::BugSaved(Err("The disk is full.".to_owned())));
    page.still();
    page.find("text \"The disk is full.\"");
    assert!(!page.find(SUBMIT).states().has(StateBits::DISABLED));

    page.press(SUBMIT);
    page.give_input(Input::BugSaved(Ok(())));
    page.find("text \"Saved\"");
    page.step(Duration::from_millis(1000));
    assert!(page.try_find("region \"bug report\"").is_err(), "it closed");
}

#[test]
fn escape_or_the_close_button_closes_the_report_at_any_stage() {
    let mut page = sample();
    open(&mut page);
    page.key(KeyCode::Escape);
    assert!(page.try_find("region \"bug report\"").is_err());

    open(&mut page);
    page.key(KeyCode::Space);
    page.still();
    page.find(FIELD);
    page.press("button \"Close\"");
    assert!(page.try_find("region \"bug report\"").is_err());
    // Opened again, the report starts empty.
    open(&mut page);
    page.key(KeyCode::Space);
    page.still();
    assert_eq!(page.field("description").text, "");
}

#[test]
fn the_table_stays_frozen_while_the_report_is_open_and_plays_on_after() {
    let mut builder = ViewBuilder::new();
    let plains = builder.card(ZoneName::Battlefield, Side::You, "Plains");
    plains.types = vec![CardType::Land];
    let slot = plains.slot;
    let mut before = builder.update();
    before.view.script = Some(ScriptBuilder::reset().build());
    let mut after = before.clone();
    let object = (after.view.objects.iter_mut())
        .find(|object| object.slot == slot)
        .expect("the land is in the view");
    object.tapped = true;
    let object = object.clone();
    let mut script = ScriptBuilder::default().beat([Act::Tap { object }]).build();
    script.timing = time_script(&script, &Played::of(&before.view));
    after.view.script = Some(script);

    let mut page = page(&[before, after]);
    let land = "card \"Plains\"";
    let at_rest = page.find(land).rect();
    // The second view comes once the first has rested; into the tap's turn, then frozen there.
    let mut moved = at_rest;
    for _ in 0..400 {
        page.step(Duration::from_millis(10));
        moved = page.find(land).rect();
        if moved != at_rest {
            break;
        }
    }
    assert_ne!(moved, at_rest, "the tap has begun");
    page.chord("Cmd+B");
    let frozen = page.find(land).rect();
    page.step(Duration::from_millis(3000));
    assert_eq!(
        page.find(land).rect(),
        frozen,
        "nothing moves while it is open"
    );
    // The report shows though the table, frozen mid-script, has not settled.
    page.find("text \"Drag to select\"");
    let (_, _, moving) = page.position();
    assert!(!moving, "a frozen table asks for no frames");

    page.key(KeyCode::Escape);
    page.step(Duration::from_millis(1));
    let resumed = page.find(land).rect();
    assert!(
        (resumed.x - frozen.x).abs() < 4.0 && (resumed.width - frozen.width).abs() < 4.0,
        "the tap goes on from where it froze, not from its end: {frozen:?} then {resumed:?}"
    );
    page.still();
    assert_ne!(page.find(land).rect(), frozen, "and plays to its end");
}
