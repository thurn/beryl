//! The game screen's furniture drawn as the web interface draws it: action buttons and life orbs
//! in the colours and widths of the web's own rules. The expected values are read from the web
//! interface's files (`ui/src/design/components/buttons.css`, `life-orbs.css` and
//! `ui/src/components/Player.tsx`), and the drawn ones from the frame's draw records, not from
//! pixels, so the check holds at every scale and tier. No check compares whole pictures
//! (decision C5 in docs/beryl-3d.md); `just b3d compare` shows the two side by side.
//!
//! A panel's colour is checked twice: on its own record, and as what shows at its middle once
//! every flat-layer panel is drawn, so that a scrim or other panel drawn over a button, which
//! makes it look disabled, fails the check too.

use std::path::{Path, PathBuf};

use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use beryl_scene::agent::{Layout, hit};
use beryl_scene::driver::{Moment, Page};
use beryl_scene::frame::{Draw, Frame, Material, Role};
use beryl_scene::input::{AssetReady, GlyphCell, Rect, Size};
use glam::{Vec2, Vec3};

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

fn web_file(path: &str) -> String {
    let root: PathBuf = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../../ui/src");
    std::fs::read_to_string(root.join(path)).unwrap_or_else(|why| panic!("{path}: {why}"))
}

/// The value of `property` in the rule of `selector` in the CSS `sheet`, which must have both.
fn css(sheet: &str, selector: &str, property: &str) -> String {
    let start = sheet
        .find(&format!("\n{selector} {{"))
        .unwrap_or_else(|| panic!("no rule `{selector}`"));
    let body = &sheet[start..];
    let body = &body[..body.find("\n}").expect("the rule ends")];
    let at = body
        .find(&format!("\n  {property}:"))
        .unwrap_or_else(|| panic!("`{selector}` sets no `{property}`"));
    let value = &body[at + property.len() + 4..];
    value[..value.find(';').expect("the declaration ends")]
        .trim()
        .to_owned()
}

/// Every `#rrggbb` colour in a CSS value, in order, as opaque RGBA.
fn hex_colors(value: &str) -> Vec<[u8; 4]> {
    value
        .match_indices('#')
        .filter_map(|(at, _)| {
            let hex = value.get(at + 1..at + 7)?;
            let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
            Some([channel(0)?, channel(2)?, channel(4)?, 255])
        })
        .collect()
}

fn hex_color(value: &str) -> [u8; 4] {
    match hex_colors(value).as_slice() {
        [color] => *color,
        colors => panic!("`{value}` is not one colour but {colors:?}"),
    }
}

/// A length in CSS pixels, `1.5px` or `5`.
fn length(value: &str) -> f32 {
    let number = value.split_whitespace().next().expect("a value");
    number
        .trim_end_matches("px")
        .parse()
        .unwrap_or_else(|why| panic!("`{value}`: {why}"))
}

/// A flat-layer panel as its record draws it, in logical points on screen.
#[derive(Debug)]
struct Panel {
    /// Its index among the frame's records, which are in drawing order.
    index: usize,
    rect: Rect,
    tint: [u8; 4],
    edge: [u8; 4],
    border: f32,
}

/// The screen point, in logical points, of the table-space point `point`.
fn on_screen(frame: &Frame, point: Vec3) -> Vec2 {
    let clip = frame.view_projection.project_point3(point);
    Vec2::new(
        f32::midpoint(clip.x, 1.0) * WINDOW.width,
        (1.0 - clip.y) / 2.0 * WINDOW.height,
    )
}

/// The box on screen of a flat-layer record drawn on the unit square (`Mesh::Quad`).
fn drawn_rect(frame: &Frame, draw: &Draw) -> Rect {
    let m = draw.model;
    let centre = Vec3::new(m[3], m[7], m[11]);
    let across = Vec3::new(m[0], m[4], m[8]) / 2.0;
    let down = Vec3::new(m[1], m[5], m[9]) / 2.0;
    let corners = [
        centre - across - down,
        centre + across - down,
        centre - across + down,
        centre + across + down,
    ]
    .map(|corner| on_screen(frame, corner));
    let (min, max) = corners
        .iter()
        .fold((corners[0], corners[0]), |(min, max), &c| {
            (min.min(c), max.max(c))
        });
    Rect {
        x: min.x,
        y: min.y,
        width: max.x - min.x,
        height: max.y - min.y,
    }
}

/// Every panel of the flat layer, in drawing order.
fn panels(frame: &Frame) -> Vec<Panel> {
    (frame.layer as usize..frame.draws.len())
        .filter(|&index| frame.draws[index].material == Material::Panel as u8)
        .map(|index| {
            let draw = &frame.draws[index];
            Panel {
                index,
                rect: drawn_rect(frame, draw),
                tint: draw.tint,
                #[allow(clippy::cast_possible_truncation)]
                edge: draw.uv.map(|channel| channel as u8),
                border: draw.params[3],
            }
        })
        .collect()
}

