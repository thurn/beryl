//! The bug report sheet, in the stages of the web interface's `BugReporter`
//! (`ui/src/components/BugReport.tsx`, styled by `ui/src/screens/styles/bug-report.css`). Cmd+B
//! freezes the table and takes a picture of the window with nothing of the report in it; the
//! player then drags out the region that shows the bug over the frozen table (a click, Space,
//! Return or the whole-window button takes the whole window), says what went wrong, and
//! submits. "Saved" shows until the report closes, and the table plays on.
//!
//! Unlike the web, the sheet shows the frozen table itself rather than the picture of it: the
//! client keeps drawing the frame the picture was taken of, which is the same image, so the
//! renderer needs no texture for a window-sized picture. For the same reason the describe stage
//! shows no thumbnail of the region.
//!
//! The sheet is drawn over the screen as the last child of the screen's top element, anchored
//! over the whole window, so that the screen's own elements are kept as they were and none
//! of them fades in again in the picture.

use crate::screens::BugPress;
use crate::screens::ScreenPress;
use glam::Vec2;

use super::ScreenIn;
use kit_base::data::space::flat::phone::bug as metrics;
use kit_base::tokens::{color, ratio, shadow, size, type_style};
use kit_frame::frame::Role;
use kit_text::text::TypeStyle;
use kit_ui::element::{Align, Anchor, Element, Justify, Position, TextAlign, column, row, text};
use kit_ui::input::{Insets, Rect};
use kit_ui::widgets::{self, check_mark, field, icon_button, primary};

/// The name of the field the player describes the bug in.
pub const DESCRIPTION: &str = "description";

/// A drag shorter than this across or down, in logical points, is a click: the whole window
/// (`CLICK`).
pub const CLICK: f32 = size::BUG_CLICK;

/// The least height of the description field: the `.bug-text` rule's `min-height`.
const DESCRIPTION_HEIGHT: f32 = size::BUG_DESCRIPTION_HEIGHT;

/// The panel's width, padding, gap and corner radius (`.bug-panel`).
const PANEL_WIDTH: f32 = size::BUG_PANEL_WIDTH;
const PANEL_PADDING: [f32; 4] = [
    size::BUG_PANEL_PADDING_Y,
    size::BUG_PANEL_PADDING_X,
    size::BUG_PANEL_PADDING_Y,
    size::BUG_PANEL_PADDING_X,
];
const PANEL_GAP: f32 = size::BUG_PANEL_GAP;
const PANEL_RADIUS: f32 = size::BUG_PANEL_RADIUS;

/// `.bug-panel`'s and `.bug-tools`'s background: the surface at 97% and 92%.
const PANEL: [u8; 4] = [33, 31, 28, 247];
const TOOLS: [u8; 4] = [33, 31, 28, 235];

/// The shade over the frame while nothing is dragged (`.bug-shade`, 25%), around the region
/// being dragged (`.bug-region`, 45%) and around the region being described
/// (`.bug-region-dim`, 65%).
const SHADE: [u8; 4] = [0, 0, 0, 64];
const SHADE_DRAG: [u8; 4] = [0, 0, 0, 115];
const SHADE_DESCRIBE: [u8; 4] = [0, 0, 0, 166];

/// Where the tools stand from the window's top right corner, and the side of each tool
/// (`.bug-tools`, `.bug-tool`).
const TOOLS_INSET: [f32; 2] = [size::BUG_TOOLS_INSET_TOP, size::BUG_TOOLS_INSET_RIGHT];
const TOOL: f32 = size::BUG_TOOL;

/// How far the hint stands below the window's top (`.bug-hint`).
const HINT_TOP: f32 = size::BUG_HINT_TOP;

/// The height of the desktop hint's line of text.
const HINT_LINE: f32 = size::BUG_HINT_LINE;

/// The checkbox's side, as the start screen's fair-deal box.
const CHECKBOX: f32 = size::CHECKBOX;

