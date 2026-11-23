//! Checks the play-or-draw buttons and life orbs using native frame draw records.
//!
//! A panel's colour is checked twice: on its own record, and as what shows at its middle once
//! every flat-layer panel is drawn, so that a scrim or other panel drawn over a button, which
//! makes it look disabled, fails the check too.

use app_client::driver::{Moment, Page};
use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use game_hud::art::orb::Shape;
use glam::{Vec2, Vec3};
use kit_base::data::profiles::SizeClass;
use kit_frame::frame::{Draw, Frame, Material, RADIAL, Role};
use kit_scene::hit;
use kit_ui::input::{AssetReady, GlyphCell, Rect, Size};

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

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
                // The border color is the low byte of each of `uv`'s channels; a record of a
                // border whose sides differ (`Draw::panel_sides`, a negative `params[3]`) holds
                // each side's width in quarter points in the high bytes, the top's first.
                #[allow(clippy::cast_possible_truncation)]
                edge: draw.uv.map(|channel| (channel & 0xff) as u8),
                border: if draw.params[3] < 0.0 {
                    f32::from(draw.uv[0] >> 8) / 4.0
                } else {
                    draw.params[3]
                },
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

/// The expected face, rim and text colors of an enabled prompt button.
struct ButtonLook {
    faces: &'static [[u8; 4]],
    rims: &'static [[u8; 4]],
    ink: [u8; 4],
    border: f32,
}

const fn rgba(rgb: u32) -> [u8; 4] {
    let [_, red, green, blue] = rgb.to_be_bytes();
    [red, green, blue, 255]
}

const PRIMARY: ButtonLook = ButtonLook {
    faces: &[
        rgba(0xe2_83_22),
        rgba(0xcc_6f_1c),
        rgba(0xb4_5e_12),
        rgba(0xa7_57_0f),
        rgba(0xad_5b_10),
        rgba(0xc4_6a_18),
        rgba(0xe0_81_22),
        rgba(0xf6_a8_3c),
    ],
    rims: &[rgba(0xff_f2_b1), rgba(0xeb_a0_43), rgba(0xf7_bc_56)],
    ink: rgba(0xff_fa_f2),
    border: 1.5,
};

const SECONDARY: ButtonLook = ButtonLook {
    faces: &[rgba(0x2a_25_22), rgba(0x1d_19_17), rgba(0x26_21_20)],
    rims: &[rgba(0xe9_e3_da), rgba(0xb9_b2_a9), rgba(0xa5_9e_96)],
    ink: rgba(0xff_fd_fa),
    border: 1.5,
};

/// The box of the first button named `name` in the frame, in tree order.
fn first_button(frame: &Frame, name: &str) -> Rect {
    frame
        .resolved
        .iter()
        .find(|element| element.role == Role::Button && frame.name(element.name) == name)
        .map_or_else(|| panic!("no button named \"{name}\""), hit::bounds)
}

/// The index and the stop colors of the gradient drawn on exactly `rect`, if any, without the
/// repeats of its last stop that fill its entry of the frame's gradients.
fn gradient_on(frame: &Frame, rect: Rect) -> Option<(usize, Vec<[u8; 4]>)> {
    let index = (frame.layer as usize..frame.draws.len()).find(|&index| {
        let draw = &frame.draws[index];
        draw.material == Material::Gradient as u8 && same_rect(drawn_rect(frame, draw), rect)
    })?;
    let entry = usize::from(frame.draws[index].layer & (RADIAL - 1));
    let mut colors: Vec<[u8; 4]> = (frame.paints.table.gradients[entry].colors.iter())
        .map(|bytes| bytes.to_le_bytes())
        .collect();
    while colors.len() > 1 && colors[colors.len() - 1] == colors[colors.len() - 2] {
        colors.pop();
    }
    Some((index, colors))
}

/// Checks that the first button named `name` on `page` is drawn as `look` says: a gradient
/// face with the expected gradient stops, or a flat face in one of those colors.
fn assert_button_drawn(page: &mut Page, name: &str, look: &ButtonLook, what: &str) {
    let frame = page.frame();
    let rect = first_button(frame, name);
    let panel = panel_on(frame, rect);
    let mut faults = Vec::new();
    let gradient = gradient_on(frame, rect);
    if let Some((_, colors)) = &gradient {
        if colors.as_slice() != look.faces {
            faults.push(format!(
                "its gradient's stops are {colors:?}, expected {:?}",
                look.faces
            ));
        }
    } else if !look.faces.contains(&panel.tint) {
        faults.push(format!(
            "its face is {:?}, expected one of {:?}",
            panel.tint, look.faces
        ));
    }
    if !look.rims.contains(&panel.edge) {
        faults.push(format!(
            "its rim is {:?}, expected one of {:?}",
            panel.edge, look.rims
        ));
    }
    if (panel.border - look.border).abs() > 0.01 {
        faults.push(format!(
            "its rim is {} points wide, expected {}",
            panel.border, look.border
        ));
    }
    let middle = Vec2::new(rect.x + rect.width / 2.0, rect.y + rect.height / 2.0);
    let (shown, over) = shown_at(frame, middle, &panel);
    if gradient.is_some() {
        // A gradient face shows under its own rim, a clear panel; nothing may cover it.
        let over: Vec<String> = (over.iter())
            .filter(|p| p.tint[3] > 0)
            .map(|p| format!("a panel of {:?} on {:?}", p.tint, p.rect))
            .collect();
        if !over.is_empty() {
            faults.push(format!("its face is under {}", over.join(", ")));
        }
    } else if shown != [panel.tint[0], panel.tint[1], panel.tint[2]] {
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
            "its label is drawn in {tint:?}, expected {:?}",
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
    let mut page = pages::page(name, Side::You, SizeClass::Regular, WINDOW)
        .unwrap_or_else(|why| panic!("{why}"));
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
fn the_play_or_draw_buttons_show_their_enabled_style() {
    let mut page = page_at("opening-play", "prompt:1");
    assert_button_drawn(&mut page, "Play", &PRIMARY, "primary");
    assert_button_drawn(&mut page, "Draw", &SECONDARY, "secondary");
}

/// Both orbs draw their core fill, bezel and gold ring.
#[test]
fn the_life_orbs_draw_a_core_bezel_and_ring() {
    let core = rgba(0x2c_29_26);
    let bezel = 5.0;
    let ring = rgba(0xff_b4_44);
    // The orbs are drawn from the HUD's orb layers, each an icon cell, not as panels, so the
    // colours and widths are read from the layers the cells are rasterized from.
    let layers = &game_hud::art::orb::LAYERS;
    let filled_core = layers
        .iter()
        .find(|layer| layer.shape == Shape::Core && layer.paint.stroke.is_none())
        .expect("a layer fills the core");
    assert_eq!(filled_core.color, core, "the core's fill");
    assert!(
        (game_hud::art::orb::BEZEL - bezel).abs() < 0.01,
        "the bezel is {} points wide, expected {bezel}",
        game_hud::art::orb::BEZEL
    );
    assert!(
        layers.iter().any(|layer| {
            layer.paint.stroke == Some((3.0, true)) && layer.color == ring && !layer.active
        }),
        "no layer draws the {ring:?} stroke on both orbs"
    );
}
