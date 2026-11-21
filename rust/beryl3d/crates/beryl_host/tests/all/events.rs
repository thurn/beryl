//! Each winit event the desktop host takes becomes the `Input` docs/beryl-3d.md's "Hosts"
//! gives, in logical points, on a window at 2 pixels per point.

use app_client::Input;
use beryl_host::{LINE_POINTS, Translator, is_fullscreen_key};
use glam::Vec2;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Size,
    TextChange,
};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{
    DeviceId, ElementState, Ime, MouseButton, MouseScrollDelta, Touch, TouchPhase, WindowEvent,
};
use winit::keyboard::{KeyCode as WinitKey, ModifiersState, NativeKeyCode, PhysicalKey};

const DEVICE: DeviceId = DeviceId::dummy();

fn translator() -> Translator {
    Translator::new(PhysicalSize::new(2880, 1800), 2.0)
}

fn mouse(phase: PointerPhase, position: Vec2, button: Button, wheel: Vec2) -> Input {
    Input::Pointer(Pointer {
        id: 0,
        kind: PointerKind::Mouse,
        phase,
        position,
        button,
        wheel,
        modifiers: Modifiers::default(),
    })
}

fn surface(width: f32, height: f32, scale: f32) -> Input {
    Input::Surface {
        size: Size { width, height },
        scale,
        safe_area: Insets::default(),
    }
}

#[test]
fn a_resize_and_a_new_scale_give_the_surface_in_points() {
    let mut translator = translator();
    let resized = WindowEvent::Resized(PhysicalSize::new(2000, 1000));
    assert_eq!(
        translator.window_event(&resized),
        Some(surface(1000.0, 500.0, 2.0))
    );
    assert_eq!(translator.scale_changed(1.0), surface(2000.0, 1000.0, 1.0));
}

#[test]
fn the_mouse_moves_presses_scrolls_and_leaves_in_points() {
    let mut translator = translator();
    let at = Vec2::new(100.0, 50.0);
    let moved = WindowEvent::CursorMoved {
        device_id: DEVICE,
        position: PhysicalPosition::new(200.0, 100.0),
    };
    assert_eq!(
        translator.window_event(&moved),
        Some(mouse(PointerPhase::Move, at, Button::None, Vec2::ZERO))
    );
    for (button, expected) in [
        (MouseButton::Left, Button::Primary),
        (MouseButton::Right, Button::Secondary),
        (MouseButton::Middle, Button::Middle),
    ] {
        for (state, phase) in [
            (ElementState::Pressed, PointerPhase::Down),
            (ElementState::Released, PointerPhase::Up),
        ] {
            let event = WindowEvent::MouseInput {
                device_id: DEVICE,
                state,
                button,
            };
            assert_eq!(
                translator.window_event(&event),
                Some(mouse(phase, at, expected, Vec2::ZERO))
            );
        }
    }
    let back = WindowEvent::MouseInput {
        device_id: DEVICE,
        state: ElementState::Pressed,
        button: MouseButton::Back,
    };
    assert_eq!(translator.window_event(&back), None);
    let lines = WindowEvent::MouseWheel {
        device_id: DEVICE,
        delta: MouseScrollDelta::LineDelta(0.0, -2.0),
        phase: TouchPhase::Moved,
    };
    assert_eq!(
        translator.window_event(&lines),
        Some(mouse(
            PointerPhase::Wheel,
            at,
            Button::None,
            Vec2::new(0.0, -2.0 * LINE_POINTS)
        ))
    );
    let pixels = WindowEvent::MouseWheel {
        device_id: DEVICE,
        delta: MouseScrollDelta::PixelDelta(PhysicalPosition::new(20.0, 40.0)),
        phase: TouchPhase::Moved,
    };
    assert_eq!(
        translator.window_event(&pixels),
        Some(mouse(
            PointerPhase::Wheel,
            at,
            Button::None,
            Vec2::new(10.0, 20.0)
        ))
    );
    let left = WindowEvent::CursorLeft { device_id: DEVICE };
    assert_eq!(
        translator.window_event(&left),
        Some(mouse(PointerPhase::Leave, at, Button::None, Vec2::ZERO))
    );
}

#[test]
fn a_finger_touches_moves_and_lifts_as_its_own_pointer() {
    let mut translator = translator();
    for (phase, expected, button) in [
        (TouchPhase::Started, PointerPhase::Down, Button::Primary),
        (TouchPhase::Moved, PointerPhase::Move, Button::None),
        (TouchPhase::Ended, PointerPhase::Up, Button::Primary),
        (TouchPhase::Cancelled, PointerPhase::Cancel, Button::None),
    ] {
        let event = WindowEvent::Touch(Touch {
            device_id: DEVICE,
            phase,
            location: PhysicalPosition::new(60.0, 80.0),
            force: None,
            id: 3,
        });
        assert_eq!(
            translator.window_event(&event),
            Some(Input::Pointer(Pointer {
                id: 4,
                kind: PointerKind::Touch,
                phase: expected,
                position: Vec2::new(30.0, 40.0),
                button,
                wheel: Vec2::ZERO,
                modifiers: Modifiers::default(),
            }))
        );
    }
}

