//! The bug report sheet, driven through the `Page` test driver: Cmd+B or Ctrl+B freezes the
//! table and asks the host for its picture. A drag chooses the region, and Submit sends the
//! description and the AI box to the host. Once saved, the report closes and the table plays
//! on from where it froze.

use app_client::Input;
use app_client::command::Request;
use app_client::driver::{Duration, Page, Recording};
use app_client::{Client, Config};
use beryl_protocol::GameUpdate;
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::protocol::{CardType, Side, Status, ZoneName};
use beryl_protocol::script::{Act, Table as Played, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use glam::Vec2;
use kit_frame::frame::{Frame, Role, StateBits, Wake};
use kit_probe::agent::{Rule, TreeOptions, locate, tree};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Rect,
    Size,
};

const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};

const FIELD: &str = "field \"description\"";
const SUBMIT: &str = "button \"Submit\"";

/// A page showing `updates` from your seat in a 1600 by 900 window.
fn page(updates: &[GameUpdate]) -> Page {
    let messages = updates.iter().map(FixtureMessage::game_update).collect();
    Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        SIZE,
    )
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
    page.give_input(Input::Device(DeviceInput::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position: at,
        button: Button::Primary,
        wheel: Vec2::ZERO,
        modifiers: Modifiers::default(),
    })));
}

/// Opens the report with Cmd+B and waits for the region chooser.
fn open(page: &mut Page) {
    requests(page);
    page.chord("Cmd+B");
    page.still();
}

