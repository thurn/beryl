//! Turns winit's window events into the client's `Input`s (docs/beryl-3d.md, "Hosts"). Pointer
//! positions become logical points from the window's top left. A key is mapped by its physical
//! position, so a binding stays on the same key under any keyboard layout, and a key Beryl does
//! not bind is dropped.

use app_client::Input;
use glam::Vec2;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase, Size,
    TextChange,
};
use winit::dpi::{PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton, MouseScrollDelta, TouchPhase, WindowEvent};
use winit::keyboard::{KeyCode as WinitKey, ModifiersState, PhysicalKey};

/// How many logical points one line of a mouse wheel scrolls: one notch of the wheel, which
/// scrolls a battlefield row by one group (`pointer.wheel_notch_points` in
/// `data/beryl3d/space.json`).
pub const LINE_POINTS: f32 = kit_base::data::space::pointer::WHEEL_NOTCH_POINTS;

/// The mouse's pointer id. Fingers count from 1.
const MOUSE: u8 = 0;

/// What the translator remembers between events: the window's size and scale, where the mouse
/// is and which modifier keys are held.
#[derive(Clone, Copy, Debug)]
pub struct Translator {
    size: PhysicalSize<u32>,
    scale: f64,
    /// The mouse's last position, in logical points.
    cursor: Vec2,
    modifiers: Modifiers,
}

impl Translator {
    /// A translator for a window of `size` physical pixels at `scale` pixels per point.
    pub fn new(size: PhysicalSize<u32>, scale: f64) -> Self {
        Self {
            size,
            scale,
            cursor: Vec2::ZERO,
            modifiers: Modifiers::default(),
        }
    }

