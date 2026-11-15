//! What a host hands the client core: session updates, pointer and key events, a new surface,
//! finished assets and platform events. Everything here is plain data with no time of its own
//! (docs/beryl-3d.md, "Hosts" and "Determinism").

use beryl_session::GameUpdate;
use glam::Vec2;

use crate::command::Reply;

/// One thing that has happened, passed to `Client::handle`.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A view from the session, boxed because it is far larger than every other input.
    Update(Box<GameUpdate>),
    /// The drawing surface's size in logical points, how many physical pixels make one point,
    /// and the part of it that system bars and rounded corners leave clear.
    Surface {
        size: Size,
        scale: f32,
        safe_area: Insets,
    },
    /// A mouse, touch or pen event.
    Pointer(Pointer),
    Key(Key),
    /// The text field being edited has new contents.
    Text(TextChange),
    /// A face or a glyph page is ready on the GPU.
    Assets(AssetReady),
    /// The app was suspended or resumed, or the system is low on memory.
    Lifecycle(Lifecycle),
    /// The visual tier to draw in.
    Tier(Tier),
    /// The session's answer to a `SessionCommand`.
    Reply(Reply),
}

/// A rectangle with its top left corner at (`x`, `y`). It is in logical points unless the field
/// holding it says otherwise (a frame's clips are in physical pixels).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// A size in logical points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

/// How far in from each edge of the surface its clear area starts, in logical points.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Insets {
    pub top: f32,
    pub right: f32,
    pub bottom: f32,
    pub left: f32,
}

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
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyPhase {
    Down,
    Repeat,
    Up,
}

/// The edited text field's whole new text, sent by a phone host's native field on every change.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextChange(pub String);

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
    /// Glyphs are in a page of the atlas, each at a rectangle in texels, `[x, y, width,
    /// height]`.
    Glyphs {
        page: u16,
        glyphs: Vec<(GlyphKey, [u16; 4])>,
    },
}

/// An index into the list of faces the client made when the match loaded.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaceId(pub u16);

/// Which texture array a face is baked for: board size (256 by 360) or full size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceSize {
    Board,
    Full,
}

/// One glyph of one bundled font at one size in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlyphKey {
    pub font: u8,
    pub glyph: u16,
    pub size: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lifecycle {
    Suspended,
    Resumed,
    LowMemory,
}

/// The two visual tiers, which differ only in shading.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Baseline,
    Enhanced,
}
