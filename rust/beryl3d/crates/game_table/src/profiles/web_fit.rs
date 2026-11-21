//! The desktop layout's battlefield rows, fitted to a window so that each card lands where the
//! web interface draws it (docs/beryl-3d.md, "Matching the web interface's look").
//!
//! The web interface lays the table out on a stage 900 pixels high, scaled to fit the window
//! (`ui/src/game/Stage.tsx`), and draws each card as an upright rectangle shrunk by how far up
//! the stage it lies and leaned toward a vanishing point over the stage's middle
//! (`ui/src/game/perspective.ts`). Its rows are stage positions: `ROWS` in
//! `ui/src/game/tableRows.ts` and `sideRows` in `ui/src/game/battlefield.ts`, copied into
//! the desktop profile.
//!
//! Beryl 3D keeps its own camera, fitted to Arena (the profile's `table.camera`), and lays its cards
//! flat on a table. For each row it finds the line of the table under the web's cards' centres,
//! the row's ends, and the scale at which a card is as wide on screen as the web's, so that each
//! card's centre projects onto the centre of the web's card. A row is laid out in millimetres of
//! a card at its printed size and then scaled by that scale about its left end (`place::row`).
//! That scale is a row's at full size: a crowded row shrinks its cards further, and may shrink
//! every row with it, as the web's `battlefieldZoom` does (`place::zooms`).
//! A card lying flat under the camera's pitch is shorter on screen against its width than the
//! web's upright one, so each row's cards lean back toward the camera until they are as tall.
//! It is also a trapezoid, its far edge foreshortened, where the web's is a parallelogram, so
//! each card is sheared in the table's plane until its box on screen is as wide as the web's
//! (`lean`).

use glam::{Vec2, Vec3};

use crate::table::pile::lift;
use crate::table::{CARD_HEIGHT, CARD_WIDTH, Camera, LAYER, RowArea, Window};
use kit_base::data::profiles::{Perspective, StageSize, WebFit};
use kit_base::pose::Pose;

/// The web interface's `CARD_ASPECT`, a card's width over its height.
const ASPECT: f32 = CARD_WIDTH / CARD_HEIGHT;

/// One battlefield row of one side fitted to the web's: its area on the table, the scale its
/// cards are drawn at, and the gaps between them in millimetres before that scale.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FittedRow {
    stage: Stage,
    /// The row's band, in millimetres of the table. Its `y` is the line the centres of its
    /// untapped cards lie on, signed for its side, and its `left` and `right` are where the
    /// edges of its first and last cards may reach along that line.
    pub area: RowArea,
    /// How large its cards are against a card's printed size while the row has room for them
    /// at full size (`place::zooms`).
    pub scale: f32,
    /// How far apart its cards stand, before `scale`.
    pub gap: f32,
    /// How far its first tapped card stands from the untapped card before it, before `scale`:
    /// the web's `TAPPED_GAP` in the lands' row, and `gap` in the others.
    pub tapped_gap: f32,
    /// How far its cards lean back from lying flat, toward the camera, in radians: as far as
    /// makes an untapped card as tall on screen as the web's upright one, which a card lying flat
    /// under the camera's pitch is not. Every card of the row lies in one plane, which rises
    /// toward the far side of the table at this angle from the near edge of an untapped card
    /// (`FittedRow::height`), so that a card placed later lies over one placed before it,
    /// wherever they overlap.
    pub tilt: f32,
    /// How far along the table an attacking card's centre moves from the row's line, signed,
    /// untapped then tapped. The web moves an attacker `ADVANCE` stage pixels toward the other
    /// side (`permanentRow` in `ui/src/game/battlefield.ts`), and lines a tapped card up with
    /// the row by its top, where Beryl 3D keeps it centred on the row's line, so a tapped
    /// attacker moves as far again as makes its centre the web's. Cards that do not attack keep
    /// to the line, for the hit test's sake (`place::row`): an attacker stands out of its row.
    pub advance: [f32; 2],
    /// How wide the web draws an untapped card of the row on the window, before its lean, in
    /// logical points: its frame's width shrunk by the row's depth (`depthScale`).
    pub width: f32,
}

impl FittedRow {
    /// How high above the table the row's plane lies over `y`, at its cards' scale.
    #[must_use]
    pub fn height(&self, y: f32) -> f32 {
        let half = CARD_HEIGHT / 2.0 * self.scale;
        half * self.tilt.sin() + (y - self.area.y) * self.tilt.tan()
    }
}