    /// The `Input` for `event`, if it is one the client takes. Key events go through `key`,
    /// because winit's key event cannot be built outside winit.
    pub fn window_event(&mut self, event: &WindowEvent) -> Option<Input> {
        match event {
            WindowEvent::Resized(size) => {
                self.size = *size;
                Some(self.surface())
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                Some(self.scale_changed(*scale_factor))
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = self.logical(*position);
                Some(self.mouse(PointerPhase::Move, Button::None, Vec2::ZERO))
            }
            WindowEvent::CursorLeft { .. } => {
                Some(self.mouse(PointerPhase::Leave, Button::None, Vec2::ZERO))
            }
            WindowEvent::MouseInput { state, button, .. } => {
                let button = match button {
                    MouseButton::Left => Button::Primary,
                    MouseButton::Right => Button::Secondary,
                    MouseButton::Middle => Button::Middle,
                    _ => return None,
                };
                Some(self.mouse(phase(*state), button, Vec2::ZERO))
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let wheel = match delta {
                    MouseScrollDelta::LineDelta(x, y) => Vec2::new(*x, *y) * LINE_POINTS,
                    MouseScrollDelta::PixelDelta(position) => self.logical(*position),
                };
                Some(self.mouse(PointerPhase::Wheel, Button::None, wheel))
            }
            WindowEvent::Touch(touch) => Some(self.touch(touch.id, touch.phase, touch.location)),
            WindowEvent::Ime(ime) => ime_input(ime),
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers_from(modifiers.state());
                None
            }
            _ => None,
        }
    }

    /// The `Input` for a key at physical position `key` going down, repeating or up.
    pub fn key(&self, key: PhysicalKey, state: ElementState, repeat: bool) -> Option<Input> {
        let PhysicalKey::Code(code) = key else {
            return None;
        };
        let phase = match (state, repeat) {
            (ElementState::Pressed, false) => KeyPhase::Down,
            (ElementState::Pressed, true) => KeyPhase::Repeat,
            (ElementState::Released, _) => KeyPhase::Up,
        };
        Some(Input::Key(Key {
            code: key_code(code)?,
            phase,
            modifiers: self.modifiers,
        }))
    }

    /// The `Input` for the text a key press types, if it types any: what the edited field
    /// inserts at its caret. A chord with Control or Command types nothing, nor does a key whose
    /// text is a control character, such as Return's or Backspace's. While an input method is
    /// on, macOS sends typed text as `Ime::Commit` instead, and no key event types it.
    pub fn typed(&self, text: Option<&str>, state: ElementState) -> Option<Input> {
        let text = text?;
        let chord = self.modifiers.control || self.modifiers.command;
        if state != ElementState::Pressed || chord || text.chars().any(char::is_control) {
            return None;
        }
        Some(Input::Text(TextChange::Insert(text.to_owned())))
    }

    /// The `Input` for the window moving to a display of `scale` pixels per point. winit sends
    /// the new size in a `Resized` event after it.
    pub fn scale_changed(&mut self, scale: f64) -> Input {
        self.scale = scale;
        self.surface()
    }

    /// The `Input` for finger `id` touching, moving or lifting at `location`.
    pub fn touch(&self, id: u64, phase: TouchPhase, location: PhysicalPosition<f64>) -> Input {
        let (phase, button) = match phase {
            TouchPhase::Started => (PointerPhase::Down, Button::Primary),
            TouchPhase::Moved => (PointerPhase::Move, Button::None),
            TouchPhase::Ended => (PointerPhase::Up, Button::Primary),
            TouchPhase::Cancelled => (PointerPhase::Cancel, Button::None),
        };
        Input::Pointer(Pointer {
            // Fingers are numbered from 1 in the order winit numbers them, wrapping past 255.
            id: (id % 255) as u8 + 1,
            kind: PointerKind::Touch,
            phase,
            position: self.logical(location),
            button,
            wheel: Vec2::ZERO,
            modifiers: self.modifiers,
        })
    }

    /// The window's size and scale as the client's `Surface` input. A desktop window has no
    /// system bars over it, so its clear area is all of it.
    pub fn surface(&self) -> Input {
        let size = self.size.to_logical::<f32>(self.scale);
        Input::Surface {
            size: Size {
                width: size.width,
                height: size.height,
            },
            scale: self.scale as f32,
            safe_area: Insets::default(),
        }
    }

    /// The window's size in physical pixels.
    pub fn size(&self) -> PhysicalSize<u32> {
        self.size
    }

    pub fn modifiers(&self) -> Modifiers {
        self.modifiers
    }

    fn logical(&self, position: PhysicalPosition<f64>) -> Vec2 {
        let position = position.to_logical::<f32>(self.scale);
        Vec2::new(position.x, position.y)
    }

    fn mouse(&self, phase: PointerPhase, button: Button, wheel: Vec2) -> Input {
        Input::Pointer(Pointer {
            id: MOUSE,
            kind: PointerKind::Mouse,
            phase,
            position: self.cursor,
            button,
            wheel,
            modifiers: self.modifiers,
        })
    }
}

/// The `Input` for an input method's event: the text it is composing, shown underlined at the
/// edited field's caret, and the text it commits, inserted there. Turning the method off ends
/// any composing.
pub fn ime_input(ime: &Ime) -> Option<Input> {
    let change = match ime {
        Ime::Commit(text) => TextChange::Insert(text.clone()),
        Ime::Preedit(text, _) => TextChange::Preedit(text.clone()),
        Ime::Disabled => TextChange::Preedit(String::new()),
        Ime::Enabled => return None,
    };
    Some(Input::Text(change))
}

fn phase(state: ElementState) -> PointerPhase {
    match state {
        ElementState::Pressed => PointerPhase::Down,
        ElementState::Released => PointerPhase::Up,
    }
}

/// The modifier keys as the client names them. The command key is the Mac's ⌘ and the
/// Windows key elsewhere, which winit both call "super".
pub fn modifiers_from(state: ModifiersState) -> Modifiers {
    Modifiers {
        shift: state.shift_key(),
        control: state.control_key(),
        alt: state.alt_key(),
        command: state.super_key(),
    }
}

