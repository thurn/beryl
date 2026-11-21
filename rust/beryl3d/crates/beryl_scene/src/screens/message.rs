//! Messages emitted by screens that stand outside the game and manual table.

use beryl_protocol::protocol::Side;

use super::{GalleryMenu, GalleryQuery, Screen, Sheet};

/// A control on an app screen or sheet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScreenPress {
    Open(Screen),
    Sheet(Option<Sheet>),
    Concede,
    Start,
    Agent(u16),
    FairDeal,
    ExportBugs,
    Replay { recording: u16 },
    Seat(Side),
    GalleryMenu(GalleryMenu),
    GalleryQuery(Option<GalleryQuery>),
    DismissMenu,
    Bug(BugPress),
    Log,
    Undo,
}

/// What a `Press::Bug` does (`screens::bug_report`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BugPress {
    /// Report the whole window instead of a region dragged out over it.
    Whole,
    /// Turn the "AI play" box on or off.
    Ai,
    /// Save the report with the description typed.
    Save,
}
