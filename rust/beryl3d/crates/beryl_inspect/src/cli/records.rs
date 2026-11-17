//! `tree --records`: a frame's elements as the element records that `just b3d match` compares
//! with the web interface's (docs/beryl-3d.md, "Matching the web interface's look"). Each
//! record is `{key, role, name, object, box, z, parent, style, text}`, with `style` in CSS's
//! property names and values normalized as the web side's dump script writes them
//! (`tools/src/beryl_tools/beryl3d/match/dump.mjs`): colors as `[r, g, b, a]` with the alpha
//! from 0 to 1, lengths in points, and `"none"` for what Beryl 3D's `Style` cannot say yet
//! (gradients, shadows, filters, masks and transforms).

use beryl_scene::agent::hit::bounds;
use beryl_scene::agent::tree::locator_of;
use beryl_scene::frame::{ElementLook, Frame, Place, REVEALED_FACE, Role};
use serde_json::{Map, Value, json};

/// Rounds `value` to hundredths, as the web side rounds its boxes.
fn round(value: f32) -> Value {
    json!((f64::from(value) * 100.0).round() / 100.0)
}

/// A color as `[r, g, b, a]`, the alpha from 0 to 1 in thousandths.
fn color(rgba: [u8; 4]) -> Value {
    let alpha = (f64::from(rgba[3]) / 255.0 * 1000.0).round() / 1000.0;
    json!([rgba[0], rgba[1], rgba[2], alpha])
}

/// Each element's place in the order the frame paints it: the table's elements farthest first,
/// then the flat layer's in tree order, which is the order its records are drawn in.
fn paint_order(frame: &Frame) -> Vec<usize> {
    let mut table: Vec<usize> = (0..frame.resolved.len())
        .filter(|&at| frame.resolved[at].place != Place::Layer)
        .collect();
    // Nearer the camera is a smaller depth.
    table.sort_by(|&a, &b| frame.resolved[b].depth.total_cmp(&frame.resolved[a].depth));
    let layer = (0..frame.resolved.len()).filter(|&at| frame.resolved[at].place == Place::Layer);
    let mut z = vec![0; frame.resolved.len()];
    for (rank, at) in table.into_iter().chain(layer).enumerate() {
        z[at] = rank;
    }
    z
}

/// The element's style in CSS's names, from how it was drawn (`look`) where the frame recorded
/// that, and its opacity, which includes its ancestors' as the web side's does. No panel is a
/// transparent background, as the web's computed style says.
fn style(opacity: f32, look: Option<&ElementLook>) -> Value {
    let mut style = Map::new();
    let none = || Value::from("none");
    style.insert(
        "background-color".into(),
        look.and_then(|look| look.background)
            .map_or_else(|| color([0; 4]), color),
    );
    style.insert("background-image".into(), none());
    let border = look.and_then(|look| look.border);
    style.insert(
        "border-width".into(),
        round(border.map_or(0.0, |(_, width)| width)),
    );
    style.insert(
        "border-color".into(),
        color(border.map_or([0; 4], |(edge, _)| edge)),
    );
    style.insert(
        "border-radius".into(),
        round(look.map_or(0.0, |look| look.radius)),
    );
    style.insert("box-shadow".into(), none());
    style.insert("filter".into(), none());
    style.insert(
        "opacity".into(),
        json!((f64::from(opacity) * 1000.0).round() / 1000.0),
    );
    if let Some(look) = look {
        style.insert("color".into(), color(look.color));
        style.insert("font-family".into(), look.text.font.family().into());
        style.insert("font-size".into(), round(look.text.size));
        style.insert("font-weight".into(), look.text.weight.into());
    }
    style.insert("mask-image".into(), none());
    style.insert("transform".into(), none());
    Value::Object(style)
}

/// The frame's elements as records. The frame must have been built `looking`
/// (`Page::set_looking`), or no flat-layer element has a look or a line its words.
#[must_use]
pub fn records(frame: &Frame) -> Vec<Value> {
    let z = paint_order(frame);
    let mut looks: Vec<Option<&ElementLook>> = vec![None; frame.resolved.len()];
    for look in &frame.looks {
        if let Some(slot) = looks.get_mut(look.element as usize) {
            *slot = Some(look);
        }
    }
    (frame.resolved.iter().enumerate())
        .map(|(at, element)| {
            let rect = bounds(element);
            let look = looks[at];
            let text: Vec<Value> = (frame.lines.iter())
                .filter(|line| line.element as usize == at)
                .map(|line| {
                    json!({
                        "string": frame.name(line.words).replace('\u{fffc}', ""),
                        "family": look.map(|look| look.text.font.family()),
                        "size": round(line.size),
                        "weight": look.map(|look| look.text.weight),
                        "color": look.map(|look| color(look.color)),
                        "baseline": round(line.baseline),
                        "box": [
                            round(line.rect.x),
                            round(line.rect.y),
                            round(line.rect.width),
                            round(line.rect.height),
                        ],
                    })
                })
                .collect();
            // A card's element id is its object's slot in the view, which the web interface's
            // stage names its card nodes by (`data-stage`).
            let object = (element.role == Role::Card).then_some(element.id.0 % REVEALED_FACE);
            json!({
                "key": locator_of(frame, at as u32),
                "role": element.role.word(),
                "name": frame.name(element.name),
                "object": object,
                "box": [round(rect.x), round(rect.y), round(rect.width), round(rect.height)],
                "z": z[at],
                "parent": (element.parent != u32::MAX).then_some(element.parent),
                "style": style(element.opacity, look),
                "text": text,
            })
        })
        .collect()
}