/// The client's name for a key, or `None` for a key Beryl does not bind.
fn key_code(code: WinitKey) -> Option<KeyCode> {
    Some(match code {
        WinitKey::KeyA => KeyCode::A,
        WinitKey::KeyB => KeyCode::B,
        WinitKey::KeyC => KeyCode::C,
        WinitKey::KeyD => KeyCode::D,
        WinitKey::KeyE => KeyCode::E,
        WinitKey::KeyF => KeyCode::F,
        WinitKey::KeyG => KeyCode::G,
        WinitKey::KeyH => KeyCode::H,
        WinitKey::KeyI => KeyCode::I,
        WinitKey::KeyJ => KeyCode::J,
        WinitKey::KeyK => KeyCode::K,
        WinitKey::KeyL => KeyCode::L,
        WinitKey::KeyM => KeyCode::M,
        WinitKey::KeyN => KeyCode::N,
        WinitKey::KeyO => KeyCode::O,
        WinitKey::KeyP => KeyCode::P,
        WinitKey::KeyQ => KeyCode::Q,
        WinitKey::KeyR => KeyCode::R,
        WinitKey::KeyS => KeyCode::S,
        WinitKey::KeyT => KeyCode::T,
        WinitKey::KeyU => KeyCode::U,
        WinitKey::KeyV => KeyCode::V,
        WinitKey::KeyW => KeyCode::W,
        WinitKey::KeyX => KeyCode::X,
        WinitKey::KeyY => KeyCode::Y,
        WinitKey::KeyZ => KeyCode::Z,
        WinitKey::Digit0 => KeyCode::Digit0,
        WinitKey::Digit1 => KeyCode::Digit1,
        WinitKey::Digit2 => KeyCode::Digit2,
        WinitKey::Digit3 => KeyCode::Digit3,
        WinitKey::Digit4 => KeyCode::Digit4,
        WinitKey::Digit5 => KeyCode::Digit5,
        WinitKey::Digit6 => KeyCode::Digit6,
        WinitKey::Digit7 => KeyCode::Digit7,
        WinitKey::Digit8 => KeyCode::Digit8,
        WinitKey::Digit9 => KeyCode::Digit9,
        WinitKey::Enter | WinitKey::NumpadEnter => KeyCode::Enter,
        WinitKey::Escape => KeyCode::Escape,
        WinitKey::Space => KeyCode::Space,
        WinitKey::Tab => KeyCode::Tab,
        WinitKey::Backspace => KeyCode::Backspace,
        WinitKey::Delete => KeyCode::Delete,
        WinitKey::ArrowUp => KeyCode::ArrowUp,
        WinitKey::ArrowDown => KeyCode::ArrowDown,
        WinitKey::ArrowLeft => KeyCode::ArrowLeft,
        WinitKey::ArrowRight => KeyCode::ArrowRight,
        WinitKey::Home => KeyCode::Home,
        WinitKey::End => KeyCode::End,
        // F11 is not here: it toggles fullscreen (`is_fullscreen_key`), so the manual table's
        // F11 (lose one life) is reached by its alias, the minus key.
        WinitKey::F1 => KeyCode::F1,
        WinitKey::F2 => KeyCode::F2,
        WinitKey::F3 => KeyCode::F3,
        WinitKey::F4 => KeyCode::F4,
        WinitKey::F5 => KeyCode::F5,
        WinitKey::F6 => KeyCode::F6,
        WinitKey::F7 => KeyCode::F7,
        WinitKey::F8 => KeyCode::F8,
        WinitKey::F9 => KeyCode::F9,
        WinitKey::F10 => KeyCode::F10,
        WinitKey::F12 => KeyCode::F12,
        WinitKey::Minus => KeyCode::Minus,
        WinitKey::Equal => KeyCode::Equal,
        WinitKey::Comma => KeyCode::Comma,
        WinitKey::Period => KeyCode::Period,
        WinitKey::Slash => KeyCode::Slash,
        WinitKey::BracketLeft => KeyCode::BracketLeft,
        WinitKey::BracketRight => KeyCode::BracketRight,
        WinitKey::Backslash => KeyCode::Backslash,
        WinitKey::BrowserBack => KeyCode::Back,
        _ => return None,
    })
}