/// `.bug-title`: semibold 22-point display type.
const TITLE: TypeStyle = type_style::BUG_TITLE;

/// `.bug-hint`, `.bug-ai` and `.bug-error`: interface type of 15 and 14 points.
const HINT: TypeStyle = TypeStyle {
    size: size::BUG_HINT_SIZE,
    ..type_style::BODY
};
const ERROR: TypeStyle = TypeStyle {
    size: size::BUG_ERROR_SIZE,
    ..type_style::BODY
};

/// `.bug-saved`: semibold 20-point display type.
const SAVED: TypeStyle = type_style::BUG_SAVED;

/// Where the bug report is, from Cmd+B until it closes.
#[derive(Clone, Debug, PartialEq)]
pub enum BugStage {
    /// The table has frozen and the host is taking its picture: nothing of the report shows,
    /// or it would be in the picture. `drawn` counts the frames drawn since it froze.
    Capturing { drawn: u8 },
    /// The player drags out a region, `drag` so far, over the frozen table.
    Select { drag: Option<Rect> },
    /// The player says what went wrong in `region`; `saving` once Submit has been pressed and
    /// the host has not answered, and `error` if the host could not save it.
    Describe {
        region: Rect,
        saving: bool,
        error: Option<String>,
    },
    /// The report is saved, and closes at `close_at` (nanoseconds since the client started),
    /// once a frame has set it.
    Saved { region: Rect, close_at: Option<u64> },
}

/// What the bug report holds while it is open.
#[derive(Clone, Debug, PartialEq)]
pub struct BugForm {
    pub stage: BugStage,
    /// Whether the "AI play" box is checked.
    pub ai: bool,
}

impl BugForm {
    /// A report just opened, freezing the table.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            stage: BugStage::Capturing { drawn: 0 },
            ai: false,
        }
    }
}

impl Default for BugForm {
    fn default() -> Self {
        Self::new()
    }
}

/// The rectangle from `a` to `b`, either way round (`span`).
#[must_use]
pub fn span(a: glam::Vec2, b: glam::Vec2) -> Rect {
    let (low, high) = (a.min(b), a.max(b));
    Rect {
        x: low.x,
        y: low.y,
        width: high.x - low.x,
        height: high.y - low.y,
    }
}

/// What the flat layer shows for the report over `under`, the screen's own element, or alone
/// where the screen shows nothing.
#[must_use]
pub fn bug_report(under: Option<Element>, input: &ScreenIn) -> Element {
    let window = whole(input);
    let phone = input.window.phone().then_some(input.window.safe_area);
    let overlay = match &input.bug.stage {
        // Nothing of the report is in the picture.
        BugStage::Capturing { .. } => None,
        BugStage::Select { drag } => Some(select(*drag, window, phone)),
        BugStage::Describe {
            region,
            saving,
            error,
        } => Some(describe(
            input,
            *region,
            *saving,
            error.as_deref(),
            window,
            phone,
        )),
        BugStage::Saved { region, .. } => Some(saved(*region, window, phone)),
    };
    // Anchored at the window's top left corner, since the screen's top element may be no
    // larger than the prompt bar.
    let overlay = overlay.map(|children| {
        let mut overlay = column()
            .name("bug report")
            .background([0; 4])
            .border(([0; 4], 0.0))
            .width(window.width)
            .height(window.height)
            .children(children);
        overlay.anchor = Some(Anchor::Point(Vec2::ZERO, Vec2::ZERO));
        overlay
    });
    match under {
        Some(screen) => screen.children(overlay),
        // The game screen shows nothing between prompts; a clear element fills the window then.
        None => column()
            .name("frozen screen")
            .background([0; 4])
            .border(([0; 4], 0.0))
            .grow(1.0)
            .align_self(Align::Stretch)
            .children(overlay),
    }
}

/// The whole window, in logical points.
fn whole(input: &ScreenIn) -> Rect {
    Rect {
        x: 0.0,
        y: 0.0,
        width: input.window.size.width,
        height: input.window.size.height,
    }
}

