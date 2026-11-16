//! The widgets of the flat layer (docs/beryl-3d.md, "The flat layer"): buttons drawn in their
//! states, a label that shrinks to fit its button, the icon button's size, and a slider that a
//! drag sets.

use beryl_scene::agent::checks::{phone_sizes, text_fits};
use beryl_scene::agent::{Layout, Rule};
use beryl_scene::command::Request;
use beryl_scene::element::{Press, row};
use beryl_scene::frame::{Frame, Material, Role, StateBits};
use beryl_scene::input::{IconCell, Input, Key, KeyCode, KeyPhase, Modifiers, PointerPhase};
use beryl_scene::layout::Layer;
use beryl_scene::settings::Setting;
use beryl_scene::tokens::{color, size, type_style};
use beryl_scene::widgets::{self, icons};
use glam::Vec2;

use super::flat_layer::{
    centre, client, context, deciding, edges, fill_glyphs, find, pointer, prompt, write_layer,
};

/// When the prompt bar has faded in, in the nanoseconds `Client::frame` takes: before then its
/// colors are faded.
const OPEN: u64 = 500_000_000;

/// The panel records of `frame`, in the order they were drawn.
fn panels(frame: &Frame) -> Vec<[u8; 4]> {
    (frame.draws.iter())
        .filter(|draw| draw.material == Material::Panel as u8)
        .map(|draw| draw.tint)
        .collect()
}

#[test]
fn a_disabled_button_is_drawn_faint_and_a_press_on_it_sends_nothing() {
    let mut client = client();
    let mut view = prompt("Pay {1}{U}");
    view.buttons[1].enabled = false;
    client.handle(deciding(view));
    client.frame(0);
    fill_glyphs(&mut client);
    let frame = client.frame(OPEN);
    let cancel = find(frame, Role::Button, "Cancel") as u32;
    assert!(
        frame.resolved[cancel as usize]
            .state
            .has(StateBits::DISABLED)
    );
    // The flat stone face and dim rim of `.button:disabled`, whatever the button's own style.
    assert!(panels(frame).contains(&color::DISABLED));
    // "Cancel" is six glyphs in the faint type color, and the enabled "Auto Pay" has none.
    let faint = (frame.draws.iter())
        .filter(|draw| draw.material == Material::Glyph as u8 && draw.tint == color::TEXT_FAINT)
        .count();
    assert_eq!(faint, 6);
    let at = centre(frame, cancel as usize);
    client.handle(pointer(PointerPhase::Down, at));
    client.handle(pointer(PointerPhase::Up, at));
    assert_eq!(client.next_command(), None);
}

#[test]
fn a_primary_button_is_drawn_on_the_primary_color() {
    let mut client = client();
    client.handle(deciding(prompt("Pay {1}{U}")));
    client.frame(0);
    let frame = client.frame(OPEN);
    let panels = panels(frame);
    // "Auto Pay" is the prompt's primary button; "Cancel" is drawn as an ordinary one.
    assert_eq!(
        panels
            .iter()
            .filter(|&&tint| tint == color::PRIMARY)
            .count(),
        1
    );
    assert_eq!(
        panels
            .iter()
            .filter(|&&tint| tint == color::SECONDARY)
            .count(),
        1
    );
}

#[test]
fn a_label_too_long_for_its_button_shrinks_until_it_fits() {
    let label = "Cast Sublime Epiphany for its full cost";
    let mut layer = Layer::new();
    layer.show(Some(
        row()
            .name("dock")
            .child(widgets::primary(label).width(size::BUTTON_WIDTH)),
    ));
    let frame = write_layer(&mut layer, true);
    assert_eq!(text_fits(&frame, &context()), Vec::new());
    let lines: Vec<_> = frame.lines.iter().collect();
    assert_eq!(lines.len(), 1, "the label stays on one line");
    assert!(
        lines[0].size < type_style::BUTTON.size,
        "shrunk from {} to {}",
        type_style::BUTTON.size,
        lines[0].size
    );
    let button = find(&frame, Role::Button, label);
    let (left, _, right, _) = edges(&frame, button);
    assert!((right - left - size::BUTTON_WIDTH).abs() < 0.01);
}

