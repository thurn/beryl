//! A fault: one broken interface rule, with the elements it concerns and the moment, printed as
//! docs/beryl-3d.md's "What is printed" gives it.

use std::fmt::Write;

use beryl_protocol::protocol::Side;

use kit_ui::input::{Insets, Rect, Size};

/// Which window a frame was drawn for, and the moment, as the caller of a check knows them.
/// A fault's `show:` line is built from it.
#[derive(Clone, Debug, PartialEq)]
pub struct Context {
    /// The recording's name, as `just b3d list` prints it.
    pub recording: String,
    pub seat: Side,
    pub layout: Layout,
    /// The window's size in logical points.
    pub size: Size,
    /// How far in from each edge the window's safe area starts.
    pub safe_area: Insets,
    /// The moment the frame shows, in nanoseconds since the recording's start.
    pub at: u64,
}

/// The two layouts of the table and the flat layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layout {
    Desktop,
    Phone,
}

impl Layout {
    /// The shorter side, in logical points, below which a window uses the phone layout.
    pub const PHONE_BELOW_POINTS: f32 = kit_base::geometry::PHONE_BELOW_POINTS;

    /// The layout for a window of this logical size: `Phone` when its shorter side is under
    /// `PHONE_BELOW_POINTS`, so a phone held sideways is still a phone, and `Desktop` at or above
    /// it. Logical points do not depend on the screen's pixel density.
    #[must_use]
    pub fn for_size(size: Size) -> Self {
        if kit_base::geometry::phone_size(size) {
            Self::Phone
        } else {
            Self::Desktop
        }
    }
}

/// One rule of docs/beryl-3d.md's "Interface checks".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    OnScreen,
    Overlap,
    Reachable,
    Duplicate,
    TextFits,
    TextOverlap,
    Drawn,
    Named,
    BlankCard,
    LinesHold,
    Clips,
    LayerDrawn,
    MarksOn,
    PhoneSizes,
    Jump,
    RestPlacement,
    StepTime,
    StepShown,
    Crowded,
}

impl Rule {
    /// Every rule, in the doc's order.
    pub const ALL: [Self; 19] = [
        Self::OnScreen,
        Self::Overlap,
        Self::Reachable,
        Self::Duplicate,
        Self::TextFits,
        Self::TextOverlap,
        Self::Drawn,
        Self::Named,
        Self::Clips,
        Self::LayerDrawn,
        Self::MarksOn,
        Self::PhoneSizes,
        Self::Jump,
        Self::RestPlacement,
        Self::StepTime,
        Self::StepShown,
        Self::Crowded,
        Self::BlankCard,
        Self::LinesHold,
    ];

    /// Where the rule is in [`Self::ALL`].
    #[must_use]
    pub fn index(self) -> usize {
        Self::ALL
            .iter()
            .position(|&rule| rule == self)
            .expect("every rule is in ALL")
    }

    /// The word a printed fault names the rule by.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::OnScreen => "on-screen",
            Self::Overlap => "overlap",
            Self::Reachable => "reachable",
            Self::Duplicate => "duplicate",
            Self::TextFits => "text-fits",
            Self::TextOverlap => "text-overlap",
            Self::Drawn => "drawn",
            Self::Named => "named",
            Self::BlankCard => "blank-card",
            Self::LinesHold => "lines-hold",
            Self::Clips => "clips",
            Self::LayerDrawn => "layer-drawn",
            Self::MarksOn => "marks-on",
            Self::PhoneSizes => "phone-sizes",
            Self::Jump => "no-jumps",
            Self::RestPlacement => "rest-placement",
            Self::StepTime => "step-time",
            Self::StepShown => "step-shown",
            Self::Crowded => "crowded",
        }
    }
}

/// An element a fault concerns, as it was in the frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Subject {
    /// The start of its tree line, which locates it.
    pub locator: String,
    /// Its bounding box on screen, in logical points.
    pub rect: Rect,
}

impl Subject {
    /// The locator followed by the box, as a fault prints it: `button "Cancel" #4 (270,598
    /// 80x44)`.
    #[must_use]
    pub fn describe(&self) -> String {
        format!("{} ({})", self.locator, rect_text(self.rect))
    }
}

/// A broken rule.
#[derive(Clone, Debug, PartialEq)]
pub struct Fault {
    pub rule: Rule,
    pub elements: Vec<Subject>,
    /// The moment, in nanoseconds since the recording's start.
    pub at: u64,
    /// One sentence that says what is wrong, naming the elements.
    pub message: String,
}

impl Fault {
    /// The fault in three lines: the rule, the test and the moment; what is wrong; and the
    /// command that shows the frame.
    #[must_use]
    pub fn print(&self, context: &Context) -> String {
        let at = milliseconds(self.at);
        let seat = match context.seat {
            Side::You => "you",
            Side::Opponent => "opponent",
        };
        let size = format!("{}x{}", context.size.width, context.size.height);
        let mut out = format!(
            "FAULT {}  b3d:{}/{seat}/{size} t={at}ms\n  {}\n  show: just b3d tree {} --seat {seat}",
            self.rule.word(),
            context.recording,
            self.message,
            context.recording,
        );
        if context.layout == Layout::Phone {
            out.push_str(" --layout phone");
        }
        let _ = write!(out, " --size {size} --at {at}ms");
        out
    }
}

/// A rectangle as the tree prints a box: `x,y wxh`, rounded to whole points.
#[must_use]
pub fn rect_text(rect: Rect) -> String {
    format!(
        "{},{} {}x{}",
        rect.x.round(),
        rect.y.round(),
        rect.width.round(),
        rect.height.round()
    )
}

/// Nanoseconds as whole milliseconds, rounded to the nearest.
fn milliseconds(nanoseconds: u64) -> u64 {
    (nanoseconds + 500_000) / 1_000_000
}
