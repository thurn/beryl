//! Pointer, keyboard, and asset data used by the client and its controls.

use glam::Vec2;

pub use crate::atlas::{FaceId, FaceSize, GlyphCell, GlyphKey, IconCell, Tier};
pub use crate::geometry::{Insets, Rect, Size};

/// A mouse, touch or pen event. It carries no time: what it starts begins at the `now` of the
/// next frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pointer {
    /// 0 for the mouse, then one for each finger.
    pub id: u8,
    pub kind: PointerKind,
    pub phase: PointerPhase,
    /// Logical points from the window's top left corner.
    pub position: Vec2,
    pub button: Button,
    /// Logical points scrolled, for `PointerPhase::Wheel`.
    pub wheel: Vec2,
    pub modifiers: Modifiers,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerKind {
    Mouse,
    Touch,
    Pen,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PointerPhase {
    Down,
    Move,
    Up,
    Cancel,
    /// The pointer left the window.
    Leave,
    Wheel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Primary,
    Secondary,
    Middle,
    None,
}

/// The modifier keys held during an event.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per modifier key, as the platforms report them"
)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

/// A key event. It carries no time, like a pointer event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Key {
    pub code: KeyCode,
    pub phase: KeyPhase,
    pub modifiers: Modifiers,
}

/// Closed: the keys Beryl binds, and `Back` for Android's back button. A host drops a key that
/// is not here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyCode {
    A,
    B,
    C,
    D,
    E,
    F,
    G,
    H,
    I,
    J,
    K,
    L,
    M,
    N,
    O,
    P,
    Q,
    R,
    S,
    T,
    U,
    V,
    W,
    X,
    Y,
    Z,
    Digit0,
    Digit1,
    Digit2,
    Digit3,
    Digit4,
    Digit5,
    Digit6,
    Digit7,
    Digit8,
    Digit9,
    Enter,
    Escape,
    Space,
    Tab,
    Backspace,
    Delete,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Home,
    End,
    /// The function keys F1 to F12, which the manual table binds (F11 and F12 change a life
    /// total, for one).
    F1,
    F2,
    F3,
    F4,
    F5,
    F6,
    F7,
    F8,
    F9,
    F10,
    F11,
    F12,
    /// The punctuation keys the manual table binds, by the physical key, as the web's
    /// `KeyboardEvent.code` names them.
    Minus,
    Equal,
    Comma,
    Period,
    Slash,
    BracketLeft,
    BracketRight,
    Backslash,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPhase {
    Down,
    Repeat,
    Up,
}

/// A change to the focused text field's text from outside the client's own key handling.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextChange {
    /// The field's whole new text, sent by a phone host's native field on every change.
    Replace(String),
    /// Characters typed, or committed by an input method (winit's `Ime::Commit`), inserted at
    /// the caret in place of the selection.
    Insert(String),
    /// The text an input method is composing (winit's `Ime::Preedit`), shown underlined at the
    /// caret and not yet part of the field's text. An empty string ends the composing.
    Preedit(String),
}

/// What a worker tells the client core when it finishes. The pixels go to the renderer; the
/// client core learns only where they are.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AssetReady {
    /// A face is in a layer of the small or of the full-size texture array.
    Face {
        face: FaceId,
        size: FaceSize,
        layer: u16,
    },
    /// A component gallery sample face is in its board-size texture array.
    GalleryFace { face: FaceId, layer: u16 },
    /// Glyphs are in a page of the atlas.
    Glyphs { page: u16, glyphs: Vec<GlyphCell> },
    /// Icons are in a page of the atlas.
    Icons { page: u16, cells: Vec<IconCell> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Suspended,
    Resumed,
    LowMemory,
}
