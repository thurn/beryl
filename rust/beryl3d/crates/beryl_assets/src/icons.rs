//! The flat layer's icons (docs/beryl-3d.md, "The frame": the `Icon` material). Each icon of
//! `kit_ui::widgets::icons` is kept here as the web interface's own SVG path data, copied
//! exactly, with the viewbox, fills and strokes its stylesheet gives it. The glyph worker
//! rasterizes every icon with `tiny_skia` into a single-channel atlas cell at the display's
//! scale, at the size the web draws it, and sends the cells as an `AssetReady::Icons`
//! (`GlyphWorker::icons`). The `Icon` material tints a cell's coverage, as `Glyph` does.
//!
//! One channel holds one color. Where the web paints an icon in more than one color, as the
//! phase icons' dark fills under cream outlines, a fill's coverage is scaled by its color's
//! brightness against the outline's (its `ink`), so that over the dark table it reads as the
//! darker of the two.

use std::cell::Cell;

use kit_ui::element::Shadow;
use kit_ui::input::IconCell;
use kit_ui::widgets::icons::{
    BUG, BUTTON_SIDE, CLOSE, FULL_CONTROL, FULL_CONTROL_SIDE, GEAR, LOCK, LOCK_SIDE, LOG, MENU,
    MINUS, PASS_ON, PHASE_SIDE, PHASES, PLUS, SPEAKER, SPEAKER_OFF, STEPS, STEPS_OUTLINED, UNDO,
    WHOLE,
};
use kit_ui::widgets::{IconId, icon_shadows};
use tiny_skia::{FillRule, LineCap, LineJoin, Paint, Pixmap, Stroke, Transform};

use crate::blur;
use crate::glyphs::Bitmap;
use crate::svg;

/// One icon's artwork.
#[derive(Clone, Copy, Debug)]
pub struct Artwork {
    pub icon: IconId,
    /// The side of the square box the web draws it in, in logical points.
    pub size: u16,
    /// The SVG viewbox: its left, top, width and height. The viewbox is fitted into the square
    /// cell and centred in it, as SVG's default `xMidYMid meet` does.
    pub view: [f32; 4],
    /// The shapes, painted in order.
    pub shapes: &'static [Shape],
}

/// One `<path>` of an icon.
#[derive(Clone, Copy, Debug)]
pub struct Shape {
    /// Its `d` attribute.
    pub data: &'static str,
    pub fill: Option<Fill>,
    pub stroke: Option<Outline>,
    /// A `rotate(degrees cx cy)` transform, in viewbox units.
    pub rotate: Option<[f32; 3]>,
}

/// How a shape is filled.
#[derive(Clone, Copy, Debug)]
pub struct Fill {
    /// The fill's brightness against the icon's main color, from 0 to 1.
    pub ink: f32,
    /// SVG's `fill-rule: evenodd`, where nonzero is the default.
    pub even_odd: bool,
}

/// How a shape is stroked: always with round caps and joins, as every stroked icon of the web
/// interface is (an open path's caps are what the eye sees; a closed one has none).
#[derive(Clone, Copy, Debug)]
pub struct Outline {
    /// The stroke's width, in viewbox units.
    pub width: f32,
}

/// The view box of the web's 24-unit icons.
const BOX_24: [f32; 4] = [0.0, 0.0, 24.0, 24.0];
/// The view box of the web's 16-unit mark icons (`MarkBadge`, components/Card.tsx).
const BOX_16: [f32; 4] = [0.0, 0.0, 16.0, 16.0];
/// The view box of Phosphor's icons.
const BOX_256: [f32; 4] = [0.0, 0.0, 256.0, 256.0];
/// The size of an icon button's glyph (`.icon-button > svg`, icon-button.css) and of a step
/// stop (`.step-icon`, phase-tracker.css).
const BUTTON: u16 = BUTTON_SIDE;
/// The size of full control's mark (`.full-control svg`, controls.css).
const FULL_CONTROL_SIZE: u16 = FULL_CONTROL_SIDE;

/// The phase icons' outline width (`.phase-icon`'s `stroke-width`).
const PHASE_STROKE: f32 = 1.7;
/// The phase icons' fills against their cream outline, #f0e2c4: `.phase-dark` and
/// `.phase-blade` (#2c2825), `.phase-card` (#3e342a) and `.phase-stone` (#5f564b), each the
/// ratio of the two colors' luma.
const DARK: f32 = 0.18;
const CARD: f32 = 0.24;
const STONE: f32 = 0.38;
/// Full control's fill, #2c2825, against its gold outline (`--gold`, #e4a242).
const FULL_CONTROL_FILL: f32 = 0.24;

const fn filled(data: &'static str) -> Shape {
    Shape {
        data,
        fill: Some(Fill {
            ink: 1.0,
            even_odd: false,
        }),
        stroke: None,
        rotate: None,
    }
}

const fn stroked(data: &'static str, width: f32) -> Shape {
    Shape {
        data,
        fill: None,
        stroke: Some(Outline { width }),
        rotate: None,
    }
}

/// A phase icon's shape filled with `ink` and outlined in cream.
const fn phase_fill(data: &'static str, ink: f32) -> Shape {
    Shape {
        data,
        fill: Some(Fill {
            ink,
            even_odd: false,
        }),
        stroke: Some(Outline {
            width: PHASE_STROKE,
        }),
        rotate: None,
    }
}

