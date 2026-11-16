//! What a host hands the client core: session updates, pointer and key events, a new surface,
//! finished assets and platform events. Everything here is plain data with no time of its own
//! (docs/beryl-3d.md, "Hosts" and "Determinism").

use beryl_protocol::card_records::CardRecords;
use beryl_protocol::{AgentView, GameUpdate};
use glam::Vec2;

use crate::command::Reply;
use crate::settings::Settings;
use crate::widgets::IconId;

/// One thing that has happened, passed to `Client::handle`.
#[derive(Clone, Debug, PartialEq)]
pub enum Input {
    /// A view from the session, boxed because it is far larger than every other input.
    Update(Box<GameUpdate>),
    /// The card store's records of every card and token the match can show, from which the
    /// client makes the match's list of faces (`Client::faces`) when the match loads. Boxed
    /// because it is far larger than every other input.
    Cards(Box<CardRecords>),
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
    /// Text typed into the focused field, or its native field's new contents on a phone.
    Text(TextChange),
    /// The platform's clipboard held this text when the client asked for it
    /// (`Request::Paste`): it is inserted at the focused field's caret.
    Paste(String),
    /// A face, or an atlas page of glyphs or icons, is ready on the GPU.
    Assets(AssetReady),
    /// The app was suspended or resumed, or the system is low on memory.
    Lifecycle(Lifecycle),
    /// Which platform the host runs on, sent once at startup: whether `Cmd` in a shortcut
    /// means the Command key, as on the Mac, or Control. Until it arrives the client takes the
    /// Mac's.
    Platform {
        mac: bool,
    },
    /// The visual tier to draw in.
    Tier(Tier),
    /// The session's answer to a `SessionCommand`.
    Reply(Reply),
    /// The player's saved settings, which the host hands over at startup.
    Settings(Settings),
    /// The agents a person may play, in the order they are offered (`beryl_session::agents`).
    /// The client shows the start screen when they arrive before any view of a game.
    Agents(Vec<AgentView>),
    /// The names of the recordings the testing screen lists, sorted. Only a tools build of the
    /// app sends it, so a release build's client has no testing screen to go to.
    Recordings(Vec<String>),
    /// Nothing changes. A thread other than the host's sends it to wake a host that sleeps until
    /// an event, so that the host looks at what the thread left for it (`Feed::wake`).
    Nudge,
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
    Home,
    End,
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
    /// Glyphs are in a page of the atlas.
    Glyphs { page: u16, glyphs: Vec<GlyphCell> },
    /// Icons are in a page of the atlas.
    Icons { page: u16, cells: Vec<IconCell> },
}

/// Where one rasterized icon lies in its atlas page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IconCell {
    pub icon: IconId,
    /// The size the icon is drawn at, in logical points: the side of its square box.
    pub size: u16,
    /// The icon's square cell in texels, `[x, y, width, height]`, rasterized at the display's
    /// scale, so `size` times the scale texels wide.
    pub rect: [u16; 4],
}

/// Where one rasterized glyph lies in its atlas page, and where its picture sits against the
/// glyph's pen position.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GlyphCell {
    pub key: GlyphKey,
    /// The glyph's rectangle in texels, `[x, y, width, height]`.
    pub rect: [u16; 4],
    /// The rectangle's top left corner relative to the pen position on the baseline, in
    /// physical pixels, with y down: a glyph that rises above the baseline has a negative y.
    pub bearing: [i16; 2],
}

/// An index into the list of faces the client made when the match loaded (`faces::FaceList`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct FaceId(pub u16);

/// Which texture array a face is baked for: board size (256 by 360) or full size.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FaceSize {
    Board,
    Full,
}

/// One glyph of one bundled font at one weight and one size in physical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct GlyphKey {
    /// The font's index in `text::fonts::ALL`.
    pub font: u8,
    /// The CSS weight the variable font is drawn at.
    pub weight: u16,
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