fn contains(rect: Rect, point: Vec2) -> bool {
    point.x >= rect.x
        && point.x <= rect.x + rect.width
        && point.y >= rect.y
        && point.y <= rect.y + rect.height
}

fn same_rect(a: Rect, b: Rect) -> bool {
    (a.x - b.x).abs() < 0.5
        && (a.y - b.y).abs() < 0.5
        && (a.width - b.width).abs() < 0.5
        && (a.height - b.height).abs() < 0.5
}

/// The panel drawn on exactly `rect`.
fn panel_on(frame: &Frame, rect: Rect) -> Panel {
    let mut found: Vec<Panel> = panels(frame)
        .into_iter()
        .filter(|panel| same_rect(panel.rect, rect))
        .collect();
    assert!(!found.is_empty(), "no panel is drawn on {rect:?}");
    found.pop().expect("one panel")
}

/// The colour that shows at `point` once every flat-layer panel is drawn, each blended over
/// those before it, and the panels drawn over `own` there. Corners are taken as square, so
/// `point` should be well inside them.
fn shown_at(frame: &Frame, point: Vec2, own: &Panel) -> ([u8; 3], Vec<Panel>) {
    let mut color = [0.0_f32; 3];
    let mut over = Vec::new();
    for panel in panels(frame) {
        if !contains(panel.rect, point) {
            continue;
        }
        let inner = Rect {
            x: panel.rect.x + panel.border,
            y: panel.rect.y + panel.border,
            width: panel.rect.width - 2.0 * panel.border,
            height: panel.rect.height - 2.0 * panel.border,
        };
        let fill = if contains(inner, point) {
            panel.tint
        } else {
            panel.edge
        };
        if fill[3] == 0 {
            continue;
        }
        let alpha = f32::from(fill[3]) / 255.0;
        for (channel, value) in color.iter_mut().zip(fill) {
            *channel = f32::from(value) * alpha + *channel * (1.0 - alpha);
        }
        if panel.index > own.index {
            over.push(panel);
        }
    }
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    (color.map(|channel| channel.round() as u8), over)
}

/// The tints of the glyphs drawn inside `rect`.
fn glyph_tints(frame: &Frame, rect: Rect) -> Vec<[u8; 4]> {
    (frame.layer as usize..frame.draws.len())
        .map(|index| &frame.draws[index])
        .filter(|draw| draw.material == Material::Glyph as u8)
        .filter(|draw| {
            let r = drawn_rect(frame, draw);
            contains(rect, Vec2::new(r.x + r.width / 2.0, r.y + r.height / 2.0))
        })
        .map(|draw| draw.tint)
        .collect()
}

/// How a web rule draws a button: its face, rim, label and rim width. A gradient face or a
/// rim lit unevenly lists every colour the web uses; the flat layer draws one of them.
struct ButtonLook {
    faces: Vec<[u8; 4]>,
    rims: Vec<[u8; 4]>,
    ink: [u8; 4],
    border: f32,
}

fn button_look(selector: &str) -> ButtonLook {
    let sheet = web_file("design/components/buttons.css");
    ButtonLook {
        faces: hex_colors(&css(&sheet, selector, "background")),
        rims: hex_colors(&css(&sheet, selector, "border-color")),
        ink: hex_color(&css(&sheet, selector, "color")),
        border: length(&css(&sheet, ".button", "border")),
    }
}

/// The box of the first button named `name` in the frame, in tree order.
fn first_button(frame: &Frame, name: &str) -> Rect {
    frame
        .resolved
        .iter()
        .find(|element| element.role == Role::Button && frame.name(element.name) == name)
        .map_or_else(|| panic!("no button named \"{name}\""), hit::bounds)
}

/// Checks that the first button named `name` on `page` is drawn as `look` says.
fn assert_button_drawn(page: &mut Page, name: &str, look: &ButtonLook, what: &str) {
    let frame = page.frame();
    let rect = first_button(frame, name);
    let panel = panel_on(frame, rect);
    let mut faults = Vec::new();
    if !look.faces.contains(&panel.tint) {
        faults.push(format!(
            "its face is {:?}, not one of the web's {:?}",
            panel.tint, look.faces
        ));
    }
    if !look.rims.contains(&panel.edge) {
        faults.push(format!(
            "its rim is {:?}, not one of the web's {:?}",
            panel.edge, look.rims
        ));
    }
    if (panel.border - look.border).abs() > 0.01 {
        faults.push(format!(
            "its rim is {} points wide, not the web's {}",
            panel.border, look.border
        ));
    }
    let middle = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let (shown, over) = shown_at(frame, middle, &panel);
    if shown != [panel.tint[0], panel.tint[1], panel.tint[2]] {
        let over: Vec<String> = over
            .iter()
            .map(|p| format!("a panel of {:?} on {:?}", p.tint, p.rect))
            .collect();
        faults.push(format!(
            "its middle shows {shown:?}, not its face, under {}",
            over.join(", ")
        ));
    }
    let tints = glyph_tints(frame, rect);
    if tints.is_empty() {
        faults.push("no glyph of its label is drawn".to_owned());
    }
    if let Some(tint) = tints.iter().find(|tint| **tint != look.ink) {
        faults.push(format!(
            "its label is drawn in {tint:?}, not the web's {:?}",
            look.ink
        ));
    }
    assert!(
        faults.is_empty(),
        "button \"{name}\", a {what} button: {}",
        faults.join("; ")
    );
}