/// Each side's three rows, the viewer's first, each in the order of `place::ROWS`.
pub type FittedRows = [[FittedRow; 3]; 2];

/// The web interface's stage as it is fitted to a window (`fit` in `ui/src/game/Stage.tsx`).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Stage {
    /// How wide the stage is, in stage pixels: as wide as the window's shape asks, within
    /// `STAGE_WIDTHS`.
    width: f32,
    /// Logical points per stage pixel.
    scale: f32,
    /// How far the stage's top left corner lies from the window's, in stage pixels.
    offset: Vec2,
    base: f32,
    perspective: Perspective,
}

impl Stage {
    fn of(window: Window, stage: StageSize, perspective: Perspective) -> Self {
        let size = window.size;
        let width = (stage.height * size.width / size.height).clamp(stage.least, stage.most);
        let scale = (size.width / width).min(size.height / stage.height);
        Self {
            width,
            scale,
            base: stage.base,
            perspective,
            offset: Vec2::new(
                (size.width / scale - width) / 2.0,
                (size.height / scale - stage.height) / 2.0,
            ),
        }
    }

    /// A point of the stage on the window, in logical points.
    fn on_window(&self, point: Vec2) -> Vec2 {
        (point + self.offset) * self.scale
    }

    /// The web's `fromRight`: what stands at the right keeps its distance from the right edge.
    fn right_held(&self, x: f32) -> f32 {
        x + self.width - self.base
    }

    /// The web's `fromMiddle`: what stands at the middle keeps its distance from the middle.
    fn middle_held(&self, x: f32) -> f32 {
        x + (self.width - self.base) / 2.0
    }

    /// The tangent of the web's `lean` of a thing whose top centre is at (`x`, `y`): how far its
    /// sides run sideways for each pixel down.
    fn lean(&self, x: f32, y: f32) -> f32 {
        (x - self.width / 2.0) / (y - self.perspective.vanishing_y)
    }

    /// The web's `depthScale`: how much a thing at stage height `y` is shrunk by distance.
    fn depth_scale(&self, y: f32) -> f32 {
        let Perspective {
            distance,
            near_y,
            depth_per_px,
            ..
        } = self.perspective;
        distance / (distance + (near_y - y).max(0.0) * depth_per_px)
    }
}

/// One of the web's rows of permanents (`sideRows`), in stage pixels at full zoom.
struct RowSpec {
    /// Where its cards' tops lie.
    top: f32,
    /// Where its first card's left edge may stand, and its last card's right edge.
    start: f32,
    end: f32,
    /// Where it is centred while it fits there; none for a row laid from `start`.
    centre: Option<f32>,
    /// A card's frame width, before depth scaling.
    frame: f32,
    /// The gaps between cards, and before the first tapped card, before depth scaling.
    gap: f32,
    tapped_gap: f32,
}

/// The rows of `side` (the viewer's first) fitted to `window` through `camera`.
fn side_rows(stage: &Stage, camera: &Camera, data: &WebFit, opponent: bool) -> [FittedRow; 3] {
    let rows = if opponent { data.opponent } else { data.you };
    let creatures = RowSpec {
        top: rows.permanents,
        start: data.room.start,
        end: stage.right_held(data.room.end),
        centre: Some(stage.width / 2.0),
        frame: data.creature_width,
        gap: data.permanent_gap * data.creature_width,
        tapped_gap: data.permanent_gap * data.creature_width,
    };
    let lands = RowSpec {
        top: rows.back,
        start: rows.lands,
        end: stage.middle_held(rows.lands_end),
        centre: None,
        frame: data.land_width,
        gap: data.land_gap,
        tapped_gap: data.tapped_gap,
    };
    let others = RowSpec {
        top: rows.back,
        start: stage.middle_held(rows.others_x),
        end: stage.right_held(rows.others_end),
        centre: None,
        frame: rows.others_width,
        gap: data.permanent_gap * rows.others_width,
        tapped_gap: data.permanent_gap * rows.others_width,
    };
    // Toward the other side: up the stage on the viewer's side, down it on the opponent's.
    let forward = if opponent {
        data.advance
    } else {
        -data.advance
    };
    [creatures, lands, others].map(|spec| fit_row(stage, camera, &spec, forward))
}

