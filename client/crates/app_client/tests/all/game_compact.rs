//! The game screen on the phone layout (docs/beryl-3d.md, "Sizes"; docs/ui-conventions.md, "The
//! phone"): the furniture in the table's bands with no prompt open, the open prompt's pill and
//! dock, the menu that opens the settings with the log and the bug report, and every control
//! inside the safe area, as large as a finger and reachable at each phone size and after a
//! resize.

use app_client::Client;
use app_client::Input;
use app_client::command::SessionCommand;
use app_client::driver::{Duration, Page, Recording};
use beryl_card_records::{TokenMakers, fixture_message};
use beryl_protocol::ipc::FixtureMessage;
use beryl_protocol::prompt::{ButtonId, Click, ClickRequest};
use beryl_protocol::protocol::{CardType, ManaPool, Side, ZoneName};
use beryl_protocol::script::{Act, Table as Played, Tally, time_script};
use beryl_protocol::testing::{ScriptBuilder, ViewBuilder};
use game_table::profiles::Profile;
use game_table::table::Window;
use kit_frame::frame::{Frame, Role};
use kit_probe::agent::checks::{keep_clear, minimum_sizes, on_screen, overlap, reachable};
use kit_probe::agent::text_overlap::text_overlap;
use kit_probe::agent::{Context, Fault};
use kit_ui::input::Input as DeviceInput;
use kit_ui::input::{Insets, PointerPhase, Size};

use super::flat_layer::{centre, deciding, decision, edges, find, pointer, prompt};

/// The three phone windows the interface tests run in.
const PHONES: [(f32, f32); 3] = [(393.0, 759.0), (375.0, 647.0), (430.0, 932.0)];

/// A status bar and a home bar, as the phone interface tests give every phone window.
const SAFE: Insets = Insets {
    top: 44.0,
    right: 0.0,
    bottom: 34.0,
    left: 0.0,
};

const MS: u64 = 1_000_000;

fn window_bands(window: Window) -> Option<game_table::profiles::TableBands> {
    Profile::for_window(&crate::test_data::data().profiles, window).bands()
}

fn window(width: f32, height: f32) -> Window {
    Window {
        size_class: None,
        size: Size { width, height },
        scale: 3.0,
        safe_area: SAFE,
    }
}

fn surface(window: Window) -> Input {
    Input::Device(DeviceInput::Surface {
        size: window.size,
        scale: window.scale,
        safe_area: window.safe_area,
    })
}

/// A client in a phone window of `width` by `height`.
fn compact(width: f32, height: f32) -> Client {
    let mut client = super::flat_layer::client();
    client.handle(surface(window(width, height)));
    client
}

/// The sample table while the opponent acts: the viewer has no decision open.
fn waiting() -> Input {
    Input::Update(Box::new(ViewBuilder::sample().update()))
}

fn has(frame: &Frame, role: Role, name: &str) -> bool {
    (frame.resolved.iter()).any(|element| element.role == role && frame.name(element.name) == name)
}

/// Presses the element of `role` named `name` where the last frame drew it.
fn press(client: &mut Client, now: u64, role: Role, name: &str) {
    let frame = client.frame(now);
    let at = centre(frame, find(frame, role, name));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
}

/// The faults the phone's rules find on the flat layer's elements, leaving out the cards, whose
/// places are the table's.
fn layer_faults(frame: &Frame, window: Window) -> Vec<String> {
    let context = Context {
        size_class: crate::COMPACT,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(crate::COMPACT)
            .checks,
        size: window.size,
        safe_area: window.safe_area,
        at: 0,
    };
    let checks: [fn(&Frame, &Context) -> Vec<Fault>; 4] =
        [on_screen, overlap, reachable, minimum_sizes];
    (checks.iter())
        .flat_map(|check| check(frame, &context))
        .map(|fault| fault.message)
        .filter(|message| !message.starts_with("card "))
        .collect()
}