/// `name` from the viewer's seat at `moment`, with a made-up atlas cell for every glyph its
/// text wants, so that labels are drawn: the test driver rasterizes none itself.
fn page_at(name: &str, moment: &str) -> Page {
    let mut page =
        pages::page(name, Side::You, Layout::Desktop, WINDOW).unwrap_or_else(|why| panic!("{why}"));
    page.seek(&moment.parse::<Moment>().expect("a moment"))
        .unwrap_or_else(|why| panic!("{why}"));
    let glyphs = (page.wanted_glyphs().into_iter())
        .map(|key| GlyphCell {
            key,
            rect: [0, 0, 20, 24],
            bearing: [0, -20],
        })
        .collect();
    page.give_glyphs(AssetReady::Glyphs { page: 0, glyphs });
    page
}

/// The play-or-draw prompt's buttons, which the opening screen's scrim once covered, so that
/// the Play button looked disabled though it could be pressed.
#[test]
fn the_play_or_draw_buttons_are_drawn_as_the_webs_enabled_buttons() {
    let mut page = page_at("opening-play", "prompt:1");
    assert_button_drawn(
        &mut page,
        "Play",
        &button_look(".button-primary"),
        "primary",
    );
    assert_button_drawn(
        &mut page,
        "Draw",
        &button_look(".button-secondary"),
        "secondary",
    );
}

/// The component gallery's buttons: primary, secondary and disabled, as the web's gallery shows
/// them side by side.
#[test]
fn the_gallery_buttons_are_drawn_as_the_webs_in_each_state() {
    let mut page = page_at("screen/gallery", "end");
    assert_button_drawn(
        &mut page,
        "Done",
        &button_look(".button-primary"),
        "primary",
    );
    assert_button_drawn(
        &mut page,
        "View Battlefield",
        &button_look(".button-secondary"),
        "secondary",
    );
    assert_button_drawn(
        &mut page,
        "Pass",
        &button_look(".button.button:disabled"),
        "disabled",
    );
}

/// Both life orbs: the core's fill and the bezel's width of the web's `.life-orb-core` and
/// `BEZEL`, and the active player's gold of `.life-orb-ring`.
#[test]
fn the_life_orbs_are_drawn_in_the_webs_core_bezel_and_ring() {
    let sheet = web_file("design/components/life-orbs.css");
    let core = hex_color(&css(&sheet, ".life-orb-core", "fill"));
    let bezel = length(&css(&sheet, ".life-orb-core", "stroke-width"));
    let ring = hex_color(&css(&sheet, ".life-orb-ring", "stroke"));
    let player = web_file("components/Player.tsx");
    assert!(
        player.contains(&format!("export const BEZEL = {bezel};")),
        "Player.tsx's `BEZEL` is not life-orbs.css's stroke width {bezel}"
    );
    let mut page = page_at("pay-default", "prompt:1");
    for (locator, active) in [
        ("region \"your orb\"", true),
        ("region \"opponent's orb\"", false),
    ] {
        let rect = page.find(locator).rect();
        let frame = page.frame();
        let panel = panel_on(frame, rect);
        assert_eq!(panel.tint, core, "{locator}'s core");
        assert!(
            (panel.border - bezel).abs() < 0.01,
            "{locator}'s bezel is {} points wide, not the web's {bezel}",
            panel.border
        );
        if active {
            assert_eq!(panel.edge, ring, "{locator}, the active player's, ring");
        } else {
            assert_ne!(panel.edge, ring, "{locator} is gold though not active");
        }
        // A point of the core beside the life total, which is drawn over the core's middle.
        let beside = Vec2::new(rect.x + rect.width / 6.0, rect.y + rect.height / 2.0);
        let (shown, over) = shown_at(frame, beside, &panel);
        assert_eq!(
            shown,
            [core[0], core[1], core[2]],
            "{locator}'s core is drawn over by {over:?}"
        );
    }
}