#[test]
fn the_report_shades_both_life_totals_without_hiding_them() {
    let mut page = sample();
    page.set_looking(true);
    let totals = ["player \"You 20\"", "player \"Opponent 20\""];
    let before = totals.map(|total| page.find(total).rect());

    let check = |page: &mut Page, shade| {
        for (total, rect) in totals.into_iter().zip(before) {
            assert_eq!(page.find(total).rect(), rect);
        }
        let frame = page.frame();
        let shade = locate(frame, shade).expect("the report draws its shade");
        let shade_draw = frame
            .owners
            .iter()
            .position(|&owner| owner == shade)
            .expect("the shade is painted");
        for total in totals {
            let total = locate(frame, total).expect("the frozen life total is present");
            let last_glyph = frame
                .owners
                .iter()
                .rposition(|&owner| owner == total)
                .expect("the life total is painted");
            assert!(last_glyph < shade_draw, "the shade paints over the total");
        }
    };
    open(&mut page);
    check(&mut page, "region \"shade\"");
    let input_faults: Vec<_> = page
        .faults()
        .into_iter()
        .filter(|fault| matches!(fault.rule, Rule::Overlap | Rule::Reachable))
        .collect();
    assert!(input_faults.is_empty(), "{input_faults:#?}");

    let [local, _] = before;
    let start = Vec2::new(local.x - 12.0, local.y - 12.0);
    let end = Vec2::new(local.x + local.width + 12.0, local.y + local.height + 12.0);
    mouse(&mut page, PointerPhase::Down, start);
    mouse(&mut page, PointerPhase::Move, end);
    mouse(&mut page, PointerPhase::Up, end);
    page.still();
    page.find(FIELD);
    check(&mut page, "region \"shade above\"");
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

/// A discard held up while the player decides whether to exile it.
fn exile_prompt() -> Page {
    use super::flat_layer::prompt_button;
    use beryl_protocol::prompt::{ButtonId, Purpose, ShapeBody};
    use beryl_protocol::protocol::{Kind, StackItem, Status};
    use beryl_protocol::testing::prompt;

    let mut view = ViewBuilder::sample();
    let card = view.card(ZoneName::Graveyard, Side::You, "Oust").slot;
    let converter = view
        .card(ZoneName::Battlefield, Side::You, "Currency Converter")
        .slot;
    view.script(ScriptBuilder::reset().build());
    let mut update = view.update();
    let Input::Update(deciding) = super::flat_layer::deciding(prompt(
        Purpose::Move,
        "Exile that card from your graveyard?",
        vec![
            prompt_button(ButtonId::Option { index: 0 }, "Take Action", true),
            prompt_button(ButtonId::Option { index: 1 }, "Decline", false),
        ],
        ShapeBody::YesNo {
            about: vec![card],
            places: None,
        },
    )) else {
        unreachable!("the sample prompt is a game update");
    };
    update.view.status = deciding.view.status;
    if let Status::Deciding { decision } = &mut update.view.status {
        decision.kind = Kind::YesNo;
        decision.cancellable = false;
        decision.resolving = Some(StackItem::Ability {
            source: Some(converter),
            ability: 0,
            granted: false,
            name: "Currency Converter".to_owned(),
            triggered: true,
            taps_to_pay: None,
            nth: 0,
            text: None,
            controller: Side::You,
            manual: None,
            targets: Vec::new(),
            x: 0,
        });
    }
    let mut page = page(&[update]);
    page.still();
    page
}

#[test]
fn the_exile_question_keeps_the_action_dock_width() {
    let mut page = exile_prompt();
    page.find("ability \"Currency Converter\"");
    let title = page
        .find("text \"Exile that card from your graveyard?\"")
        .rect();
    let decline = page.find("button \"Decline\"").rect();
    let action = page.find("button \"Take Action\"").rect();
    assert!((action.width - 250.0).abs() < 0.01, "{action:?}");
    assert!((decline.width - action.width).abs() < 0.01, "{decline:?}");
    assert!((title.width - action.width).abs() < 0.01, "{title:?}");
    assert!((title.height - 59.8).abs() < 0.1, "{title:?}");
}

#[test]
fn the_bug_report_paints_over_a_card_held_up_by_the_prompt() {
    let mut page = exile_prompt();
    let card = "card \"Oust\"";
    let before = page.find(card).rect();

    page.set_looking(true);
    open(&mut page);
    assert_eq!(page.find(card).rect(), before);
    let frame = page.frame();
    let card = super::flat_layer::find(frame, Role::Card, "Oust");
    // The tray shade has the same name. Use the shade belonging to the bug reporter.
    let shade = (frame.resolved.iter().enumerate())
        .rfind(|(_, node)| frame.name(node.name) == "shade")
        .expect("the reporter draws a shade")
        .0;
    assert_drawn_over(frame, shade, card);

    page.press("button \"Whole window\"");
    page.still();
    let frame = page.frame();
    let card = super::flat_layer::find(frame, Role::Card, "Oust");
    let panel = super::flat_layer::find(frame, Role::Region, "Report bug");
    assert_drawn_over(frame, panel, card);
    page.find(FIELD);
}

fn assert_drawn_over(frame: &Frame, front: usize, behind: usize) {
    let front = u32::try_from(front).unwrap();
    let behind = u32::try_from(behind).unwrap();
    let start = frame
        .owners
        .iter()
        .position(|&owner| owner == front)
        .unwrap();
    let end = frame
        .owners
        .iter()
        .rposition(|&owner| owner == behind)
        .unwrap();
    assert!(
        start > end,
        "the reporter must paint after every record of the held card"
    );
    let order: Vec<_> = frame.paint_order().collect();
    let position = |owner| order.iter().position(|&index| index == owner).unwrap();
    assert!(
        position(front) > position(behind),
        "the probe must report the same paint order"
    );
}

#[test]
fn the_two_frames_after_cmd_b_show_nothing_of_the_report() {
    // The host sees `FreezeFrame` after the first frame and reads back the second.
    let mut client = Client::new(crate::test_data::data(), Config::LARGEST);
    client.handle(Input::Device(DeviceInput::Surface {
        size: SIZE,
        scale: 1.0,
        safe_area: Insets::default(),
    }));
    let mut view = ViewBuilder::sample();
    view.script(ScriptBuilder::reset().build());
    client.handle(Input::Update(Box::new(view.update())));
    client.frame(0);
    let command = Modifiers {
        command: true,
        ..Modifiers::default()
    };
    for phase in [KeyPhase::Down, KeyPhase::Up] {
        client.handle(Input::Device(DeviceInput::Key(Key {
            code: KeyCode::B,
            phase,
            modifiers: command,
        })));
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
fn the_opening_header_stays_clear_in_the_bug_report_picture() {
    use super::{elements::key, flat_layer::find};
    use kit_base::tokens::ratio::OPENING_HUD_OPACITY;
    use kit_ui::input::Window;

    let mut view = ViewBuilder::new();
    for _ in 0..7 {
        view.card(ZoneName::Hand, Side::You, "Island");
    }
    view.script(ScriptBuilder::reset().build());
    let mut opening = view.update();
    opening.view.turn.number = 0;
    opening.view.turn.starting = Some(Side::Opponent);
    opening.view.status = Status::Thinking;
    let mut page = Page::in_window(
        crate::test_data::data(),
        Recording(vec![FixtureMessage::game_update(&opening)]).into(),
        Side::You,
        Window {
            size: Size {
                width: 1728.0,
                height: 1117.0,
            },
            scale: 2.0,
            ..Window::default()
        },
    )
    .expect("the opening plays");
    page.still();
    let now = page.now();
    let (client, _) = page.host_assets();

    let check_header = |frame: &Frame| {
        let printed = tree(frame, &TreeOptions::default()).expect("a whole tree prints");
        assert!(printed.contains("text \"On the draw\""), "{printed}");
        assert!(printed.contains("text \"Opening hand\""), "{printed}");
        assert!(printed.contains("player \"You 20\""), "{printed}");
        assert!(!printed.contains("player \"Opponent 20\""), "{printed}");
        let coin = &frame.resolved[find(frame, Role::Region, "opening coin")];
        let label = &frame.resolved[find(frame, Role::Text, "On the draw")];
        let coin_right = coin
            .corners
            .iter()
            .map(|corner| corner.x)
            .fold(f32::MIN, f32::max);
        let label_left = label
            .corners
            .iter()
            .map(|corner| corner.x)
            .fold(f32::MAX, f32::min);
        assert!(coin_right < label_left, "{printed}");
        assert!(!printed.contains("text \"1\""), "{printed}");
        let orb = &frame.resolved[find(frame, Role::Region, "opponent's orb")];
        let total = &frame.resolved[find(frame, Role::Player, "You 20")];
        assert!((orb.opacity - OPENING_HUD_OPACITY).abs() < f32::EPSILON);
        assert!(total.opacity < OPENING_HUD_OPACITY);
    };
    check_header(client.frame(now));
    client.handle(key(
        KeyCode::B,
        Modifiers {
            command: true,
            ..Modifiers::default()
        },
    ));
    // The host reads the second frame. Both frames should retain the same HUD visibility.
    check_header(client.frame(now + 16_000_000));
    check_header(client.frame(now + 32_000_000));
    client.handle(key(KeyCode::Escape, Modifiers::default()));
    check_header(client.frame(now + 48_000_000));

    client.handle(Input::Update(Box::new(view.update())));
    client.frame(now + 64_000_000);
    let frame = client.frame(now + 80_000_000);
    let printed = tree(frame, &TreeOptions::default()).expect("a whole tree prints");
    assert!(!printed.contains("text \"Opening hand\""), "{printed}");
    assert!(printed.contains("player \"Opponent 20\""), "{printed}");
    let orb = &frame.resolved[find(frame, Role::Region, "opponent's orb")];
    assert!((orb.opacity - 1.0).abs() < f32::EPSILON);
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
        client.handle(Input::Device(DeviceInput::Key(Key {
            code: KeyCode::B,
            phase,
            modifiers: command,
        })));
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
fn the_report_keeps_mouse_and_keyboard_input_off_the_frozen_prompt() {
    use super::elements::key;
    use super::flat_layer::{centre, client, deciding, find, pointer, prompt};

    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let cancel = centre(frame, find(frame, Role::Button, "Cancel"));
    client.handle(key(
        KeyCode::B,
        Modifiers {
            command: true,
            ..Modifiers::default()
        },
    ));
    client.frame(1);
    client.frame(2);
    client.frame(3);
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    let frame = client.frame(4);
    find(frame, Role::Field, "description");

    client.handle(pointer(PointerPhase::Down, cancel));
    client.handle(pointer(PointerPhase::Up, cancel));
    assert_eq!(client.next_command(), None, "the report owns the click");

    for now in 5..15 {
        client.handle(key(KeyCode::Tab, Modifiers::default()));
        let frame = client.frame(now);
        let cancel = &frame.resolved[find(frame, Role::Button, "Cancel")];
        assert!(!cancel.state.has(StateBits::FOCUSED), "the report owns Tab");
    }
}

#[test]
fn editing_the_report_over_a_table_menu_keeps_focus_in_the_description() {
    use super::elements::key;
    use super::manual::{client_at_table, middle, pointer, table_view};
    use kit_ui::input::TextChange;

    let (mut client, frame) = client_at_table(&table_view());
    let card = middle(&frame, Role::Card, "Serra Angel");
    client.handle(pointer(PointerPhase::Down, card, Button::Secondary));
    let frame = client.frame(1);
    assert!(
        frame
            .resolved
            .iter()
            .any(|node| node.role == Role::MenuItem)
    );

    client.handle(key(
        KeyCode::B,
        Modifiers {
            shift: true,
            control: true,
            ..Modifiers::default()
        },
    ));
    client.frame(2);
    client.frame(3);
    client.frame(4);
    client.handle(key(KeyCode::Enter, Modifiers::default()));
    for words in ["Could ", "not untap"] {
        client.handle(Input::Device(DeviceInput::Text(TextChange::Insert(
            words.to_owned(),
        ))));
    }
    assert_eq!(client.field("description").text, "Could not untap");
}

#[test]
fn a_targeting_arrow_stays_frozen_during_region_selection() {
    use game_probe::story::{Body, Story};

    let catalog = app_client::stories::embedded();
    let source = catalog
        .iter()
        .find(|source| source.name == "story/app_screens/bug-report-over-game")
        .expect("the reporter story");
    let story = Story::parse(&source.text).expect("the reporter story parses");
    let Body::Capture { capture, .. } = story.body else {
        panic!("the embedded reporter story is a capture");
    };
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(capture.messages).into(),
        Side::You,
        SIZE,
    )
    .expect("the reporter story plays");
    page.still();

    let aim = |page: &mut Page| {
        let frame = page.frame();
        let index = locate(frame, "arrow \"hostile-target\"").expect("the aim is drawn");
        frame.resolved[index as usize].corners
    };
    let before = Vec2::new(800.0, 670.0);
    page.move_to(before);
    let frozen = aim(&mut page);
    assert_eq!(frozen[3], before, "the live arrow aims at the mouse");
    let source = "card \"Jace, Wielder of Mysteries\"";
    let card = page.find(source).rect();

    open(&mut page);
    page.find("text \"Drag to select\"");
    assert_eq!(aim(&mut page), frozen, "opening the report keeps the aim");
    let start = Vec2::new(300.0, 200.0);
    mouse(&mut page, PointerPhase::Down, start);
    for end in [Vec2::new(500.0, 260.0), Vec2::new(700.0, 400.0)] {
        mouse(&mut page, PointerPhase::Move, end);
        assert_eq!(
            page.find("region \"region\"").rect(),
            Rect {
                x: start.x,
                y: start.y,
                width: end.x - start.x,
                height: end.y - start.y,
            },
            "the selection follows the mouse"
        );
        assert_eq!(page.find(source).rect(), card, "the table stays in place");
        assert_eq!(aim(&mut page), frozen, "the arrow keeps its whole curve");
    }

    page.key(KeyCode::Escape);
    assert!(page.try_find("region \"bug report\"").is_err());
    page.find("text \"Choose a target.\"");
    let after = Vec2::new(800.0, 135.0);
    page.move_to(after);
    assert_eq!(aim(&mut page)[3], after, "the live arrow tracks again");
}

#[test]
fn ctrl_b_freezes_the_table_then_a_drag_chooses_the_region_and_submit_sends_it() {
    let mut page = sample();
    requests(&mut page);
    page.chord("Ctrl+B");
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
    page.give_input(Input::Device(DeviceInput::Text(
        kit_ui::input::TextChange::Insert("attacked into a wrath".to_owned()),
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