#[test]
fn idle_furniture_stands_in_the_strips_without_a_prompt() {
    let mut client = compact(393.0, 759.0);
    client.handle(waiting());
    let frame = client.frame(0);
    let bands = window_bands(window(393.0, 759.0)).unwrap();
    for (name, strip) in [
        ("your orb", bands.your_strip),
        ("opponent's orb", bands.opponent_strip),
    ] {
        let orb = centre(frame, find(frame, Role::Region, name));
        assert!((orb.y - strip.middle()).abs() < 0.5, "{name} at {orb}");
        assert!((orb.x - 393.0 / 2.0).abs() < 0.5, "{name} at {orb}");
    }
    let menu = centre(frame, find(frame, Role::Button, "Menu"));
    assert!((menu.y - bands.opponent_strip.middle()).abs() < 0.5);
    let phase = centre(frame, find(frame, Role::Region, "phase"));
    assert!((phase.y - bands.middle.middle()).abs() < 0.5);
    // The desktop's gear, log column and step stops have no place on the phone.
    assert!(!has(frame, Role::Button, "Settings"));
    assert!(!has(frame, Role::Button, "Show log"));
    assert!(!(frame.resolved.iter()).any(|element| element.role == Role::Toggle));
}

#[test]
fn the_dock_stands_in_your_strip_and_sends_the_prompts_decisions() {
    let mut client = compact(375.0, 647.0);
    client.handle(deciding(prompt("Pay {1}{U}")));
    let frame = client.frame(0);
    let bands = window_bands(window(375.0, 647.0)).unwrap();
    let auto = find(frame, Role::Button, "Auto Pay");
    let (_, top, right, bottom) = edges(frame, auto);
    assert!(top >= bands.your_strip.top && bottom <= bands.your_strip.bottom());
    assert!((right - bands.right).abs() < 0.5);
    // The prompt's other button stands over the dock.
    let (_, _, _, cancel_bottom) = edges(frame, find(frame, Role::Button, "Cancel"));
    assert!(cancel_bottom < top);
    press(&mut client, MS, Role::Button, "Cancel");
    assert_eq!(
        client.next_command(),
        Some(SessionCommand::Click(ClickRequest {
            decision: decision(),
            revision: 3,
            click: Click::Button {
                button: ButtonId::Decline
            },
        }))
    );
}

#[test]
fn the_pill_stands_on_the_middle_line_or_clear_of_the_stack() {
    // With the stack empty, the pill is centred on the middle band's line.
    let middle = window_bands(window(393.0, 759.0)).unwrap().middle.middle();
    let mut client = compact(393.0, 759.0);
    client.handle(asked_over(&[]));
    let frame = client.frame(0);
    let pill = centre(frame, find(frame, Role::Text, "Pay {1}{U}"));
    assert!((pill.y - middle).abs() < 1.0, "{pill} against {middle}");

    // A spell on the stack stands on the line, so the pill moves to the nearest height clear of
    // it.
    let mut client = compact(393.0, 759.0);
    client.handle(asked_over(&["Lightning Bolt"]));
    let frame = client.frame(0);
    let pill = edges(frame, find(frame, Role::Text, "Pay {1}{U}"));
    let spell = (frame.resolved.iter())
        .position(|element| {
            element.role == Role::Card && frame.name(element.name) == "Lightning Bolt"
        })
        .expect("the spell on the stack");
    let spell = edges(frame, spell);
    assert!(
        pill.3 <= spell.1 || pill.1 >= spell.3,
        "the pill {pill:?} covers the stack's spell {spell:?}"
    );
}