/// The stage where a region is dragged out: the shade, or the region so far with the window
/// shaded around it, the hint until a drag starts, and the tools. On a phone, whose safe area is
/// `phone`, the hint stands at the top left beside the tools (`body.phone .bug-hint`).
fn select(drag: Option<Rect>, window: Rect, phone: Option<Insets>) -> Vec<Element> {
    let mut children = match drag {
        Some(region) => framed(region, window, SHADE_DRAG),
        None => vec![placed("shade", window).background(SHADE)],
    };
    if drag.is_none() {
        let style = if phone.is_some() {
            TypeStyle {
                size: metrics::HINT,
                ..HINT
            }
        } else {
            // `.bug-hint` sets no line height, and WebKit's normal line for Open Sans at 15
            // points is a 20-point line box.
            TypeStyle {
                line_height: HINT_LINE / HINT.size,
                ..HINT
            }
        };
        let hint = text("Drag to select")
            .style(style)
            .color(color::TEXT)
            .background(TOOLS)
            .border((color::METAL, 1.0));
        let mut hint = hint;
        hint.style.padding = [
            size::BUG_HINT_PADDING_Y,
            size::BUG_HINT_PADDING_X,
            size::BUG_HINT_PADDING_Y,
            size::BUG_HINT_PADDING_X,
        ];
        // `border-radius: 999px` draws a pill: half the hint's height, its line, padding and
        // border, which is the radius the web's computed style reports.
        hint.style.radius = style.size * style.line_height / 2.0 + size::BUG_HINT_PADDING_Y + 1.0;
        let line = row()
            .name("hint")
            .position(Position::Absolute)
            .inset([Some(0.0), None, None, Some(0.0)])
            .width(window.width);
        children.push(match phone {
            Some(safe) => {
                let mut line = line.child(hint);
                line.style.padding = [
                    safe.top + metrics::HINT_TOP,
                    0.0,
                    0.0,
                    safe.left + metrics::HINT_LEFT,
                ];
                line
            }
            None => line
                .justify_content(Justify::Center)
                .child(hint.margin([HINT_TOP, 0.0, 0.0, 0.0])),
        });
    }
    children.push(tools(true, phone));
    children
}

/// The stage where the bug is described: the region with the window shaded around it, the
/// panel in the middle of the window and the close tool.
fn describe(
    input: &ScreenIn,
    region: Rect,
    saving: bool,
    error: Option<&str>,
    window: Rect,
    phone: Option<Insets>,
) -> Vec<Element> {
    let mut state = input.fields.get(DESCRIPTION);
    "What went wrong?".clone_into(&mut state.placeholder);
    let ready = !state.text.trim().is_empty() && !saving;
    let mut description = field(DESCRIPTION, &state);
    description.style.min_height = Some(DESCRIPTION_HEIGHT);
    description.style.align_self = Some(Align::Stretch);
    if phone.is_some() {
        // `body.phone .bug-text`: shorter, and no taller than a few lines, so the sheet leaves
        // the frozen view above it.
        description.style.min_height = Some(metrics::DESCRIPTION_LEAST);
        description.style.max_height = Some(metrics::DESCRIPTION_MOST);
    }
    let submit = primary(if saving { "Saving…" } else { "Submit" });
    let mut submit = submit
        .enabled(ready)
        .on_press(ScreenPress::Bug(BugPress::Save));
    // `size="small"`: shorter, with smaller type, and only as wide as its label.
    submit.style.height = Some(size::BUTTON_HEIGHT * ratio::SMALL_SCALE);
    submit.style.text.size = type_style::BUTTON.size * ratio::SMALL_SCALE;
    submit.style.min_width = None;
    let target = kit_base::data::audit::PHONE_TARGET_POINTS;
    let mut ai = ai_box(input.bug.ai);
    if phone.is_some() {
        // `body.phone .bug-actions .button` and `.bug-ai`: touch targets.
        submit.style.height = Some(target);
        submit.style.min_width = Some(metrics::SUBMIT_WIDTH);
        ai.style.min_height = Some(target);
    }
    let actions = row()
        .name("actions")
        .align_self(Align::Stretch)
        .align_items(Align::Center)
        .justify_content(Justify::SpaceBetween)
        .child(ai)
        .child(submit);
    let title = if phone.is_some() {
        TypeStyle {
            size: metrics::TITLE,
            ..TITLE
        }
    } else {
        TITLE
    };
    let mut panel_children = vec![text("Report bug").style(title).color(color::TYPE)];
    panel_children.push(description);
    if let Some(error) = error {
        panel_children.push(text(error).style(ERROR).color(color::STAT_DOWN));
    }
    panel_children.push(actions);
    let mut children = framed(region, window, SHADE_DESCRIBE);
    children.push(placed_panel(panel(panel_children), window, phone));
    children.push(tools(false, phone));
    children
}