/// `spec` fitted through `camera`. Along a line of the screen, a card's place across the table
/// is linear in its place across the screen, as a point's place across the web's stage is linear
/// in the x of the card it is the centre of: the web's lean moves a card's centre sideways by an
/// amount linear in its x. So the row's ends, its scale and its gaps follow from where two of its
/// points land. Its line and its cards' tilt are found by `Stand::fit`.
fn fit_row(stage: &Stage, camera: &Camera, spec: &RowSpec, forward: f32) -> FittedRow {
    let width = spec.frame * stage.depth_scale(spec.top);
    let height = width / ASPECT;
    let line = stage.on_window(Vec2::new(0.0, spec.top + height / 2.0)).y;
    // The centre of the web's card whose top left corner is at `x`, leaned about its top.
    let centre_of = |x: f32| x + width / 2.0 + stage.lean(x + width / 2.0, spec.top) * height / 2.0;
    let across = |x: f32, lift: f32| {
        let point = stage.on_window(Vec2::new(x, 0.0));
        camera.on_plane(Vec2::new(point.x, line), lift).x
    };
    // Millimetres of the table for each stage pixel between two cards' left edges, at `lift`.
    let per_pixel = |lift: f32| {
        (across(centre_of(spec.end - width), lift) - across(centre_of(spec.start), lift))
            / (spec.end - width - spec.start)
    };
    let mut stand = Stand {
        y: camera.on_plane(Vec2::new(0.0, line), 0.0).y,
        tilt: 0.0,
        scale: width * per_pixel(0.0) / CARD_WIDTH,
    };
    // The scale and the stand depend on each other only a little: two rounds settle both.
    for _ in 0..2 {
        stand = stand.fit(camera, line, height * stage.scale);
        stand.scale = width * per_pixel(stand.lift(CARD_HEIGHT)) / CARD_WIDTH;
    }
    let lift = stand.lift(CARD_HEIGHT);
    let scale = stand.scale;
    // A gap of the web's, against its card's frame, is the same share of a card here.
    let gap = |pixels: f32| CARD_WIDTH * pixels / spec.frame;
    let row = FittedRow {
        stage: *stage,
        area: RowArea {
            y: stand.y,
            left: across(centre_of(spec.start), lift) - CARD_WIDTH * scale / 2.0,
            right: across(centre_of(spec.end - width), lift) + CARD_WIDTH * scale / 2.0,
            centre: spec.centre.map(|x| across(x, lift)),
        },
        scale,
        gap: gap(spec.gap),
        tapped_gap: gap(spec.tapped_gap),
        tilt: stand.tilt,
        advance: [0.0; 2],
        width: width * stage.scale,
    };
    // A tapped card's centre lies half the difference of its height and width higher on the
    // web's stage than an untapped one's, both tops on the row's top.
    let rise = -(height - width) / 2.0;
    let moved = |pixels: f32| {
        let goal = line + pixels * stage.scale;
        let moved = Stand::solve(row.area.y, |y| {
            camera.to_screen(Vec3::new(0.0, y, row.height(y))).y - goal
        });
        moved - row.area.y
    };
    FittedRow {
        advance: [moved(forward), moved(forward + rise)],
        ..row
    }
}

/// How the cards of a row stand: the line of the table their centres lie over, how far each
/// leans back from lying flat toward the camera, about the table's x axis, and their scale.
#[derive(Clone, Copy, Debug)]
struct Stand {
    y: f32,
    /// In radians.
    tilt: f32,
    scale: f32,
}

/// The most a row's cards lean toward the camera, in radians: well short of standing upright.
const MOST_TILT: f32 = 1.2;

impl Stand {
    /// How far above the table the centre of a card `length` millimetres long from its near edge
    /// to its far one stands, before the scale, with its near edge on the table.
    fn lift(self, length: f32) -> f32 {
        length / 2.0 * self.scale * self.tilt.sin()
    }

    /// The top and bottom on screen, in logical points, of a card `length` millimetres long
    /// whose centre lies `up` millimetres over `y`, leaning back.
    fn extent(self, camera: &Camera, (y, up): (f32, f32), length: f32) -> (f32, f32) {
        let half = length / 2.0 * self.scale;
        let (along, rise) = (half * self.tilt.cos(), half * self.tilt.sin());
        let top = camera.to_screen(Vec3::new(0.0, y + along, up + rise)).y;
        let bottom = camera.to_screen(Vec3::new(0.0, y - along, up - rise)).y;
        (top, bottom)
    }

    /// `extent` of an untapped card whose centre lies over `y`, with its near edge on the table.
    fn untapped(self, camera: &Camera, y: f32) -> (f32, f32) {
        self.extent(camera, (y, self.lift(CARD_HEIGHT)), CARD_HEIGHT)
    }