#[test]
fn stacked_buttons_stand_in_the_middle_band_when_a_permanent_lies_under_them() {
    let size = (375.0, 647.0);
    let bands = window_bands(window(size.0, size.1)).unwrap();
    let mut client = compact(size.0, size.1);
    client.handle(asked_with(&[], &["Relic", "Talisman"]));
    let frame = client.frame(0);
    let cancel = edges(frame, find(frame, Role::Button, "Cancel"));
    let middle = f32::midpoint(cancel.1, cancel.3);
    assert!(
        (middle - bands.middle.middle()).abs() < 1.0,
        "Cancel {cancel:?} is off the middle band {:?}",
        bands.middle
    );
    assert!((cancel.2 - bands.right).abs() < 0.5);
    // The pill is narrowed to the band's left, clear of the button.
    let pill = edges(frame, find(frame, Role::Text, "Pay {1}{U}"));
    assert!(
        pill.2 <= cancel.0,
        "the pill {pill:?} reaches under Cancel {cancel:?}"
    );
    let window = window(size.0, size.1);
    let context = Context {
        size_class: crate::COMPACT,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(crate::COMPACT)
            .checks,
        size: window.size,
        safe_area: window.safe_area,
        at: 0,
    };
    let covered: Vec<String> = (reachable(frame, &context).into_iter())
        .map(|fault| fault.message)
        .collect();
    assert_eq!(covered, Vec::<String>::new());
    assert!(
        frame
            .clearances
            .iter()
            .any(|placement| placement.name == "stacked prompt controls"
                && !placement.avoided.is_empty())
    );
    assert!(keep_clear(frame, &context).is_empty());
}

/// The viewer paying a cost, with a hand of one card and the spells `stack` on the stack, and
/// nothing on the battlefield.
fn asked_over(stack: &[&str]) -> Input {
    asked_with(stack, &[])
}

/// The viewer paying a cost, with a hand of one card, the spells `stack` on the stack and the
/// noncreature, nonland permanents `permanents` on their battlefield.
fn asked_with(stack: &[&str], permanents: &[&str]) -> Input {
    let mut view = ViewBuilder::new();
    view.card(ZoneName::Hand, Side::You, "Island");
    for name in stack {
        view.card(ZoneName::Stack, Side::You, name);
    }
    for name in permanents {
        view.card(ZoneName::Battlefield, Side::You, name);
    }
    let Input::Update(mut update) = deciding(prompt("Pay {1}{U}")) else {
        unreachable!("`deciding` gives an update")
    };
    let built = view.build();
    update.view.objects = built.objects;
    update.view.stack = built.stack;
    update.view.battlefield = built.battlefield;
    update.view.players = built.players;
    Input::Update(update)
}

#[test]
fn the_menu_opens_the_settings_with_the_log_and_the_bug_report() {
    let mut client = compact(393.0, 759.0);
    client.handle(waiting());
    press(&mut client, 0, Role::Button, "Menu");
    let frame = client.frame(MS);
    assert!(has(frame, Role::Button, "Report bug"));
    press(&mut client, MS, Role::Button, "Log");
    let frame = client.frame(2 * MS);
    assert!(has(frame, Role::Button, "Close log"));
    assert!(has(frame, Role::List, "Game log"));
    assert!(
        !has(frame, Role::Button, "Report bug"),
        "the settings closed"
    );
    assert_eq!(
        layer_faults(frame, window(393.0, 759.0)),
        Vec::<String>::new()
    );
    press(&mut client, 2 * MS, Role::Button, "Close log");
    let frame = client.frame(3 * MS);
    assert!(!has(frame, Role::List, "Game log"));
    assert!(has(frame, Role::Button, "Menu"));
}

#[test]
fn a_pile_opens_its_viewer_and_closes_again() {
    let mut view = ViewBuilder::sample();
    for _ in 0..3 {
        view.card(ZoneName::Graveyard, Side::You, "Brainstorm");
    }
    let mut messages = vec![FixtureMessage::game_update(&view.update())];
    messages.insert(0, fixture_message(&messages, TokenMakers::NONE));
    let size = Size {
        width: 393.0,
        height: 759.0,
    };
    let mut page = Page::new(
        crate::test_data::data(),
        Recording(messages).into(),
        Side::You,
        size,
    )
    .expect("the view can be read");
    page.still();
    page.press("pile \"Your graveyard\"");
    page.step(Duration::from_millis(1000));
    page.find("list \"cards\"");
    page.press("button \"close\"");
    page.step(Duration::from_millis(1000));
    assert!(page.try_find("list \"cards\"").is_err());
    page.find("pile \"Your graveyard\"");
}