/// The stage once the report is saved: the region still framed, and "Saved".
fn saved(region: Rect, window: Rect, phone: Option<Insets>) -> Vec<Element> {
    let words = text("Saved").style(SAVED).color(color::TYPE);
    let mut children = framed(region, window, SHADE_DESCRIBE);
    children.push(placed_panel(
        panel(vec![words]).name("Saved"),
        window,
        phone,
    ));
    children
}

/// The panel in the middle of the window or, on a phone whose safe area is `phone`, as a
/// sheet along the window's foot (`body.phone .bug-panel`): as wide as the window, with rounded
/// corners, padded past the safe area's foot. A panel's corners all share one radius here, so
/// its bottom corners are rounded too, unlike the web's. While the system keyboard is up the
/// window is shorter, so the sheet stands on the keyboard.
fn placed_panel(mut panel: Element, window: Rect, phone: Option<Insets>) -> Element {
    let Some(safe) = phone else {
        return centred("panel", panel, window);
    };
    panel.style.width = Some(window.width);
    panel.style.gap = metrics::GAP;
    panel.style.radius = metrics::RADIUS;
    panel.style.padding = [
        metrics::PADDING_Y,
        safe.right + metrics::PADDING_X,
        safe.bottom + metrics::PADDING_Y,
        safe.left + metrics::PADDING_X,
    ];
    panel.style.max_height = Some(window.height - safe.top);
    panel.style.overflow = kit_ui::element::Overflow::Scroll;
    column()
        .name("panel")
        .background([0; 4])
        .border(([0; 4], 0.0))
        .position(Position::Absolute)
        .inset([Some(0.0), None, None, Some(0.0)])
        .width(window.width)
        .height(window.height)
        .justify_content(Justify::End)
        .text_align(TextAlign::Start)
        .child(panel)
}

/// The panel that holds the description or "Saved" (`.bug-panel`).
fn panel(children: Vec<Element>) -> Element {
    let mut panel = column()
        .name("Report bug")
        .width(PANEL_WIDTH)
        .gap(PANEL_GAP)
        .background(PANEL)
        .border((color::METAL, 1.0))
        .radius(PANEL_RADIUS)
        .children(children);
    panel.style.padding = PANEL_PADDING;
    panel
}

/// The "AI play" box (`.bug-ai`): a questionable play of the AI's, whose report carries the
/// agent's latest searches.
fn ai_box(on: bool) -> Element {
    let check = row()
        .name("checkbox")
        .width(CHECKBOX)
        .height(CHECKBOX)
        .radius(size::BUG_CHECKBOX_RADIUS)
        .border((color::GOLD, size::BORDER))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .children(on.then(|| check_mark(CHECKBOX - size::BUG_CHECK_INSET)));
    Element::new(Role::Toggle)
        .name("AI play")
        .selected(on)
        .gap(size::BUG_AI_GAP)
        .align_items(Align::Center)
        .on_press(ScreenPress::Bug(BugPress::Ai))
        .child(check)
        .child(text("AI play").style(HINT).color(color::TEXT_DIM))
}

