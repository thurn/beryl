//! The texture for one ability, drawn over its source card.

use kit_base::atlas::FaceSize;
use kit_text::text::fonts::Font;
use tiny_skia::{FillRule, Mask, Path, PathBuilder, Shader, Transform};

use super::INK;
use super::layout::{Box2, Geometry, TextStyle, Typesetter};
use super::paint::{Canvas, color, stops, vertical};
use crate::faces::Plate;

/// The plate over the lower half of a face that says one ability, as `AbilityPlate` in
/// `Card.tsx` draws it: parchment in a dark rim, its label on a tab across its top, and the
/// ability's words in its middle, shrunk to fit.
pub(super) fn draw_plate(
    canvas: &mut Canvas,
    text: &mut Typesetter,
    plate: &Plate,
    size: FaceSize,
    scan: bool,
) {
    let g = Geometry::new(size);
    let card = g.card;
    let em = if plate.choice {
        card.width / 12.0
    } else {
        g.em
    };
    let curved = scan && plate.choice;
    let area = Box2 {
        x: card.x + 0.05 * card.width,
        y: card.y + 0.56 * card.height,
        width: 0.9 * card.width,
        height: (1.0 - 0.56 - if curved { 0.02 } else { 0.045 }) * card.height,
    };
    let radius = 0.35 * em;
    let border = Shader::SolidColor(color([0x1b, 0x17, 0x12], 1.0));
    let paper = area.inset(0.08 * em, 0.08 * em, 0.08 * em, 0.08 * em);
    let fill = vertical(
        paper,
        stops(&[
            (0.0, [0xf8, 0xf2, 0xe2], 1.0),
            (1.0, [0xe6, 0xdc, 0xc4], 1.0),
        ]),
    );
    if curved {
        // The scan ends where the plate begins, inside the curved foot of the web's
        // `.stack-item-scanned`. Its printed rules box and collector line are hidden.
        clip_choice(canvas, card, area.y);
        canvas.fill(outline(area, [0.0; 2], area.height * 0.36).as_ref(), border);
        canvas.fill(
            outline(paper, [0.0; 2], paper.height * 0.36).as_ref(),
            Shader::SolidColor(color([0xc9, 0xb9, 0x98], 1.0)),
        );
        let inside = paper.inset(0.1 * em, 0.1 * em, 0.1 * em, 0.1 * em);
        canvas.fill(
            outline(inside, [0.0; 2], inside.height * 0.36).as_ref(),
            fill,
        );
    } else {
        canvas.fill_box(area, radius, border);
        canvas.fill_box(paper, radius - 0.08 * em, fill);
        canvas.edge_inside(
            paper,
            radius - 0.08 * em,
            0.1 * em,
            color([150, 120, 70], 0.45),
        );
    }
    let top = if curved { paper.y } else { area.y };
    let below_tab = draw_tab(canvas, text, area, (top, em), plate.triggered, curved);
    // On a scanned choice, leave more room at the bottom for the curved plate.
    let size = 0.74 * em;
    let rules_area = if curved { paper } else { area };
    let room = Box2 {
        y: below_tab,
        height: rules_area.bottom() - below_tab,
        ..rules_area
    }
    .inset(
        0.3 * size,
        if curved { 0.9 } else { 0.55 } * size,
        if curved { 1.7 } else { 0.3 } * size,
        if curved { 0.9 } else { 0.55 } * size,
    );
    let style = TextStyle {
        font: Font::CrimsonPro,
        weight: 560.0,
        size,
        line_height: 1.22,
        color: INK,
    };
    let set = text.set_block_to_fit(&plate.rules, style, (room.width, room.height), plate.choice);
    let top = room.y + ((room.height - set.height) / 2.0).max(0.0);
    canvas.text(&set, (room.x, top));
}