/// A phase icon's shape filled with `ink` and not outlined (`stroke="none"`).
const fn phase_bare(data: &'static str, ink: f32) -> Shape {
    Shape {
        data,
        fill: Some(Fill {
            ink,
            even_odd: false,
        }),
        stroke: None,
        rotate: None,
    }
}

const fn phase(icon: IconId, shapes: &'static [Shape]) -> Artwork {
    Artwork {
        icon,
        size: PHASE_SIDE,
        view: BOX_24,
        shapes,
    }
}

const fn button(icon: IconId, shapes: &'static [Shape]) -> Artwork {
    Artwork {
        icon,
        size: BUTTON,
        view: BOX_24,
        shapes,
    }
}

/// A step stop: one Phosphor path, filled with `currentColor` (Steps.tsx).
const fn step(icon: IconId, shapes: &'static [Shape]) -> Artwork {
    Artwork {
        icon,
        size: BUTTON,
        view: BOX_256,
        shapes,
    }
}

/// The speaker's body, filled and outlined (`.sound-icon-fill` in a `.sound-icon`).
const SPEAKER_BODY: Shape = Shape {
    data: "M4 9h4l5-4v14l-5-4H4z",
    fill: Some(Fill {
        ink: 1.0,
        even_odd: false,
    }),
    stroke: Some(Outline { width: 1.8 }),
    rotate: None,
};

/// A main phase's peak, a snow line under its summit (`PEAK`, Controls.tsx).
const PEAK: [Shape; 3] = [
    phase_fill("M3 19.5 L12 4.5 L21 19.5 Z", DARK),
    phase_bare("M6.6 18 L12 9.3 L17.4 18 Z", STONE),
    stroked("M8.6 10.4 L10.4 11.8 L12 9.6 L13.6 11.8 L15.4 10.4", 1.1),
];

