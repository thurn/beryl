//! The settings sheet (docs/beryl-3d.md, "The flat layer"): the gear that opens it, the music
//! slider that saves a level, concede only in a game, and Escape that closes it.

use beryl_scene::Client;
use beryl_scene::command::Request;
use beryl_scene::element::Press;
use beryl_scene::frame::{Role, StateBits};
use beryl_scene::input::{Input, Key, KeyCode, KeyPhase, Modifiers, PointerPhase};
use beryl_scene::screens::Screen;
use beryl_scene::tokens::size;
use glam::Vec2;

use super::flat_layer::{centre, client, deciding, edges, find, pointer, prompt};

/// The names of the buttons and sliders the client's latest frame shows.
fn controls(client: &mut Client) -> Vec<String> {
    let frame = client.frame(1);
    frame
        .resolved
        .iter()
        .filter(|element| matches!(element.role, Role::Button | Role::Slider | Role::Toggle))
        .map(|element| frame.name(element.name).to_owned())
        .collect()
}

fn click(client: &mut Client, role: Role, name: &str) {
    let frame = client.frame(1);
    let at = centre(frame, find(frame, role, name));
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
}

fn in_game() -> Client {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client
}

#[test]
fn the_gear_opens_the_settings_sheet() {
    let mut client = in_game();
    assert!(!controls(&mut client).contains(&"Music".to_owned()));
    click(&mut client, Role::Button, "Settings");
    let names = controls(&mut client);
    for name in [
        "Music",
        "Effects",
        "Mute",
        "Full control",
        "Concede",
        "close",
    ] {
        assert!(
            names.contains(&name.to_owned()),
            "{name} is missing: {names:?}"
        );
    }
}

#[test]
fn dragging_the_music_slider_saves_the_new_level() {
    let mut client = in_game();
    client.press(&Press::Sheet(Some(beryl_scene::screens::Sheet::Settings)));
    let frame = client.frame(1);
    let slider = find(frame, Role::Slider, "Music");
    let (left, _, right, _) = edges(frame, slider);
    let y = centre(frame, slider).y;
    // The right end of the track is level 100; the music starts at 35.
    let end = Vec2::new(right - size::SLIDER_THUMB / 2.0, y);
    client.handle(pointer(PointerPhase::Down, Vec2::new(left + 60.0, y)));
    client.handle(pointer(PointerPhase::Move, end));
    client.handle(pointer(PointerPhase::Up, end));
    let saved = std::iter::from_fn(|| client.next_request())
        .filter_map(|request| match request {
            Request::SaveSettings(settings) => Some(settings.music),
            _ => None,
        })
        .last();
    assert_eq!(saved, Some(100));
}

#[test]
fn concede_asks_to_confirm_and_closing_the_sheet_puts_it_back() {
    use beryl_scene::command::SessionCommand;
    let concessions = |client: &mut Client| {
        std::iter::from_fn(|| client.next_command())
            .filter(|command| matches!(command, SessionCommand::Concede(_)))
            .count()
    };
    let mut client = in_game();
    click(&mut client, Role::Button, "Settings");
    click(&mut client, Role::Button, "Concede");
    // A first press, perhaps a slip, only asks.
    assert_eq!(concessions(&mut client), 0);
    let names = controls(&mut client);
    assert!(names.contains(&"Confirm Concede".to_owned()), "{names:?}");

    // Closing the sheet puts the confirmation back.
    client.press(&Press::Sheet(None));
    click(&mut client, Role::Button, "Settings");
    let names = controls(&mut client);
    assert!(names.contains(&"Concede".to_owned()), "{names:?}");

    click(&mut client, Role::Button, "Concede");
    click(&mut client, Role::Button, "Confirm Concede");
    assert_eq!(concessions(&mut client), 1);
    assert!(
        !controls(&mut client).contains(&"Music".to_owned()),
        "the sheet closed"
    );
}

#[test]
fn concede_is_absent_on_the_start_screen() {
    let mut client = in_game();
    client.press(&Press::Open(Screen::Start));
    client.press(&Press::Sheet(Some(beryl_scene::screens::Sheet::Settings)));
    let names = controls(&mut client);
    assert!(names.contains(&"Music".to_owned()));
    assert!(!names.contains(&"Concede".to_owned()), "{names:?}");
}

#[test]
fn escape_closes_the_sheet() {
    let mut client = in_game();
    click(&mut client, Role::Button, "Settings");
    assert!(controls(&mut client).contains(&"Music".to_owned()));
    client.handle(Input::Key(Key {
        code: KeyCode::Escape,
        phase: KeyPhase::Down,
        modifiers: Modifiers::default(),
    }));
    assert!(!controls(&mut client).contains(&"Music".to_owned()));
}

/// The settings the client last asked to save, if it asked.
fn saved(client: &mut Client) -> Option<beryl_scene::settings::Settings> {
    std::iter::from_fn(|| client.next_request())
        .filter_map(|request| match request {
            Request::SaveSettings(settings) => Some(settings),
            _ => None,
        })
        .last()
}

#[test]
fn the_graphics_row_shows_the_saved_value() {
    use beryl_scene::settings::{Graphics, Setting};
    let mut client = in_game();
    client.press(&Press::Setting(Setting::Graphics(Graphics::Enhanced)));
    client.press(&Press::Sheet(Some(beryl_scene::screens::Sheet::Settings)));
    let frame = client.frame(1);
    let selected = |name: &str| {
        frame.resolved[find(frame, Role::Button, name)]
            .state
            .has(StateBits::SELECTED)
    };
    assert!(selected("Enhanced"));
    assert!(!selected("Automatic"));
    assert!(!selected("Baseline"));
}

#[test]
fn choosing_baseline_saves_it() {
    use beryl_scene::settings::Graphics;
    let mut client = in_game();
    click(&mut client, Role::Button, "Settings");
    click(&mut client, Role::Button, "Baseline");
    let settings = saved(&mut client).expect("a save");
    assert_eq!(settings.graphics, Graphics::Baseline);
}