#[test]
fn keys_carry_their_phase_and_the_held_modifiers() {
    let mut translator = translator();
    let held =
        WindowEvent::ModifiersChanged((ModifiersState::SHIFT | ModifiersState::SUPER).into());
    assert_eq!(translator.window_event(&held), None);
    let modifiers = Modifiers {
        shift: true,
        command: true,
        ..Modifiers::default()
    };
    let key = PhysicalKey::Code(WinitKey::KeyA);
    for (state, repeat, phase) in [
        (ElementState::Pressed, false, KeyPhase::Down),
        (ElementState::Pressed, true, KeyPhase::Repeat),
        (ElementState::Released, false, KeyPhase::Up),
    ] {
        assert_eq!(
            translator.key(key, state, repeat),
            Some(Input::Key(Key {
                code: KeyCode::A,
                phase,
                modifiers,
            }))
        );
    }
    for (winit, code) in [
        (WinitKey::Digit7, KeyCode::Digit7),
        (WinitKey::Enter, KeyCode::Enter),
        (WinitKey::NumpadEnter, KeyCode::Enter),
        (WinitKey::Escape, KeyCode::Escape),
        (WinitKey::ArrowLeft, KeyCode::ArrowLeft),
        (WinitKey::Home, KeyCode::Home),
        (WinitKey::End, KeyCode::End),
        (WinitKey::BrowserBack, KeyCode::Back),
    ] {
        let Some(Input::Key(key)) =
            translator.key(PhysicalKey::Code(winit), ElementState::Pressed, false)
        else {
            panic!("{winit:?} is not mapped");
        };
        assert_eq!(key.code, code);
    }
}

#[test]
fn keys_beryl_does_not_bind_are_dropped() {
    let translator = translator();
    for key in [
        PhysicalKey::Code(WinitKey::F11),
        PhysicalKey::Code(WinitKey::CapsLock),
        PhysicalKey::Unidentified(NativeKeyCode::Unidentified),
    ] {
        assert_eq!(translator.key(key, ElementState::Pressed, false), None);
    }
}

#[test]
fn f11_toggles_fullscreen_and_so_does_control_command_f_on_macos() {
    assert!(is_fullscreen_key(
        PhysicalKey::Code(WinitKey::F11),
        false,
        false
    ));
    let f = PhysicalKey::Code(WinitKey::KeyF);
    assert_eq!(is_fullscreen_key(f, true, true), cfg!(target_os = "macos"));
    assert!(!is_fullscreen_key(f, false, true));
    assert!(!is_fullscreen_key(f, true, false));
}

#[test]
fn an_input_method_commits_and_composes_text_in_the_edited_field() {
    let mut translator = translator();
    assert_eq!(
        translator.window_event(&WindowEvent::Ime(Ime::Commit("か".to_owned()))),
        Some(Input::Text(TextChange::Insert("か".to_owned())))
    );
    assert_eq!(
        translator.window_event(&WindowEvent::Ime(Ime::Preedit("ka".to_owned(), None))),
        Some(Input::Text(TextChange::Preedit("ka".to_owned())))
    );
    assert_eq!(
        translator.window_event(&WindowEvent::Ime(Ime::Disabled)),
        Some(Input::Text(TextChange::Preedit(String::new())))
    );
    assert_eq!(
        translator.window_event(&WindowEvent::Ime(Ime::Enabled)),
        None
    );
}

#[test]
fn a_key_types_its_text_unless_it_is_a_chord_or_a_control_character() {
    let mut translator = translator();
    let typed =
        |translator: &Translator, text: &str| translator.typed(Some(text), ElementState::Pressed);
    assert_eq!(
        typed(&translator, "a"),
        Some(Input::Text(TextChange::Insert("a".to_owned())))
    );
    assert_eq!(typed(&translator, "\r"), None, "Return");
    assert_eq!(typed(&translator, "\u{8}"), None, "Backspace");
    assert_eq!(translator.typed(Some("a"), ElementState::Released), None);
    let command = WindowEvent::ModifiersChanged(ModifiersState::SUPER.into());
    translator.window_event(&command);
    assert_eq!(
        typed(&translator, "c"),
        None,
        "Cmd+C copies, and types nothing"
    );
}
