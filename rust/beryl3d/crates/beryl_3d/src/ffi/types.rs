//! The C types of `include/beryl3d.h` and their conversions to and from the client's own. Every
//! enumerated value crosses as a `uint32_t` with named constants, and a value no constant
//! names is dropped rather than converted, so that no value from C can make an invalid Rust
//! enum.

use std::ffi::{CString, c_char, c_void};

use app_client::command::{Haptic, Request};
use glam::Vec2;
use kit_ui::input::{
    Button, Insets, Key, KeyCode, KeyPhase, Modifiers, Pointer, PointerKind, PointerPhase,
};

/// What `beryl3d_new` starts.
#[repr(C)]
pub struct BerylConfig {
    /// The recording to replay, a UTF-8 C string: `<data_dir>/<fixture>.json` is read. NULL
    /// plays a live game.
    pub fixture: *const c_char,
    /// The app's writable folder, a UTF-8 C string: recordings are read there, and frame logs,
    /// replays and the ledger of games written there.
    pub data_dir: *const c_char,
    /// In a debug build, the name of an agent (`beryl_search::AGENTS`) that answers the
    /// person's decisions in a fresh live game, for unattended runs. NULL for a person.
    /// Ignored in a release build.
    pub stand_in: *const c_char,
    /// The display's highest refresh rate, in frames a second.
    pub display_hz: f32,
    /// Called with `context`, from any thread, when an input arrives from another thread while
    /// the host may have paused its ticks: the host asks for a frame soon, hopping to its main
    /// thread itself. May be NULL.
    pub wake: Option<unsafe extern "C" fn(context: *mut c_void)>,
    pub context: *mut c_void,
}

/// The parts of the surface that system bars or a notch cover, in points.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct BerylInsets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

/// When the next frame is wanted: `kind` is one of the `BERYL_WAKE_` values, and `at` the
/// platform's monotonic time in nanoseconds for `BERYL_WAKE_AT`. `hz` is the rate the host
/// should ask its display for from now on.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BerylWake {
    pub kind: u32,
    pub at: u64,
    pub hz: f32,
}

pub const BERYL_WAKE_NOW: u32 = 0;
pub const BERYL_WAKE_AT: u32 = 1;
pub const BERYL_WAKE_NEVER: u32 = 2;

pub const BERYL_SUSPENDED: u32 = 0;
pub const BERYL_RESUMED: u32 = 1;
pub const BERYL_LOW_MEMORY: u32 = 2;

/// A mouse, touch or pen event. `kind`, `phase` and `button` are `BERYL_POINTER_`,
/// `BERYL_PHASE_` and `BERYL_BUTTON_` values, `modifiers` a sum of the modifier bits, and the
/// position logical points from the surface's top left.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BerylPointer {
    pub id: u32,
    pub kind: u32,
    pub phase: u32,
    pub x: f32,
    pub y: f32,
    pub button: u32,
    pub wheel_x: f32,
    pub wheel_y: f32,
    pub modifiers: u32,
}

/// A key event: `code` is a `BERYL_KEY_` value, `phase` a `BERYL_KEY_DOWN`, `REPEAT` or `UP`.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BerylKey {
    pub code: u32,
    pub phase: u32,
    pub modifiers: u32,
}

/// Something the client asks of the platform: `kind` is a `BERYL_REQUEST_` value. `haptic` is
/// a `BERYL_HAPTIC_` value for a haptic. The rectangle is the edited field's, in points.
/// `text` is the field's contents, the path to share or the URL, else NULL.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BerylRequest {
    pub kind: u32,
    pub haptic: u32,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub text: *const c_char,
}

pub const BERYL_POINTER_MOUSE: u32 = 0;
pub const BERYL_POINTER_TOUCH: u32 = 1;
pub const BERYL_POINTER_PEN: u32 = 2;

pub const BERYL_PHASE_DOWN: u32 = 0;
pub const BERYL_PHASE_MOVE: u32 = 1;
pub const BERYL_PHASE_UP: u32 = 2;
pub const BERYL_PHASE_CANCEL: u32 = 3;
pub const BERYL_PHASE_LEAVE: u32 = 4;
pub const BERYL_PHASE_WHEEL: u32 = 5;

pub const BERYL_BUTTON_PRIMARY: u32 = 0;
pub const BERYL_BUTTON_SECONDARY: u32 = 1;
pub const BERYL_BUTTON_MIDDLE: u32 = 2;
pub const BERYL_BUTTON_NONE: u32 = 3;