/// Every icon, in the order of their ids.
pub const ALL: [Artwork; 43] = [
    // `Gear`, game/Corner.tsx: filled even-odd, its hole left empty (`.gear`, table.css).
    button(
        GEAR,
        &[Shape {
            data: "M19.67 9.71 L22.34 9.69 L22.34 14.31 L19.67 14.29 L19.04 15.80 L20.95 17.68 \
                   L17.68 20.95 L15.80 19.04 L14.29 19.67 L14.31 22.34 L9.69 22.34 L9.71 19.67 \
                   L8.20 19.04 L6.32 20.95 L3.05 17.68 L4.96 15.80 L4.33 14.29 L1.66 14.31 \
                   L1.66 9.69 L4.33 9.71 L4.96 8.20 L3.05 6.32 L6.32 3.05 L8.20 4.96 L9.71 4.33 \
                   L9.69 1.66 L14.31 1.66 L14.29 4.33 L15.80 4.96 L17.68 3.05 L20.95 6.32 \
                   L19.04 8.20 Z M16.9 12 A4.9 4.9 0 1 0 7.1 12 A4.9 4.9 0 1 0 16.9 12 Z",
            fill: Some(Fill {
                ink: 1.0,
                even_odd: true,
            }),
            stroke: None,
            rotate: None,
        }],
    ),
    // `BugIcon`, components/BugReport.tsx: strokes 1.8 wide (`.bug-icon`, bug-report.css).
    button(
        BUG,
        &[
            stroked("M9 5.5 7.5 3.5M15 5.5l1.5-2", 1.8),
            stroked(
                "M12 7.5c-2.5 0-4.5 2.2-4.5 5.5S9.5 20 12 20s4.5-3.7 4.5-7-2-5.5-4.5-5.5Z",
                1.8,
            ),
            stroked("M9.2 7.4a2.8 2.8 0 0 1 5.6 0", 1.8),
            stroked(
                "M12 10v10M7.5 12H4M16.5 12H20M7.8 16.5 5 18.5M16.2 16.5l2.8 2M8 9.5 5.5 7.5M16 \
                 9.5l2.5-2",
                1.8,
            ),
        ],
    ),
    // `UndoButton`, game/Corner.tsx: a stroke 2.2 wide (`.undo-arrow`).
    button(
        UNDO,
        &[stroked(
            "M8.5 4.5 4 9l4.5 4.5M4 9h10a5.5 5.5 0 0 1 0 11h-4",
            2.2,
        )],
    ),
    // The game log's toggle, game/Narration.tsx: strokes 1.8 wide in a 20-unit box
    // (`.game-log-toggle svg`, table.css).
    Artwork {
        icon: LOG,
        size: BUTTON,
        view: [0.0, 0.0, 20.0, 20.0],
        shapes: &[stroked("M4 5h12M4 10h12M4 15h8", 1.8)],
    },
    // The zone viewer's closing X, game/Piles.tsx: filled (`.zone-viewer-close svg`).
    button(
        CLOSE,
        &[filled(
            "M5.6 3.5 12 9.9l6.4-6.4 2.1 2.1-6.4 6.4 6.4 6.4-2.1 2.1-6.4-6.4-6.4 6.4-2.1-2.1 \
             6.4-6.4-6.4-6.4z",
        )],
    ),
    // The speaker of audio/SoundControls.tsx, with its sound waves and muted.
    button(
        SPEAKER,
        &[
            SPEAKER_BODY,
            stroked("M16 9a4 4 0 0 1 0 6M18.5 6.5a7.5 7.5 0 0 1 0 11", 1.8),
        ],
    ),
    button(
        SPEAKER_OFF,
        &[SPEAKER_BODY, stroked("M16.5 9.5l5 5M21.5 9.5l-5 5", 1.8)],
    ),
    // `FullControlIcon`, game/Corner.tsx: a dark stop sign outlined in gold, with two bars
    // (`.full-control svg`, controls.css).
    Artwork {
        icon: FULL_CONTROL,
        size: FULL_CONTROL_SIZE,
        view: BOX_24,
        shapes: &[
            Shape {
                data: "M8 1.5 H16 L22.5 8 V16 L16 22.5 H8 L1.5 16 V8 Z",
                fill: Some(Fill {
                    ink: FULL_CONTROL_FILL,
                    even_odd: false,
                }),
                stroke: Some(Outline { width: 1.6 }),
                rotate: None,
            },
            stroked("M9.5 7.5 V16.5 M14.5 7.5 V16.5", 2.6),
        ],
    },
    // `FastForwardIcon`, components/Controls.tsx, on the pass-on button: filled, in a box
    // 24 by 16, drawn 22 wide (`.pass-on-face > svg`, controls.css).
    Artwork {
        icon: PASS_ON,
        size: BUTTON,
        view: [0.0, 0.0, 24.0, 16.0],
        shapes: &[filled("M1 1 L11 8 L1 15 Z M12 1 L22 8 L12 15 Z")],
    },
    // The web draws plus and minus as text, so these are strokes 2 wide, the weight of the
    // phone menu's lines (mobile/chrome.tsx), at an icon button's size.
    button(PLUS, &[stroked("M5 12h14M12 5v14", 2.0)]),
    button(MINUS, &[stroked("M5 12h14", 2.0)]),
    // The step stops of components/Steps.tsx, each filled and then outlined.
    // untap: Phosphor's "arrow-clockwise".
    step(
        STEPS[0],
        &[filled(
            "M240,56v48a8,8,0,0,1-8,8H184a8,8,0,0,1-5.66-13.66l17-17-10.55-9.65-.25-.24a80,80,0,1,0-1.67,114.78,8,8,0,1,1,11,11.63A95.44,95.44,0,0,1,128,224h-1.32A96,96,0,1,1,195.75,60l10.93,10L226.34,50.3A8,8,0,0,1,240,56Z",
        )],
    ),
    step(
        STEPS_OUTLINED[0],
        &[filled(
            "M244,56v48a12,12,0,0,1-12,12H184a12,12,0,1,1,0-24H201.1l-19-17.38c-.13-.12-.26-.24-.38-.37A76,76,0,1,0,127,204h1a75.53,75.53,0,0,0,52.15-20.72,12,12,0,0,1,16.49,17.45A99.45,99.45,0,0,1,128,228h-1.37A100,100,0,1,1,198.51,57.06L220,76.72V56a12,12,0,0,1,24,0Z",
        )],
    ),
    // upkeep: Phosphor's "hourglass".
    step(
        STEPS[1],
        &[filled(
            "M200,75.64V40a16,16,0,0,0-16-16H72A16,16,0,0,0,56,40V76a16.08,16.08,0,0,0,6.41,12.8L114.67,128,62.4,167.2A16.07,16.07,0,0,0,56,180v36a16,16,0,0,0,16,16H184a16,16,0,0,0,16-16V180.36a16,16,0,0,0-6.36-12.77L141.26,128l52.38-39.59A16.05,16.05,0,0,0,200,75.64Z",
        )],
    ),
    step(
        STEPS_OUTLINED[1],
        &[filled(
            "M204,75.64V40a20,20,0,0,0-20-20H72A20,20,0,0,0,52,40V76a20.1,20.1,0,0,0,8,16l48,36L60,164a20.1,20.1,0,0,0-8,16v36a20,20,0,0,0,20,20H184a20,20,0,0,0,20-20V180.36a20.13,20.13,0,0,0-7.94-16L147.9,128l48.16-36.4A20.13,20.13,0,0,0,204,75.64ZM180,212H76V182l52-39,52,39.33Zm0-138.35L128,113,76,74V44H180Z",
        )],
    ),
    // draw: Phosphor's "cards".
    step(
        STEPS[2],
        &[filled(
            "M200,88V200a16,16,0,0,1-16,16H40a16,16,0,0,1-16-16V88A16,16,0,0,1,40,72H184A16,16,0,0,1,200,88Zm16-48H64a8,8,0,0,0,0,16H216V176a8,8,0,0,0,16,0V56A16,16,0,0,0,216,40Z",
        )],
    ),
    step(
        STEPS_OUTLINED[2],
        &[filled(
            "M180,72H36A20,20,0,0,0,16,92V204a20,20,0,0,0,20,20H180a20,20,0,0,0,20-20V92A20,20,0,0,0,180,72Zm-4,128H40V96H176ZM240,52V176a12,12,0,0,1-24,0V56H64a12,12,0,0,1,0-24H220A20,20,0,0,1,240,52Z",
        )],
    ),
    // first main: Phosphor's "mountains".
    step(
        STEPS[3],
        &[filled(
            "M254.88,195.92l-54.56-92.08A15.87,15.87,0,0,0,186.55,96h0a15.85,15.85,0,0,0-13.76,7.84l-15.64,26.39a4,4,0,0,0,0,4.07l26.8,45.47a8.13,8.13,0,0,1-1.89,10.55,8,8,0,0,1-11.8-2.26L101.79,71.88a16,16,0,0,0-27.58,0L1.11,195.94a8,8,0,0,0,1,9.52A8.23,8.23,0,0,0,8.23,208H247.77a8.29,8.29,0,0,0,6.09-2.55A8,8,0,0,0,254.88,195.92ZM64.43,120,88,80l23.57,40ZM140,52a24,24,0,1,1,24,24A24,24,0,0,1,140,52Z",
        )],
    ),
    step(
        STEPS_OUTLINED[3],
        &[filled(
            "M160,80a32,32,0,1,0-32-32A32,32,0,0,0,160,80Zm0-40a8,8,0,1,1-8,8A8,8,0,0,1,160,40Zm94.32,153.88L199.76,101.8A19.85,19.85,0,0,0,182.55,92h0a19.83,19.83,0,0,0-17.2,9.8l-18.7,31.55-37.42-63.5a20,20,0,0,0-34.46,0L1.66,193.91A12,12,0,0,0,12,212H244a12,12,0,0,0,10.32-18.12ZM92,87.87,108.57,116H75.43ZM33,188l28.28-48h61.44L151,188Zm145.86,0L160.56,157l22-37.1L222.94,188Z",
        )],
    ),
    // beginning of combat: Phosphor's "flag".
    step(
        STEPS[4],
        &[filled(
            "M232,56V176a8,8,0,0,1-2.76,6c-15.28,13.23-29.89,18-43.82,18-18.91,0-36.57-8.74-53-16.85C105.87,170,82.79,158.61,56,179.77V224a8,8,0,0,1-16,0V56a8,8,0,0,1,2.77-6h0c36-31.18,68.31-15.21,96.79-1.12C167,62.46,190.79,74.2,218.76,50A8,8,0,0,1,232,56Z",
        )],
    ),
    step(
        STEPS_OUTLINED[4],
        &[filled(
            "M40.14,46.88A12,12,0,0,0,36,56V224a12,12,0,0,0,24,0V181.72c22.84-17.12,42.1-9.12,70.68,5,16.23,8,34.74,17.2,54.8,17.2,14.72,0,30.28-4.94,46.38-18.88A12,12,0,0,0,236,176V56a12,12,0,0,0-19.86-9.07c-24.71,21.41-44.53,13.31-74.82-1.68C113.19,31.27,78.17,13.94,40.14,46.88ZM212,170.26c-22.84,17.13-42.1,9.11-70.68-5C118.16,153.76,90.33,140,60,153.87V61.69c22.84-17.12,42.1-9.12,70.68,5,16.23,8,34.74,17.2,54.8,17.2A63,63,0,0,0,212,78.08Z",
        )],
    ),
    // attackers: Phosphor's "sword".
    step(
        STEPS[5],
        &[filled(
            "M216,32H152a8,8,0,0,0-6.34,3.12l-64,83.21L72,108.69a16,16,0,0,0-22.64,0l-8.69,8.7a16,16,0,0,0,0,22.63l22,22-32,32a16,16,0,0,0,0,22.63l8.69,8.68a16,16,0,0,0,22.62,0l32-32,22,22a16,16,0,0,0,22.64,0l8.69-8.7a16,16,0,0,0,0-22.63l-9.64-9.64,83.21-64A8,8,0,0,0,224,104V40A8,8,0,0,0,216,32Zm-8,68.06-81.74,62.88L115.32,152l50.34-50.34a8,8,0,0,0-11.32-11.31L104,140.68,93.07,129.74,155.94,48H208Z",
        )],
    ),
    step(
        STEPS_OUTLINED[5],
        &[filled(
            "M216,28H152a12,12,0,0,0-9.33,4.45L79.5,110.51l-4.66-4.65a20,20,0,0,0-28.29,0L29.86,122.55a20,20,0,0,0,0,28.29h0L45,166,23.86,187.17a20,20,0,0,0,0,28.28l16.69,16.69a20,20,0,0,0,28.28,0L90,211l15.17,15.16a20,20,0,0,0,28.29,0l16.69-16.69a20,20,0,0,0,0-28.3l-4.65-4.65,78.06-63.17A12,12,0,0,0,228,104V40A12,12,0,0,0,216,28ZM54.69,212.34l-11-11L62,183l11,11Zm64.61-6L49.65,136.7l11.05-11,69.65,69.65ZM204,98.27l-75.58,61.17L121,152l47.51-47.5a12,12,0,0,0-17-17L104,135l-7.45-7.44L157.73,52H204Z",
        )],
    ),
    // blockers: Phosphor's "shield".
    step(
        STEPS[6],
        &[filled(
            "M224,56v56c0,52.72-25.52,84.67-46.93,102.19-23.06,18.86-46,25.27-47,25.53a8,8,0,0,1-4.2,0c-1-.26-23.91-6.67-47-25.53C57.52,196.67,32,164.72,32,112V56A16,16,0,0,1,48,40H208A16,16,0,0,1,224,56Z",
        )],
    ),
    step(
        STEPS_OUTLINED[6],
        &[filled(
            "M208,36H48A20,20,0,0,0,28,56v56c0,54.29,26.32,87.22,48.4,105.29,23.71,19.39,47.44,26,48.44,26.29a12.1,12.1,0,0,0,6.32,0c1-.28,24.73-6.9,48.44-26.29,22.08-18.07,48.4-51,48.4-105.29V56A20,20,0,0,0,208,36Zm-4,76c0,35.71-13.09,64.69-38.91,86.15A126.28,126.28,0,0,1,128,219.38a126.14,126.14,0,0,1-37.09-21.23C65.09,176.69,52,147.71,52,112V60H204Z",
        )],
    ),
    // damage: Phosphor's "lightning".
    step(
        STEPS[7],
        &[filled(
            "M213.85,125.46l-112,120a8,8,0,0,1-13.69-7l14.66-73.33L45.19,143.49a8,8,0,0,1-3-13l112-120a8,8,0,0,1,13.69,7L153.18,90.9l57.63,21.61a8,8,0,0,1,3,12.95Z",
        )],
    ),
    step(
        STEPS_OUTLINED[7],
        &[filled(
            "M219.71,117.38a12,12,0,0,0-7.25-8.52L161.28,88.39l10.59-70.61a12,12,0,0,0-20.64-10l-112,120a12,12,0,0,0,4.31,19.33l51.18,20.47L84.13,238.22a12,12,0,0,0,20.64,10l112-120A12,12,0,0,0,219.71,117.38ZM113.6,203.55l6.27-41.77a12,12,0,0,0-7.41-12.92L68.74,131.37,142.4,52.45l-6.27,41.77a12,12,0,0,0,7.41,12.92l43.72,17.49Z",
        )],
    ),
    // end of combat: Phosphor's "tent".
    step(
        STEPS[8],
        &[filled(
            "M255.31,188.75l-64-144A8,8,0,0,0,184,40H72a8,8,0,0,0-7.31,4.75h0l0,.12v0L.69,188.75A8,8,0,0,0,8,200H248a8,8,0,0,0,7.31-11.25ZM64,184H20.31L64,85.7Zm16,0V85.7L123.69,184Z",
        )],
    ),
    step(
        STEPS_OUTLINED[8],
        &[filled(
            "M255,187.13l-64-144A12,12,0,0,0,180,36H76a12,12,0,0,0-10.85,6.9,2.42,2.42,0,0,0-.12.23L65,43.3a.08.08,0,0,0,0,0L1,187.13A12,12,0,0,0,12,204H244a12,12,0,0,0,11-16.87ZM64,104.55V180H30.46ZM88,180V104.55L121.54,180Zm59.8,0L94.47,60H172.2l53.34,120Z",
        )],
    ),
    // second main: Phosphor's "mountains".
    step(
        STEPS[9],
        &[filled(
            "M254.88,195.92l-54.56-92.08A15.87,15.87,0,0,0,186.55,96h0a15.85,15.85,0,0,0-13.76,7.84l-15.64,26.39a4,4,0,0,0,0,4.07l26.8,45.47a8.13,8.13,0,0,1-1.89,10.55,8,8,0,0,1-11.8-2.26L101.79,71.88a16,16,0,0,0-27.58,0L1.11,195.94a8,8,0,0,0,1,9.52A8.23,8.23,0,0,0,8.23,208H247.77a8.29,8.29,0,0,0,6.09-2.55A8,8,0,0,0,254.88,195.92ZM64.43,120,88,80l23.57,40ZM140,52a24,24,0,1,1,24,24A24,24,0,0,1,140,52Z",
        )],
    ),
    step(
        STEPS_OUTLINED[9],
        &[filled(
            "M160,80a32,32,0,1,0-32-32A32,32,0,0,0,160,80Zm0-40a8,8,0,1,1-8,8A8,8,0,0,1,160,40Zm94.32,153.88L199.76,101.8A19.85,19.85,0,0,0,182.55,92h0a19.83,19.83,0,0,0-17.2,9.8l-18.7,31.55-37.42-63.5a20,20,0,0,0-34.46,0L1.66,193.91A12,12,0,0,0,12,212H244a12,12,0,0,0,10.32-18.12ZM92,87.87,108.57,116H75.43ZM33,188l28.28-48h61.44L151,188Zm145.86,0L160.56,157l22-37.1L222.94,188Z",
        )],
    ),
    // end step: Phosphor's "moon".
    step(
        STEPS[10],
        &[filled(
            "M235.54,150.21a104.84,104.84,0,0,1-37,52.91A104,104,0,0,1,32,120,103.09,103.09,0,0,1,52.88,57.48a104.84,104.84,0,0,1,52.91-37,8,8,0,0,1,10,10,88.08,88.08,0,0,0,109.8,109.8,8,8,0,0,1,10,10Z",
        )],
    ),
    step(
        STEPS_OUTLINED[10],
        &[filled(
            "M236.37,139.4a12,12,0,0,0-12-3A84.07,84.07,0,0,1,119.6,31.59a12,12,0,0,0-15-15A108.86,108.86,0,0,0,49.69,55.07,108,108,0,0,0,136,228a107.09,107.09,0,0,0,64.93-21.69,108.86,108.86,0,0,0,38.44-54.94A12,12,0,0,0,236.37,139.4Zm-49.88,47.74A84,84,0,0,1,68.86,69.51,84.93,84.93,0,0,1,92.27,48.29Q92,52.13,92,56A108.12,108.12,0,0,0,200,164q3.87,0,7.71-.27A84.79,84.79,0,0,1,186.49,187.14Z",
        )],
    ),
    // cleanup: Phosphor's "broom".
    step(
        STEPS[11],
        &[filled(
            "M235.29,216.7C212.86,205.69,200,182.12,200,152V134.69a15.94,15.94,0,0,0-10.09-14.87l-28.65-11.46A8,8,0,0,1,156.79,98l22.32-56.67C184,28.79,178,14.21,165.34,9.51a24,24,0,0,0-30.7,13.71L112.25,80.08a8,8,0,0,1-10.41,4.5L73.11,73.08a15.91,15.91,0,0,0-17.38,3.66C34.68,98.4,24,123.71,24,152a111.53,111.53,0,0,0,31.15,77.53A8.06,8.06,0,0,0,61,232H232a8,8,0,0,0,8-7.51A8.21,8.21,0,0,0,235.29,216.7ZM115.11,216a87.52,87.52,0,0,1-24.26-41.71,8.21,8.21,0,0,0-9.25-6.18A8,8,0,0,0,75.28,178a105.33,105.33,0,0,0,18.36,38H64.44A95.62,95.62,0,0,1,40,152a85.92,85.92,0,0,1,7.73-36.3l137.8,55.13c3,18.06,10.55,33.5,21.89,45.19Z",
        )],
    ),
    step(
        STEPS_OUTLINED[11],
        &[filled(
            "M237.24,213.21C216.12,203,204,180.64,204,152V134.73a19.94,19.94,0,0,0-12.62-18.59l-24.86-9.81a4,4,0,0,1-2.26-5.14l21.33-53A32,32,0,0,0,167.17,6,32.13,32.13,0,0,0,126.25,24.2l-.07.18-21,53.09a3.94,3.94,0,0,1-2.14,2.2,3.89,3.89,0,0,1-3,.06L74.6,69.43A19.89,19.89,0,0,0,52.87,74C31.06,96.43,20,122.68,20,152a115.46,115.46,0,0,0,32.29,80.3A12,12,0,0,0,61,236H232a12,12,0,0,0,5.24-22.79ZM68.19,92.73,91.06,102A28,28,0,0,0,127.5,86.31l20.95-53a8.32,8.32,0,0,1,10.33-4.81,8,8,0,0,1,4.61,10.57,1.17,1.17,0,0,0,0,.11L142,92.29a28.05,28.05,0,0,0,15.68,36.33L180,137.45V152c0,1,0,2.07.05,3.1l-122.44-49A101.91,101.91,0,0,1,68.19,92.73ZM116.74,212a83.73,83.73,0,0,1-22.09-39,12,12,0,0,0-23.25,6,110.27,110.27,0,0,0,14.49,33H66.25A91.53,91.53,0,0,1,44,152a84,84,0,0,1,3.41-24.11l136.67,54.66A86.58,86.58,0,0,0,198.66,212Z",
        )],
    ),
    // The phase icons of components/Controls.tsx (`PHASES`), outlined in cream 1.7 wide over
    // dark fills (`.phase-icon`, phase-tracker.css). Beginning: two cards, each a rounded
    // `<rect>` written as a path.
    phase(
        PHASES[0],
        &[
            Shape {
                rotate: Some([-14.0, 9.5, 12.5]),
                ..phase_fill(
                    "M5.9 5.5H13.1A1.4 1.4 0 0 1 14.5 6.9V18.1A1.4 1.4 0 0 1 13.1 19.5H5.9A1.4 \
                     1.4 0 0 1 4.5 18.1V6.9A1.4 1.4 0 0 1 5.9 5.5Z",
                    DARK,
                )
            },
            Shape {
                rotate: Some([10.0, 14.5, 11.5]),
                ..phase_fill(
                    "M10.9 4.5H18.1A1.4 1.4 0 0 1 19.5 5.9V17.1A1.4 1.4 0 0 1 18.1 18.5H10.9A1.4 \
                     1.4 0 0 1 9.5 17.1V5.9A1.4 1.4 0 0 1 10.9 4.5Z",
                    CARD,
                )
            },
        ],
    ),
    phase(PHASES[1], &PEAK),
    // Combat: crossed swords.
    phase(
        PHASES[2],
        &[
            phase_fill(
                "M3.12 3.12 L4.77 3.43 L15.47 14.12 L15.35 14.25 L18.38 17.29 L18.75 16.93 \
                 L20.08 18.26 L18.26 20.08 L16.93 18.75 L17.29 18.38 L14.25 15.35 L14.12 15.47 \
                 L3.43 4.77 Z",
                DARK,
            ),
            stroked(
                "M5.31 5.31 L13.52 13.52 M17.23 12.12 L12.12 17.23",
                PHASE_STROKE,
            ),
            phase_fill(
                "M20.88 3.12 L19.23 3.43 L8.53 14.12 L8.65 14.25 L5.62 17.29 L5.25 16.93 \
                 L3.92 18.26 L5.74 20.08 L7.07 18.75 L6.71 18.38 L9.75 15.35 L9.88 15.47 \
                 L20.57 4.77 Z",
                DARK,
            ),
            stroked(
                "M18.69 5.31 L10.48 13.52 M6.77 12.12 L11.88 17.23",
                PHASE_STROKE,
            ),
        ],
    ),
    // Second main: the first main phase's peak before a lower one.
    phase(
        PHASES[3],
        &[
            phase_fill("M3 18.5 L8.5 9 L14 18.5 Z", DARK),
            phase_fill("M8 18.5 L14.5 6 L21 18.5 Z", DARK),
            phase_bare("M11.2 17 L14.5 10.6 L17.8 17 Z", STONE),
            stroked("M11.9 10.9 L13.3 12 L14.5 10.3 L15.7 12 L17.1 10.9", 1.1),
        ],
    ),
    // End: the sun setting.
    phase(
        PHASES[4],
        &[
            phase_fill("M5.5 17.5 A6.5 6.5 0 0 1 18.5 17.5 Z", DARK),
            stroked(
                "M3 17.5 H21 M12 4.5 V7 M5.6 7.6 L7.3 9.3 M18.4 7.6 L16.7 9.3 M7 20.5 H17",
                PHASE_STROKE,
            ),
        ],
    ),
    // The bug report's whole-window button, components/BugReport.tsx: four corners stroked 2.2
    // wide (`.bug-whole`, bug-report.css).
    button(
        WHOLE,
        &[stroked("M4 9V4h5M15 4h5v5M20 15v5h-5M9 20H4v-5", 2.2)],
    ),
    // `MenuIcon`, mobile/chrome.tsx: three bars stroked 2 wide (`.phone-menu`, phone.css).
    button(MENU, &[stroked("M5 7h14M5 12h14M5 17h14", 2.0)]),
    // The lock of a permanent that doesn't untap, `MARK_ICONS.lock` in components/Card.tsx:
    // filled even-odd, so the shackle's hole is left empty. It is drawn on the card's face
    // (`kit_base::geometry::badges`), not in the flat layer.
    Artwork {
        icon: LOCK,
        size: LOCK_SIDE,
        view: BOX_16,
        shapes: &[Shape {
            data: "M4 7 V5 A4 4 0 0 1 12 5 V7 H14 V15 H2 V7 Z M6 7 H10 V5 A2 2 0 0 0 6 5 Z",
            fill: Some(Fill {
                ink: 1.0,
                even_odd: true,
            }),
            stroke: None,
            rotate: None,
        }],
    },
];