#[test]
fn an_icon_button_is_44_points_square_and_passes_the_phone_sizes_check() {
    let mut layer = Layer::new();
    layer.icons_mut().add(
        0,
        &[IconCell {
            icon: icons::GEAR,
            size: 22,
            rect: [0, 0, 44, 44],
        }],
    );
    layer.show(Some(row().name("corner").child(
        widgets::icon_button(icons::GEAR, "Settings").on_press(Press::Sheet(None)),
    )));
    let frame = write_layer(&mut layer, true);
    let gear = find(&frame, Role::Button, "Settings");
    let (left, top, right, bottom) = edges(&frame, gear);
    assert!((right - left - 44.0).abs() < 0.01 && (bottom - top - 44.0).abs() < 0.01);
    let phone = beryl_scene::agent::Context {
        layout: Layout::Phone,
        ..context()
    };
    assert_eq!(phone_sizes(&frame, &phone), Vec::new());
    // The glyph is drawn 22 points square in the dim text color.
    let icon: Vec<_> = (frame.draws.iter())
        .filter(|draw| draw.material == Material::Icon as u8)
        .collect();
    assert_eq!(icon.len(), 1);
    assert_eq!(icon[0].tint, color::TEXT_DIM);
    // A button too small for a finger fails it.
    let mut layer = Layer::new();
    layer.show(Some(
        row().name("corner").child(
            widgets::icon_button(icons::GEAR, "Settings")
                .width(30.0)
                .on_press(Press::Sheet(None)),
        ),
    ));
    let frame = write_layer(&mut layer, true);
    let faults = phone_sizes(&frame, &phone);
    assert_eq!(faults.len(), 1);
    assert_eq!(faults[0].rule, Rule::PhoneSizes);
}

#[test]
fn dragging_a_slider_to_its_middle_sets_the_level_to_50() {
    let mut client = client();
    client.frame(0);
    let music = Setting::Music(35);
    (client.layer_mut()).show(Some(
        row().name("sound").child(widgets::slider("Music", music)),
    ));
    let frame = client.frame(1);
    let slider = find(frame, Role::Slider, "Music");
    let (left, _, right, _) = edges(frame, slider);
    let middle = centre(frame, slider);
    // Going down on the thumb, at 35, changes nothing.
    let thumb = size::SLIDER_THUMB;
    let at_35 = Vec2::new(left + thumb / 2.0 + (right - left - thumb) * 0.35, middle.y);
    client.handle(pointer(PointerPhase::Down, at_35));
    assert_eq!(client.next_request(), None);
    client.handle(pointer(PointerPhase::Move, middle));
    client.handle(pointer(PointerPhase::Up, middle));
    let saved = std::iter::from_fn(|| client.next_request()).find_map(|request| match request {
        Request::SaveSettings(settings) => Some(settings.music),
        _ => None,
    });
    assert_eq!(saved, Some(50));
    assert_eq!(client.next_command(), None);
}

#[test]
fn a_focused_slider_moves_by_5_under_the_arrow_keys_and_is_drawn_held_while_pressed() {
    let mut client = client();
    client.frame(0);
    (client.layer_mut()).show(Some(
        row()
            .name("sound")
            .child(widgets::slider("Effects", Setting::Effects(100))),
    ));
    let frame = client.frame(1);
    let slider = find(frame, Role::Slider, "Effects");
    let (_, _, right, _) = edges(frame, slider);
    let end = Vec2::new(right, centre(frame, slider).y);
    // A press at the end, where the level already is, takes the focus and holds the slider.
    client.handle(pointer(PointerPhase::Down, end));
    let frame = client.frame(2);
    let state = frame.resolved[find(frame, Role::Slider, "Effects")].state;
    assert!(state.has(StateBits::FOCUSED) && state.has(StateBits::HELD));
    assert!(state.has(StateBits::HOVERED));
    client.handle(pointer(PointerPhase::Up, end));
    assert_eq!(client.next_request(), None);
    let arrow = |code| {
        Input::Key(Key {
            code,
            phase: KeyPhase::Down,
            modifiers: Modifiers::default(),
        })
    };
    // At 100 the right key has nowhere to go; the left key takes the level down a step.
    client.handle(arrow(KeyCode::ArrowRight));
    assert_eq!(client.next_request(), None);
    client.handle(arrow(KeyCode::ArrowLeft));
    assert_eq!(
        client.next_request(),
        Some(Request::SaveSettings(beryl_scene::settings::Settings {
            effects: 95,
            ..beryl_scene::settings::Settings::default()
        }))
    );
}