#[test]
fn every_control_is_on_screen_large_enough_and_reachable_at_every_size_and_on_resize() {
    for (width, height) in PHONES {
        let mut client = compact(width, height);
        client.handle(deciding(prompt("Pay {1}{U}")));
        let frame = client.frame(0);
        assert_eq!(
            layer_faults(frame, window(width, height)),
            Vec::<String>::new(),
            "{width}x{height}"
        );
    }
    // Into a smaller phone, and back to a desktop window and out again, with the prompt open.
    let mut client = compact(393.0, 759.0);
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.frame(0);
    client.handle(surface(window(375.0, 647.0)));
    let frame = client.frame(MS);
    assert_eq!(
        layer_faults(frame, window(375.0, 647.0)),
        Vec::<String>::new()
    );
    assert!(has(frame, Role::Button, "Auto Pay"));
    client.handle(surface(window(1440.0, 900.0)));
    let frame = client.frame(2 * MS);
    assert!(
        has(frame, Role::Button, "Settings"),
        "the desktop's gear is back"
    );
    assert!(!has(frame, Role::Button, "Menu"));
    client.handle(surface(window(430.0, 932.0)));
    let frame = client.frame(3 * MS);
    assert_eq!(
        layer_faults(frame, window(430.0, 932.0)),
        Vec::<String>::new()
    );
}

#[test]
fn the_compact_fps_monitor_leaves_room_for_the_table_and_settings() {
    use app_client::FrameRates;
    use app_client::settings::Settings;

    for (width, height) in PHONES {
        let mut client = compact(width, height);
        client.handle(waiting());
        let normal = {
            let frame = client.frame(0);
            edges(frame, find(frame, Role::Region, "opponent's orb"))
        };
        client.handle(Input::Settings(Settings {
            fps_monitor: true,
            ..Settings::default()
        }));
        client.set_frame_rates(FrameRates {
            current: Some(118),
            low: Some(58),
            target: Some(120),
            cadence: Some(120),
        });
        // The widest tier word, so that the row's room is checked with it.
        client.handle(Input::Device(DeviceInput::Tier(
            kit_ui::input::Tier::Enhanced,
        )));
        let frame = client.frame(MS);
        let (left, top, right, bottom) = edges(frame, find(frame, Role::Region, "FPS monitor"));
        assert!(has(frame, Role::Text, "Enhanced"));
        let (_, orb_top, _, _) = edges(frame, find(frame, Role::Region, "opponent's orb"));

        assert!(left >= SAFE.left && right <= width - SAFE.right);
        assert!(top >= SAFE.top && bottom <= orb_top);
        assert!(layer_faults(frame, window(width, height)).is_empty());
        press(&mut client, 2 * MS, Role::Button, "Menu");
        let frame = client.frame(600 * MS);
        let (_, title_top, _, _) = edges(frame, find(frame, Role::Text, "Settings"));
        assert!(bottom <= title_top);

        client.handle(Input::Settings(Settings::default()));
        press(&mut client, 900 * MS, Role::Button, "close");
        let frame = client.frame(1200 * MS);
        assert!(!has(frame, Role::Region, "FPS monitor"));
        let restored = edges(frame, find(frame, Role::Region, "opponent's orb"));
        assert_eq!(normal, restored);
    }
}

/// A client in the phone window of `width` by `height` after a script has set the viewer's and
/// the opponent's pools to `mana` and ended.
fn client_with_pools(width: f32, height: f32, mana: ManaPool) -> Client {
    let mut client = compact(width, height);
    client.record();
    let mut builder = ViewBuilder::sample();
    builder.script(ScriptBuilder::reset().build());
    let start = builder.update();
    client.handle(Input::Update(Box::new(start.clone())));
    client.frame(0);
    let mut script = ScriptBuilder::default();
    for player in [Side::You, Side::Opponent] {
        script.beat([Act::Count {
            tally: Tally::Pool { player, mana },
        }]);
    }
    let mut script = script.build();
    script.timing = time_script(&script, &Played::of(&start.view));
    let mut after = start;
    after.view.players.you.mana = mana;
    after.view.players.opponent.mana = mana;
    after.view.script = Some(script);
    client.handle(Input::Update(Box::new(after)));
    client.frame(MS);
    let record = &client.history()[1];
    let end = record.start.expect("the script has started")
        + record.steps.last().expect("the script has steps").to;
    client.frame(end + 1_000 * MS);
    client
}