thread_local! {
    static ICONS_RASTERIZED: Cell<u64> = const { Cell::new(0) };
}

/// How many icons this thread has rasterized. Only the glyph worker rasterizes them, so on any
/// other thread, such as the one that draws frames, this stays zero.
#[must_use]
pub fn icons_rasterized_on_this_thread() -> u64 {
    ICONS_RASTERIZED.with(Cell::get)
}

/// The side of `artwork`'s cell in texels at `scale` physical pixels per point.
#[must_use]
pub fn side(artwork: &Artwork, scale: f32) -> u16 {
    (f32::from(artwork.size) * scale).round().clamp(1.0, 1024.0) as u16
}

/// Draws `artwork` at `scale` physical pixels per point into a square of coverage. A shape
/// whose path data does not parse draws nothing.
#[must_use]
pub(crate) fn rasterize(artwork: &Artwork, scale: f32) -> Bitmap {
    draw(artwork, scale, false)
}

/// The cells of the drop shadows `shadows` (a chain of `icon_shadows::CHAINS`) of `artwork` at
/// `scale`, one for each shadow, as CSS applies a chain: shadow `k`'s cell is the coverage of
/// the icon and every shadow before it, composited, blurred by shadow `k`'s deviation
/// (`Shadow::drop_sigma`) and moved by its offset. Each cell is the icon's square grown by
/// `icon_shadows::stage_pad` pixels on every side, in texels of `stage_step` pixels, each the
/// mean of the pixels it covers. A drop shadow is cast by the icon's alpha, not its colors, so
/// every fill is drawn at full coverage first, whatever its `ink`.
#[must_use]
pub(crate) fn rasterize_shadows(artwork: &Artwork, scale: f32, shadows: &[Shadow]) -> Vec<Bitmap> {
    let Some(last) = shadows.len().checked_sub(1) else {
        return Vec::new();
    };
    let icon = draw(artwork, scale, true);
    let side = usize::from(icon.width);
    let pad = usize::from(icon_shadows::stage_pad(shadows, last, scale));
    let wide = side + 2 * pad;
    // The alpha of the icon and the shadows composited so far, on a canvas grown by the last
    // shadow's pad.
    let mut alpha = vec![0.0_f32; wide * wide];
    for y in 0..side {
        for x in 0..side {
            alpha[(y + pad) * wide + x + pad] = f32::from(icon.coverage[y * side + x]) / 255.0;
        }
    }
    let mut cells = Vec::with_capacity(shadows.len());
    for (stage, shadow) in shadows.iter().enumerate() {
        let blurred = blur::blur_plane(&alpha, wide, shadow.drop_sigma() * scale);
        let moved = moved_by(&blurred, wide, [shadow.x * scale, shadow.y * scale]);
        let opacity = f32::from(shadow.color[3]) / 255.0;
        for (under, &over) in alpha.iter_mut().zip(&moved) {
            *under += opacity * over * (1.0 - *under);
        }
        let reach = usize::from(icon_shadows::stage_pad(shadows, stage, scale));
        let step = usize::from(icon_shadows::stage_step(shadows, stage, scale));
        cells.push(cropped(&moved, wide, pad - reach, side + 2 * reach, step));
    }
    cells
}