    /// The stand whose untapped card is `height` points tall on screen with its middle on the
    /// screen's line `line`, as the web's upright card is: the tilt found by bisection, the line
    /// of the table by the secant method, each in turn until both settle. A card grows taller on
    /// screen as it leans back only until it faces the camera, and shorter past that, so the
    /// bisection looks no further than that tilt. Near the bottom of the window, where the camera
    /// looks down more steeply, it faces the camera after a small lean, and a card that cannot
    /// grow as tall as `height` stands at that tilt, as tall as it can be.
    fn fit(self, camera: &Camera, line: f32, height: f32) -> Stand {
        let mut stand = self;
        for _ in 0..4 {
            let tall = |tilt: f32| {
                let (top, bottom) = Stand { tilt, ..stand }.untapped(camera, stand.y);
                bottom - top
            };
            // The tallest tilt, by ternary search.
            let (mut low, mut high) = (0.0, MOST_TILT);
            for _ in 0..32 {
                let (a, b) = (low + (high - low) / 3.0, high - (high - low) / 3.0);
                if tall(a) < tall(b) {
                    low = a;
                } else {
                    high = b;
                }
            }
            let (mut low, mut high) = (0.0, f32::midpoint(low, high));
            for _ in 0..24 {
                let tilt = f32::midpoint(low, high);
                if tall(tilt) < height {
                    low = tilt;
                } else {
                    high = tilt;
                }
            }
            stand.tilt = low;
            stand.y = Stand::solve(stand.y, |y| {
                let (top, bottom) = stand.untapped(camera, y);
                f32::midpoint(top, bottom) - line
            });
        }
        stand
    }

    /// The `y` near `start` where `miss` is nought, by the secant method.
    fn solve(start: f32, miss: impl Fn(f32) -> f32) -> f32 {
        let (mut a, mut b) = (start, start + 1.0);
        let (mut miss_a, mut miss_b) = (miss(a), miss(b));
        for _ in 0..8 {
            if (miss_b - miss_a).abs() < f32::EPSILON {
                break;
            }
            let next = b - miss_b * (b - a) / (miss_b - miss_a);
            (a, miss_a) = (b, miss_b);
            (b, miss_b) = (next, miss(next));
        }
        b
    }
}