#[test]
fn unspent_mana_stands_clear_of_the_life_totals() {
    for (width, height) in PHONES {
        for mana in [
            ManaPool {
                colorless: 3,
                ..ManaPool::default()
            },
            ManaPool {
                blue: 2,
                green: 1,
                ..ManaPool::default()
            },
        ] {
            let mut client = client_with_pools(width, height, mana);
            let frame = client.frame(60_000 * MS);
            let size = window(width, height);
            let context = Context {
                size_class: crate::COMPACT,
                audit: crate::test_data::data().audit,
                checks: crate::test_data::data()
                    .profiles
                    .for_size_class(crate::COMPACT)
                    .checks,
                size: size.size,
                safe_area: size.safe_area,
                at: 0,
            };
            let faults: Vec<String> = text_overlap(frame, &context)
                .into_iter()
                .map(|fault| fault.message)
                .collect();
            assert_eq!(faults, Vec::<String>::new(), "{width}x{height} {mana:?}");
        }
    }
}

/// A yes-or-no question about the first card of `view`, which holds that card up over a shaded
/// table.
fn asked_about_a_card(view: &mut ViewBuilder) -> Input {
    let card = view.card(ZoneName::Graveyard, Side::You, "Brainstorm").slot;
    let question = beryl_protocol::testing::prompt(
        beryl_protocol::prompt::Purpose::Move,
        "Return Brainstorm to your hand?",
        vec![
            super::flat_layer::prompt_button(ButtonId::Option { index: 0 }, "Yes", true),
            super::flat_layer::prompt_button(ButtonId::Option { index: 1 }, "No", false),
        ],
        beryl_protocol::prompt::ShapeBody::YesNo {
            about: vec![card],
            places: None,
        },
    );
    let Input::Update(mut update) = deciding(question) else {
        unreachable!("`deciding` gives an update")
    };
    let built = view.build();
    update.view.objects = built.objects;
    update.view.battlefield = built.battlefield;
    update.view.players = built.players;
    Input::Update(update)
}

/// Whether the frame has a count of how many cards of the viewer's lands row are off its end.
fn has_lands_count(frame: &Frame) -> bool {
    (frame.resolved.iter()).any(|element| frame.name(element.name).starts_with("Your lands right"))
}

#[test]
fn a_prompt_that_holds_cards_up_hides_the_rows_counts() {
    let shown = window(375.0, 647.0);
    let mut client = compact(375.0, 647.0);
    let mut view = ViewBuilder::new();
    // Lands of one name stack, so each has a name of its own for the row to run past the screen.
    for number in 0..20 {
        let name = format!("Land {number}");
        view.card(ZoneName::Battlefield, Side::You, &name).types = vec![CardType::Land];
    }
    client.handle(Input::Update(Box::new(beryl_protocol::GameUpdate {
        session: beryl_protocol::protocol::SessionId(1),
        view: view.build(),
    })));
    client.frame(0);
    assert!(has_lands_count(client.frame(5_000 * MS)), "the row scrolls");

    client.handle(asked_about_a_card(&mut view));
    client.frame(2 * MS);
    let frame = client.frame(5_000 * MS);
    assert!(
        has(frame, Role::Region, "shade"),
        "the prompt holds a card up"
    );
    assert!(!has_lands_count(frame));
    let context = Context {
        size_class: crate::COMPACT,
        audit: crate::test_data::data().audit,
        checks: crate::test_data::data()
            .profiles
            .for_size_class(crate::COMPACT)
            .checks,
        size: shown.size,
        safe_area: shown.safe_area,
        at: 0,
    };
    assert!(kit_probe::agent::text_overlap::text_overlap(frame, &context).is_empty());
}