/// `plane`, rows of `wide` values, moved by `by` pixels rounded to whole ones, with nothing
/// coming in past its edges.
fn moved_by(plane: &[f32], wide: usize, by: [f32; 2]) -> Vec<f32> {
    #[expect(clippy::cast_possible_truncation, reason = "an offset of a few pixels")]
    let [dx, dy] = by.map(|length| length.round() as isize);
    let tall = plane.len() / wide.max(1);
    let mut out = vec![0.0_f32; plane.len()];
    for y in 0..tall {
        for x in 0..wide {
            let from = (y.checked_add_signed(-dy), x.checked_add_signed(-dx));
            if let (Some(from_y), Some(from_x)) = from
                && from_y < tall
                && from_x < wide
            {
                out[y * wide + x] = plane[from_y * wide + from_x];
            }
        }
    }
    out
}

/// The square of `plane` (rows of `wide` values) `length` pixels across with its top left
/// corner at `start` on both axes, as a cell of coverage in texels of `step` by `step` pixels,
/// each the mean of the pixels it covers, a texel past the square's edge counting the pixels
/// beyond as empty.
fn cropped(plane: &[f32], wide: usize, start: usize, length: usize, step: usize) -> Bitmap {
    let step = step.max(1);
    let texels = length.div_ceil(step);
    let mut coverage = vec![0_u8; texels * texels];
    #[expect(clippy::cast_precision_loss, reason = "a small count")]
    let block = (step * step) as f32;
    for row in 0..texels {
        for column in 0..texels {
            let mut sum = 0.0;
            for y in row * step..((row + 1) * step).min(length) {
                for x in column * step..((column + 1) * step).min(length) {
                    sum += plane[(start + y) * wide + start + x];
                }
            }
            coverage[row * texels + column] = blur::level(sum / block);
        }
    }
    let side = u16::try_from(texels).unwrap_or(u16::MAX);
    Bitmap {
        width: side,
        height: side,
        bearing: [0, 0],
        coverage,
    }
}