/// `pose`, a card of the row `row` zoomed out by `zoom` (`place::zooms`) as `place::row` lays
/// it, sheared across the table and moved along `x` so that its box on screen is the web's. The
/// web draws a permanent as an upright
/// rectangle skewed toward its vanishing point about its top (`onTable` and `footprint` in
/// `ui/src/game/placement.ts`), a parallelogram whose box is as wide as the card plus the
/// lean's tangent times its height, centred on the card. A card lying flat under this camera
/// is a trapezoid whose far edge is foreshortened, whose box is narrower, the more so the
/// farther from the screen's middle it lies. A shear in the table's plane (`Pose::shear`)
/// moves its far edge sideways, widening its box by about as much as it moves, and a move
/// along `x` centres that box where the web's is. A `tapped` card is sheared the same way, as
/// the web skews its lying box. Near the screen's middle, where the web's card barely leans, a
/// trapezoid's near edge alone can be wider than the web's box, and no shear narrows it: such a
/// card is only moved, to centre its box on the web's.
#[must_use]
pub fn lean((row, zoom): (&FittedRow, f32), camera: &Camera, pose: Pose, tapped: bool) -> Pose {
    let stage = row.stage;
    // A card at rest is face up or face down, which outlines the same shape. It is measured face
    // up, so that a card placed face up and turned over later gets the shear it would have been
    // placed with face down, to the last bit.
    let flat = Pose {
        shear: 0.0,
        turn: 0.0,
        ..pose
    };
    let transform = flat.transform();
    let span = |shear: f32, moved: f32| {
        // Each trial sets the table-space shear and x offset of the same face. Reuse its
        // rotation, scale and local skew, and move the centre after shearing around it.
        let mut trial = Pose { shear, ..flat }.sheared(transform);
        trial.translation.x += moved;
        let corners = camera.card_corners(trial);
        corners
            .iter()
            .fold((f32::MAX, f32::MIN, f32::MAX), |(l, r, t), c| {
                (l.min(c.x), r.max(c.x), t.min(c.y))
            })
    };
    let (_, _, top) = span(0.0, 0.0);
    // The web's card across the screen and down it: a tapped card lies turned a quarter.
    let width = row.width * zoom;
    let (wide, tall) = if tapped {
        (width / ASPECT, width)
    } else {
        (width, width / ASPECT)
    };
    let centre = camera.to_screen(pose.position).x;
    // The web's lean is taken at the middle of its card's top edge, which lies the lean's
    // tangent times half the card's height from the card's centre.
    let on_stage = |point: f32, offset: f32| point / stage.scale - offset;
    let (middle, top) = (
        on_stage(centre, stage.offset.x),
        on_stage(top, stage.offset.y),
    );
    let half_tall = tall / stage.scale / 2.0;
    let rough = stage.lean(middle, top);
    let tangent = stage.lean(middle - rough * half_tall, top);
    // The row places a card's centre where an untapped card's leaned centre lies, half the
    // tangent times its height from its top's middle; a tapped card is less tall, so the web's
    // centre lies nearer its top's middle.
    let centre = if tapped {
        centre + tangent * (tall - width / ASPECT) / 2.0
    } else {
        centre
    };
    let half = f32::midpoint(wide, tangent.abs() * tall);
    let goal = (centre - half, centre + half);
    let miss = |shear: f32, moved: f32| {
        let (left, right, _) = span(shear, moved);
        (left - goal.0, right - goal.1)
    };
    // The shear and the move by Newton's method: the box's ends are nearly linear in both.
    let (mut shear, mut moved) = (0.0, 0.0);
    for _ in 0..4 {
        let now = miss(shear, moved);
        let by_shear = miss(shear + SHEAR_STEP, moved);
        let by_move = miss(shear, moved + 1.0);
        let (a, c) = (
            (by_shear.0 - now.0) / SHEAR_STEP,
            (by_shear.1 - now.1) / SHEAR_STEP,
        );
        let (b, d) = (by_move.0 - now.0, by_move.1 - now.1);
        let determinant = a * d - b * c;
        if determinant.abs() < f32::EPSILON {
            break;
        }
        shear -= (d * now.0 - b * now.1) / determinant;
        moved -= (a * now.1 - c * now.0) / determinant;
    }
    let (left, right) = miss(shear, moved);
    if left.abs().max(right.abs()) > CLOSE {
        // No shear gives the web's box: centre the unsheared one on it.
        shear = 0.0;
        moved = 0.0;
        for _ in 0..2 {
            let (left, right) = miss(0.0, moved);
            moved -= f32::midpoint(left, right);
        }
    }
    Pose {
        shear,
        position: pose.position + Vec3::X * moved,
        ..pose
    }
}

/// How near, in logical points, `lean` must bring a box's ends to the web's to keep its shear.
const CLOSE: f32 = 0.25;

/// The step in `Pose::shear` by which `lean` measures how the box's ends follow the shear.
const SHEAR_STEP: f32 = 0.01;

/// Every row of the desktop layout fitted to `window` through `camera`.
pub(super) fn rows(window: Window, camera: &Camera, data: &WebFit) -> FittedRows {
    let stage = Stage::of(window, data.stage, data.perspective);
    [false, true].map(|opponent| side_rows(&stage, camera, data, opponent))
}

/// The library, graveyard and exile piles of the desktop layout fitted to a window, so that
/// each pile's top card lands where the web interface draws it (`pileLook` in
/// `ui/src/game/pileLook.ts`). The web keeps a pile's top card at its anchor however many cards
/// the pile holds, so a pile here stands wherever puts its top card's centre under `tops`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FittedPiles {
    /// The middle of each pile's top card on the window, in logical points: by side, the
    /// viewer's first, then library, graveyard and exile.
    pub tops: [[Vec2; 3]; 2],
    /// How large each side's pile cards are against a card's printed size, the viewer's first.
    pub scale: [f32; 2],
    /// How far each side's pile cards lean back from lying flat toward the camera, in radians,
    /// as a fitted row's do (`FittedRow::tilt`), so that a top card is as tall on screen as the
    /// web's upright one.
    pub tilt: [f32; 2],
    /// How far each pile's cards are sheared, by side (the viewer's first) then library,
    /// graveyard and exile: the tangent of the web's `lean` of the pile's top card, as
    /// `Pose::skew` takes it.
    pub skew: [[f32; 3]; 2],
}