pub const BERYL_SHIFT: u32 = 1;
pub const BERYL_CONTROL: u32 = 2;
pub const BERYL_ALT: u32 = 4;
pub const BERYL_COMMAND: u32 = 8;

pub const BERYL_KEY_DOWN: u32 = 0;
pub const BERYL_KEY_REPEAT: u32 = 1;
pub const BERYL_KEY_UP: u32 = 2;

pub const BERYL_REQUEST_BEGIN_TEXT_ENTRY: u32 = 0;
pub const BERYL_REQUEST_END_TEXT_ENTRY: u32 = 1;
pub const BERYL_REQUEST_SHARE: u32 = 2;
pub const BERYL_REQUEST_HAPTIC: u32 = 3;
pub const BERYL_REQUEST_OPEN_URL: u32 = 4;

pub const BERYL_HAPTIC_LIGHT: u32 = 0;
pub const BERYL_HAPTIC_MEDIUM: u32 = 1;

/// Every key code in the order of the header's `BERYL_KEY_` values, which are its indexes:
/// `BERYL_KEY_A` is 0 and `BERYL_KEY_BACK` the last.
pub const KEY_CODES: [KeyCode; 47] = {
    use KeyCode::{
        A, ArrowDown, ArrowLeft, ArrowRight, ArrowUp, B, Back, Backspace, C, D, Delete, Digit0,
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, E, Enter, Escape,
        F, G, H, I, J, K, L, M, N, O, P, Q, R, S, Space, T, Tab, U, V, W, X, Y, Z,
    };
    [
        A, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z, Digit0,
        Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Enter, Escape,
        Space, Tab, Backspace, Delete, ArrowUp, ArrowDown, ArrowLeft, ArrowRight, Back,
    ]
};

/// The modifier keys the bits of `bits` name, or `None` when a bit names none.
fn modifiers(bits: u32) -> Option<Modifiers> {
    if bits & !(BERYL_SHIFT | BERYL_CONTROL | BERYL_ALT | BERYL_COMMAND) != 0 {
        return None;
    }
    Some(Modifiers {
        shift: bits & BERYL_SHIFT != 0,
        control: bits & BERYL_CONTROL != 0,
        alt: bits & BERYL_ALT != 0,
        command: bits & BERYL_COMMAND != 0,
    })
}

/// The scale and safe area of a surface the host passed, as the client takes them. A scale
/// that is not a positive number becomes 1, since dividing the surface's pixels by it would
/// give a size in points that is infinite, negative or not a number. An inset that is not a
/// number or is negative becomes 0, since it would push the layout off the surface.
#[must_use]
pub fn surface_from_c(scale: f32, safe: BerylInsets) -> (f32, Insets) {
    let scale = if scale.is_finite() && scale > 0.0 {
        scale
    } else {
        1.0
    };
    let inset = |value: f32| {
        if value.is_finite() {
            value.max(0.0)
        } else {
            0.0
        }
    };
    let insets = Insets {
        top: inset(safe.top),
        right: inset(safe.right),
        bottom: inset(safe.bottom),
        left: inset(safe.left),
    };
    (scale, insets)
}

/// The client's pointer event for `event`, or `None` when its id is over 255, a value names
/// nothing, or its position or wheel is not a finite number (which no hit test can place).
#[must_use]
pub fn pointer_from_c(event: BerylPointer) -> Option<Pointer> {
    if ![event.x, event.y, event.wheel_x, event.wheel_y]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let kind = match event.kind {
        BERYL_POINTER_MOUSE => PointerKind::Mouse,
        BERYL_POINTER_TOUCH => PointerKind::Touch,
        BERYL_POINTER_PEN => PointerKind::Pen,
        _ => return None,
    };
    let phase = match event.phase {
        BERYL_PHASE_DOWN => PointerPhase::Down,
        BERYL_PHASE_MOVE => PointerPhase::Move,
        BERYL_PHASE_UP => PointerPhase::Up,
        BERYL_PHASE_CANCEL => PointerPhase::Cancel,
        BERYL_PHASE_LEAVE => PointerPhase::Leave,
        BERYL_PHASE_WHEEL => PointerPhase::Wheel,
        _ => return None,
    };
    let button = match event.button {
        BERYL_BUTTON_PRIMARY => Button::Primary,
        BERYL_BUTTON_SECONDARY => Button::Secondary,
        BERYL_BUTTON_MIDDLE => Button::Middle,
        BERYL_BUTTON_NONE => Button::None,
        _ => return None,
    };
    Some(Pointer {
        id: u8::try_from(event.id).ok()?,
        kind,
        phase,
        position: Vec2::new(event.x, event.y),
        button,
        wheel: Vec2::new(event.wheel_x, event.wheel_y),
        modifiers: modifiers(event.modifiers)?,
    })
}