/// Draws `artwork` as `rasterize` does, or with every fill at full coverage if `solid`.
fn draw(artwork: &Artwork, scale: f32, solid: bool) -> Bitmap {
    ICONS_RASTERIZED.with(|count| count.set(count.get() + 1));
    let side = side(artwork, scale);
    let mut pixmap = Pixmap::new(u32::from(side), u32::from(side)).expect("a cell has an area");
    let [left, top, width, height] = artwork.view;
    let fit = f32::from(side) / width.max(height);
    let base = Transform::from_row(
        fit,
        0.0,
        0.0,
        fit,
        (f32::from(side) - width * fit) / 2.0 - left * fit,
        (f32::from(side) - height * fit) / 2.0 - top * fit,
    );
    for shape in artwork.shapes {
        let Some(path) = svg::parse(shape.data) else {
            continue;
        };
        let transform = shape.rotate.map_or(base, |[degrees, x, y]| {
            base.pre_concat(Transform::from_rotate_at(degrees, x, y))
        });
        if let Some(fill) = shape.fill {
            let rule = if fill.even_odd {
                FillRule::EvenOdd
            } else {
                FillRule::Winding
            };
            let ink = if solid { 1.0 } else { fill.ink };
            pixmap.fill_path(&path, &paint(ink), rule, transform, None);
        }
        if let Some(stroke) = shape.stroke {
            let stroke = Stroke {
                width: stroke.width,
                line_cap: LineCap::Round,
                line_join: LineJoin::Round,
                ..Stroke::default()
            };
            pixmap.stroke_path(&path, &paint(1.0), &stroke, transform, None);
        }
    }
    // The pixels are premultiplied, so a gray of `ink` painted opaque leaves its red channel
    // at `ink` times the coverage.
    let coverage = pixmap.pixels().iter().map(|pixel| pixel.red()).collect();
    Bitmap {
        width: side,
        height: side,
        bearing: [0, 0],
        coverage,
    }
}