/// Draws the uppercase label on its gold-edged tab and returns the bottom edge.
fn draw_tab(
    canvas: &mut Canvas,
    text: &mut Typesetter,
    area: Box2,
    (top, em): (f32, f32),
    triggered: bool,
    curved: bool,
) -> f32 {
    let label_em = 0.62 * em;
    let label = if triggered {
        "TRIGGERED ABILITY"
    } else {
        "ACTIVATED ABILITY"
    };
    let label = text.set_tab(
        label,
        TextStyle {
            font: Font::CrimsonPro,
            weight: 650.0,
            size: label_em,
            line_height: 1.1,
            color: [0xf2, 0xea, 0xd8],
        },
        if curved { 0.04 * label_em } else { 0.0 },
    );
    let tab = Box2 {
        x: area.x + (area.width - label.width) / 2.0 - 0.6 * label_em,
        y: top + 0.45 * label_em,
        width: label.width + 1.2 * label_em,
        height: label.height + if curved { 0.36 } else { 0.2 } * label_em,
    };
    canvas.fill_box(
        tab,
        tab.height / 2.0,
        Shader::SolidColor(color([0xc9, 0xa3, 0x5c], 1.0)),
    );
    let inner = tab.inset(
        0.08 * label_em,
        0.08 * label_em,
        0.08 * label_em,
        0.08 * label_em,
    );
    let dark = vertical(
        inner,
        stops(&[
            (0.0, [0x3a, 0x36, 0x31], 1.0),
            (1.0, [0x1c, 0x1a, 0x17], 1.0),
        ]),
    );
    canvas.fill_box(inner, inner.height / 2.0, dark);
    canvas.text(
        &label,
        (
            tab.x + 0.6 * label_em,
            tab.y + if curved { 0.16 } else { 0.1 } * label_em,
        ),
    );
    tab.bottom()
}

fn clip_choice(canvas: &mut Canvas, card: Box2, fold: f32) {
    canvas.fill_box(
        Box2 {
            y: fold,
            height: card.bottom() - fold,
            ..card
        },
        0.0,
        Shader::SolidColor(color([0x0b, 0x0a, 0x09], 1.0)),
    );
    let Some(edge) = outline(
        card,
        [0.0475 * card.width, 0.035 * card.height],
        0.18 * card.height,
    ) else {
        return;
    };
    let Some(mut mask) = Mask::new(canvas.pixmap.width(), canvas.pixmap.height()) else {
        return;
    };
    mask.fill_path(&edge, FillRule::Winding, true, Transform::identity());
    canvas.pixmap.apply_mask(&mask);
}

/// The plate and the scan behind it have elliptical bottom corners that meet in the middle.
fn outline(area: Box2, [top_x, top_y]: [f32; 2], bottom_y: f32) -> Option<Path> {
    // A cubic's control point along the tangent to one quarter of an ellipse.
    const QUARTER: f32 = 0.552_284_8;
    let (left, top, right, bottom) = (area.x, area.y, area.right(), area.bottom());
    let mid = f32::midpoint(left, right);
    let lower = bottom - bottom_y;
    let across = area.width * 0.5 * QUARTER;
    let down = bottom_y * QUARTER;
    let mut edge = PathBuilder::new();
    edge.move_to(left + top_x, top);
    edge.line_to(right - top_x, top);
    edge.cubic_to(
        right - top_x * (1.0 - QUARTER),
        top,
        right,
        top + top_y * (1.0 - QUARTER),
        right,
        top + top_y,
    );
    edge.line_to(right, lower);
    edge.cubic_to(right, lower + down, mid + across, bottom, mid, bottom);
    edge.cubic_to(mid - across, bottom, left, lower + down, left, lower);
    edge.line_to(left, top + top_y);
    edge.cubic_to(
        left,
        top + top_y * (1.0 - QUARTER),
        left + top_x * (1.0 - QUARTER),
        top,
        left + top_x,
        top,
    );
    edge.close();
    edge.finish()
}