/// The client's key event for `event`, or `None` when a value names nothing.
#[must_use]
pub fn key_from_c(event: BerylKey) -> Option<Key> {
    let phase = match event.phase {
        BERYL_KEY_DOWN => KeyPhase::Down,
        BERYL_KEY_REPEAT => KeyPhase::Repeat,
        BERYL_KEY_UP => KeyPhase::Up,
        _ => return None,
    };
    Some(Key {
        code: *KEY_CODES.get(usize::try_from(event.code).ok()?)?,
        phase,
        modifiers: modifiers(event.modifiers)?,
    })
}

/// `request` as the header's struct, with its text put in `text`, which the struct points
/// into. `None` for a request the platform does not carry out (saving settings, music and
/// sound levels), which the app will handle itself.
pub fn request_to_c(request: &Request, text: &mut CString) -> Option<BerylRequest> {
    let mut out = BerylRequest {
        kind: 0,
        haptic: 0,
        x: 0.0,
        y: 0.0,
        width: 0.0,
        height: 0.0,
        text: std::ptr::null(),
    };
    let mut set_text = |contents: &str| {
        // A NUL cannot cross in a C string, so one in the text is left out.
        *text = CString::new(contents.replace('\0', "")).unwrap_or_default();
        text.as_ptr()
    };
    match request {
        Request::BeginTextEntry { rect, text, .. } => {
            out.kind = BERYL_REQUEST_BEGIN_TEXT_ENTRY;
            out.x = rect.x;
            out.y = rect.y;
            out.width = rect.width;
            out.height = rect.height;
            out.text = set_text(text);
        }
        Request::EndTextEntry => out.kind = BERYL_REQUEST_END_TEXT_ENTRY,
        Request::Share { path } => {
            out.kind = BERYL_REQUEST_SHARE;
            out.text = set_text(&path.to_string_lossy());
        }
        Request::Haptic(haptic) => {
            out.kind = BERYL_REQUEST_HAPTIC;
            out.haptic = match haptic {
                Haptic::Light => BERYL_HAPTIC_LIGHT,
                Haptic::Medium => BERYL_HAPTIC_MEDIUM,
            };
        }
        Request::OpenUrl(url) => {
            out.kind = BERYL_REQUEST_OPEN_URL;
            out.text = set_text(url);
        }
        // A phone's text is edited in its native field, which copies and pastes for itself, so
        // the clipboard requests of a field edited in place never come from a phone's client.
        // The app's feed takes the bug report's picture and saves the report, as it sees each
        // request first; nothing exports bug reports from a phone yet.
        Request::SaveSettings(_)
        | Request::ExportBugs
        | Request::FreezeFrame
        | Request::SaveBug { .. }
        | Request::Music(_)
        | Request::Audio { .. }
        | Request::Copy(_)
        | Request::Paste
        // A phone's app has no testing screen, whose recordings only a desktop tools build
        // lists.
        | Request::Replay { .. }
        // A phone's app is never quit from inside it.
        | Request::Quit => return None,
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use std::ffi::CStr;
    use std::path::PathBuf;

    use app_client::settings::Settings;
    use kit_frame::frame::ElementId;
    use kit_ui::input::Rect;

    use super::*;

    /// `request` as the header's struct, and its text as Rust's.
    fn convert(request: &Request) -> Option<(BerylRequest, Option<String>)> {
        let mut text = CString::default();
        let out = request_to_c(request, &mut text)?;
        let read = (!out.text.is_null()).then(|| {
            // SAFETY: a non-null `text` points into `text`, which is alive here.
            #[allow(unsafe_code)]
            let read = unsafe { CStr::from_ptr(out.text) };
            read.to_string_lossy().into_owned()
        });
        Some((out, read))
    }

    #[test]
    fn each_request_crosses_with_its_kind_rectangle_and_text() {
        let (begin, text) = convert(&Request::BeginTextEntry {
            field: ElementId(7),
            rect: Rect {
                x: 1.0,
                y: 2.0,
                width: 30.0,
                height: 4.0,
            },
            text: "héllo".to_owned(),
        })
        .expect("crosses");
        assert_eq!(
            (begin.kind, begin.x, begin.y, begin.width, begin.height),
            (BERYL_REQUEST_BEGIN_TEXT_ENTRY, 1.0, 2.0, 30.0, 4.0)
        );
        assert_eq!(text.as_deref(), Some("héllo"));

        let (end, text) = convert(&Request::EndTextEntry).expect("crosses");
        assert_eq!((end.kind, text), (BERYL_REQUEST_END_TEXT_ENTRY, None));

        let (share, text) = convert(&Request::Share {
            path: PathBuf::from("/tmp/game.json"),
        })
        .expect("crosses");
        assert_eq!(share.kind, BERYL_REQUEST_SHARE);
        assert_eq!(text.as_deref(), Some("/tmp/game.json"));

        for (haptic, value) in [
            (Haptic::Light, BERYL_HAPTIC_LIGHT),
            (Haptic::Medium, BERYL_HAPTIC_MEDIUM),
        ] {
            let (out, text) = convert(&Request::Haptic(haptic)).expect("crosses");
            assert_eq!(
                (out.kind, out.haptic, text),
                (BERYL_REQUEST_HAPTIC, value, None)
            );
        }

        let (url, text) =
            convert(&Request::OpenUrl("https://example.com/a\0b".to_owned())).expect("crosses");
        assert_eq!(url.kind, BERYL_REQUEST_OPEN_URL);
        assert_eq!(text.as_deref(), Some("https://example.com/ab"));
    }

    #[test]
    fn requests_the_app_keeps_do_not_cross() {
        for request in [
            Request::SaveSettings(Settings::default()),
            Request::Music(None),
            Request::Audio {
                music: 50,
                effects: 50,
                muted: false,
            },
        ] {
            assert!(convert(&request).is_none(), "{request:?} crossed");
        }
    }

    #[test]
    fn values_no_constant_names_are_dropped() {
        let touch = BerylPointer {
            id: 1,
            kind: BERYL_POINTER_TOUCH,
            button: BERYL_BUTTON_PRIMARY,
            modifiers: BERYL_SHIFT | BERYL_COMMAND,
            ..BerylPointer::default()
        };
        let pointer = pointer_from_c(touch).expect("a touch converts");
        assert_eq!(pointer.kind, PointerKind::Touch);
        assert!(pointer.modifiers.shift && pointer.modifiers.command);
        assert!(!pointer.modifiers.control && !pointer.modifiers.alt);
        assert!(pointer_from_c(BerylPointer { id: 256, ..touch }).is_none());
        assert!(pointer_from_c(BerylPointer { kind: 99, ..touch }).is_none());
        assert!(pointer_from_c(BerylPointer { phase: 6, ..touch }).is_none());
        assert!(pointer_from_c(BerylPointer { button: 4, ..touch }).is_none());
        assert!(
            pointer_from_c(BerylPointer {
                x: f32::NAN,
                ..touch
            })
            .is_none()
        );
        assert!(
            pointer_from_c(BerylPointer {
                wheel_y: f32::INFINITY,
                ..touch
            })
            .is_none()
        );
        assert!(
            pointer_from_c(BerylPointer {
                modifiers: 16,
                ..touch
            })
            .is_none()
        );

        let back = BerylKey {
            code: 46,
            phase: BERYL_KEY_UP,
            modifiers: 0,
        };
        assert_eq!(key_from_c(back).map(|key| key.code), Some(KeyCode::Back));
        assert_eq!(
            key_from_c(BerylKey { code: 0, ..back }).map(|key| key.code),
            Some(KeyCode::A)
        );
        assert!(key_from_c(BerylKey { code: 47, ..back }).is_none());
        assert!(key_from_c(BerylKey { phase: 3, ..back }).is_none());
    }

    #[test]
    #[allow(clippy::float_cmp)] // The replacements are exact values, not computed ones.
    fn a_surface_scale_or_inset_that_is_not_a_size_is_replaced() {
        let safe = BerylInsets {
            top: 47.0,
            right: -3.0,
            bottom: f32::NAN,
            left: f32::INFINITY,
        };
        let (scale, insets) = surface_from_c(3.0, safe);
        assert_eq!(scale, 3.0);
        assert_eq!(
            (insets.top, insets.right, insets.bottom, insets.left),
            (47.0, 0.0, 0.0, 0.0)
        );
        for bad in [0.0, -2.0, f32::NAN, f32::INFINITY] {
            assert_eq!(surface_from_c(bad, safe).0, 1.0, "scale {bad}");
        }
    }
}