/// An opaque gray of brightness `ink`, antialiased.
fn paint(ink: f32) -> Paint<'static> {
    let gray = (ink.clamp(0.0, 1.0) * 255.0).round() as u8;
    let mut paint = Paint::default();
    paint.set_color_rgba8(gray, gray, gray, 255);
    paint.anti_alias = true;
    paint
}

/// The cell of `artwork` placed at `rect`, or of one of its drop shadows (`IconCell::shadow`).
#[must_use]
pub(crate) fn cell(artwork: &Artwork, rect: [u16; 4], shadow: u8) -> IconCell {
    IconCell {
        icon: artwork.icon,
        size: artwork.size,
        rect,
        shadow,
    }
}

/// The artwork of `icon`, if it is one of `ALL`.
#[must_use]
pub(crate) fn artwork(icon: IconId) -> Option<&'static Artwork> {
    ALL.iter().find(|artwork| artwork.icon == icon)
}

#[cfg(test)]
mod tests {
    use kit_ui::widgets::icons::{ORB, atlas_size};

    use super::ALL;
    use crate::svg;

    /// The test driver's stub atlas, and the "drawn" check it serves, hold each icon at
    /// `atlas_size`, so every icon's artwork must be drawn at that size, and each id once.
    #[test]
    fn every_icon_is_drawn_at_its_atlas_size() {
        assert_eq!(ALL.len(), usize::from(ORB));
        for (index, artwork) in ALL.iter().enumerate() {
            assert_eq!(usize::from(artwork.icon.0), index, "{:?}", artwork.icon);
            assert_eq!(artwork.size, atlas_size(artwork.icon), "{:?}", artwork.icon);
        }
    }

    #[test]
    fn every_shape_of_every_icon_is_valid_path_data() {
        for artwork in &ALL {
            for shape in artwork.shapes {
                assert!(
                    svg::parse(shape.data).is_some(),
                    "{:?}: {}",
                    artwork.icon,
                    shape.data
                );
            }
        }
    }
}