/// The tools at the top right on their dark pill (`.bug-tools`): the whole window, while a
/// region is being chosen, and the close button.
/// On a phone, whose safe area is `phone`, they stand inside it and each is a touch target
/// (`body.phone .bug-tools`).
fn tools(choosing: bool, phone: Option<Insets>) -> Element {
    let side = if phone.is_some() {
        kit_base::data::audit::PHONE_TARGET_POINTS
    } else {
        TOOL
    };
    let inset = phone.map_or(TOOLS_INSET, |safe| {
        [
            safe.top + metrics::TOOLS_TOP,
            safe.right + metrics::TOOLS_RIGHT,
        ]
    });
    let tool = |icon, name: &str, press| {
        let mut tool = icon_button(icon, name)
            .width(side)
            .height(side)
            .background([0; 4])
            .border(([0; 4], 0.0))
            .radius(side / 2.0)
            .on_press(press);
        // `.bug-tool` has no rim: `background: none` and no box shadow.
        tool.style.box_shadow = Vec::new();
        tool
    };
    let mut pill = row()
        .name("bug tools")
        .gap(2.0)
        .background(TOOLS)
        .border((color::METAL, 1.0))
        // `border-radius: 999px` on a pill one tool tall: half its height.
        .radius(side / 2.0 + size::BUG_TOOLS_PADDING + 1.0)
        .position(Position::Absolute)
        .inset([Some(inset[0]), Some(inset[1]), None, None])
        .children(choosing.then(|| {
            tool(
                widgets::icons::WHOLE,
                "Whole window",
                ScreenPress::Bug(BugPress::Whole),
            )
        }))
        .child(tool(
            widgets::icons::CLOSE,
            "Close",
            ScreenPress::CloseBugReport,
        ));
    pill.style.padding = [size::BUG_TOOLS_PADDING; 4];
    pill.style.box_shadow = shadow::BUG_TOOLS.to_vec();
    pill
}

/// `region` outlined in gold (`.bug-region`), with the rest of `window` shaded by `shade` in
/// four bands around it, as the web's wide box shadow shades it.
fn framed(region: Rect, window: Rect, shade: [u8; 4]) -> Vec<Element> {
    let right = region.x + region.width;
    let bottom = region.y + region.height;
    let bands = [
        ("shade above", 0.0, 0.0, window.width, region.y),
        (
            "shade below",
            0.0,
            bottom,
            window.width,
            window.height - bottom,
        ),
        ("shade left", 0.0, region.y, region.x, region.height),
        (
            "shade right",
            right,
            region.y,
            window.width - right,
            region.height,
        ),
    ];
    let mut children: Vec<Element> = bands
        .into_iter()
        .map(|(name, x, y, width, height)| {
            let band = Rect {
                x,
                y,
                width: width.max(0.0),
                height: height.max(0.0),
            };
            placed(name, band).background(shade)
        })
        .collect();
    children.push(
        placed("region", region)
            .background([0; 4])
            .border((color::GOLD, size::BORDER)),
    );
    children
}

/// A clear region named `name` placed at `rect` over the window.
fn placed(name: &str, rect: Rect) -> Element {
    row()
        .name(name)
        .position(Position::Absolute)
        .inset([Some(rect.y), None, None, Some(rect.x)])
        .width(rect.width)
        .height(rect.height)
        .border(([0; 4], 0.0))
}

/// `child` in the middle of `window`, in a clear region named `name` that fills it.
fn centred(name: &str, child: Element, window: Rect) -> Element {
    column()
        .name(name)
        .background([0; 4])
        .border(([0; 4], 0.0))
        .position(Position::Absolute)
        .inset([Some(0.0), None, None, Some(0.0)])
        .width(window.width)
        .height(window.height)
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .text_align(TextAlign::Start)
        .child(child)
}