/// Every pile of the desktop layout fitted to `window` through `camera`. The web draws a pile's
/// top card as it draws a permanent: upright, shrunk by how far up the stage its anchor lies and
/// leaned toward the vanishing point by a skew about its top left corner (`PileButton` in
/// `ui/src/game/Piles.tsx`).
pub(super) fn piles(window: Window, camera: &Camera, data: &WebFit) -> FittedPiles {
    let piles = &data.piles;
    let stage = Stage::of(window, data.stage, data.perspective);
    let mut fitted = FittedPiles {
        tops: [[Vec2::ZERO; 3]; 2],
        scale: [1.0; 2],
        tilt: [0.0; 2],
        skew: [[0.0; 3]; 2],
    };
    for side in 0..2 {
        // The anchor's height on the stage, the card's unscaled width, the anchors' x, and how
        // far the top card lies below its anchor (`PILE_BASE`). The opponent's top cards lie
        // so that the deepest layer of the thickest pile, which spreads downward, ends
        // `clearance` above their back row.
        let (y, frame, xs, base) = if side == 0 {
            let xs = piles.you.x;
            (piles.you.y, piles.you.width, xs, piles.you.base)
        } else {
            let xs = piles.opponent.x.map(|x| stage.right_held(x));
            let height = piles.opponent.width * stage.depth_scale(piles.opponent.y) / ASPECT;
            let base = data.opponent.back
                - piles.clearance
                - piles.opponent.spread_down * piles.most_thickness
                - piles.opponent.y
                - height;
            (piles.opponent.y, piles.opponent.width, xs, base)
        };
        let width = frame * stage.depth_scale(y);
        let height = width / ASPECT;
        for (zone, x) in xs.into_iter().enumerate() {
            let middle = x + width / 2.0;
            let centre = Vec2::new(
                middle + stage.lean(middle, y) * height / 2.0,
                y + base + height / 2.0,
            );
            fitted.tops[side][zone] = stage.on_window(centre);
        }
        let stand = fit_card(
            camera,
            fitted.tops[side][0].y,
            (width * stage.scale, height * stage.scale),
        );
        fitted.scale[side] = stand.scale;
        fitted.tilt[side] = stand.tilt;
        // The camera already leans a card toward the vanishing point the further it lies from
        // the middle of the screen, so a card is sheared only by what the web's lean has over
        // that.
        for (zone, x) in xs.into_iter().enumerate() {
            let middle = x + width / 2.0;
            let top = fitted.tops[side][zone];
            let z = LAYER * 0.5 + lift(stand.scale, stand.tilt);
            let pose = Pose {
                position: camera.on_plane(top, z).truncate().extend(z),
                rotation: glam::Quat::from_rotation_x(stand.tilt),
                scale: stand.scale,
                ..Pose::default()
            };
            let [bottom_left, bottom_right, top_right, top_left] =
                camera.card_corners(pose.transform());
            let (head, foot) = (
                top_left.midpoint(top_right),
                bottom_left.midpoint(bottom_right),
            );
            let natural = (foot.x - head.x) / (foot.y - head.y);
            let skew = stage.lean(middle, y) - natural;
            fitted.skew[side][zone] = skew;
            // The camera's perspective moves the middle of a tilted card's outline a little off
            // the screen point its middle stands under, and the web's boxes are measured by
            // their outline, so the pile moves by what the outline is off.
            let sheared = Pose { skew, ..pose };
            let corners = camera.card_corners(sheared.transform());
            let low = corners
                .iter()
                .fold(Vec2::MAX, |low, corner| low.min(*corner));
            let high = corners
                .iter()
                .fold(Vec2::MIN, |high, corner| high.max(*corner));
            fitted.tops[side][zone] += top - low.midpoint(high);
        }
    }
    fitted
}

/// The stand of a card whose middle lies on the screen's line `line` and which is `size`
/// points wide and tall on screen there, as the web's upright card is. Under a camera that is
/// neither turned nor rolled, how wide and tall a card is on screen does not depend on where
/// across the table it lies. The scale and the stand depend on each other only a little: a few
/// rounds settle both.
fn fit_card(camera: &Camera, line: f32, (width, height): (f32, f32)) -> Stand {
    let mut stand = Stand {
        y: camera.on_plane(Vec2::new(0.0, line), 0.0).y,
        tilt: 0.0,
        scale: 1.0,
    };
    for _ in 0..4 {
        stand = stand.fit(camera, line, height);
        let up = stand.lift(CARD_HEIGHT);
        let half = CARD_WIDTH / 2.0 * stand.scale;
        let left = camera.to_screen(Vec3::new(-half, stand.y, up)).x;
        let right = camera.to_screen(Vec3::new(half, stand.y, up)).x;
        stand.scale *= width / (right - left);
    }
    stand.fit(camera, line, height)
}
