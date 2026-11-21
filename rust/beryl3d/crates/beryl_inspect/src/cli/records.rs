//! `tree --records`: a frame's elements as the element records that `just b3d match` compares
//! with the web interface's (docs/beryl-3d.md, "Matching the web interface's look"). Each
//! record is `{key, role, name, object, place, box, z, parent, style, text}`, with `style`
//! in CSS's property names and values normalized as the web side's dump script writes them
//! (`tools/src/beryl_tools/beryl3d/match/dump.mjs`): colors as `[r, g, b, a]` with the alpha
//! from 0 to 1, lengths in points, shadows as `{inset, x, y, blur, spread, color}` (`box-shadow`
//! and each `drop-shadow` of `filter`), gradients as `{kind, repeating, shape, stops}`
//! (`background-image` and `mask-image`), the other filter functions as `{fn, value}`, and
//! `"none"` for a transform, but on a card on the table (`card_transform`). A border is one
//! width and color, or four (top, right, bottom and left) when its sides differ.
//!
//! After the elements come the effects the frame draws (`Looks::effects`), one record each of
//! role `effect`, named by the effect, with what it is on as its `object` (a card's slot, an
//! ability's `a<id>` or a player's side) as the web's dump names its `.stage-effect` nodes
//! (`tools/src/beryl_tools/beryl3d/match/page.mjs`), the box its quad covers on the window and
//! its opacity. `shake`, which moves the whole picture of the table, is on `table`, and its box
//! is the offset of the picture, of no size.
//!
//! After the effects come the marks' outlines (`Frame::rings`), one `mark` record each, named
//! by the mark (`eligible`, `selected`, `suggested`, `friendly-target`, `hostile-target`),
//! parented to the card, ability or life total it rings, with that as its `object` named as an
//! effect's is, as the web's `.stage-mark` nodes are (`data/beryl3d/match/roles.json`). The outline is not one of the
//! frame's elements, so the matcher could not see a missing or wrong one without these.

use beryl_protocol::protocol::Side;
use beryl_protocol::script::Subject;
use beryl_scene::agent::checks::{ring_outer, ring_rect};
use beryl_scene::agent::hit::bounds;
use beryl_scene::agent::tree::{locator_of, place_word};
use beryl_scene::element::{Border, Extent, Filter, Gradient, Length, Shadow, Shape};
use beryl_scene::frame::{
    EffectLook, ElementLook, Frame, LookPaint, LookShadow, Place, REVEALED_FACE, Resolved, Ring,
    Ringed, Role,
};
use beryl_scene::input::Size;
use beryl_scene::table::ABILITY_IDS;
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

/// A shadow as the web side's records write one (`css.py`'s `shadow`).
fn shadow(shadow: &Shadow) -> Value {
    json!({
        "inset": shadow.inset,
        "x": round(shadow.x),
        "y": round(shadow.y),
        "blur": round(shadow.blur),
        "spread": round(shadow.spread),
        "color": color(shadow.color),
    })
}

/// The `box-shadow` and `filter` values of element `at`'s shadows, each `"none"` when it has
/// none of that kind.
fn shadows(at: u32, all: &[LookShadow]) -> (Value, Value) {
    let mine = || all.iter().filter(move |look| look.element == at);
    let boxes: Vec<Value> = mine()
        .filter(|look| !look.drop)
        .map(|look| shadow(&look.shadow))
        .collect();
    let drops: Vec<Value> = mine()
        .filter(|look| look.drop)
        .map(|look| json!({"fn": "drop-shadow", "shadow": shadow(&look.shadow)}))
        .collect();
    let value = |list: Vec<Value>| {
        if list.is_empty() {
            Value::from("none")
        } else {
            Value::Array(list)
        }
    };
    (value(boxes), value(drops))
}

/// A number as CSS writes it: no decimals when it is whole.
fn css_number(value: f32) -> String {
    let rounded = (f64::from(value) * 100.0).round() / 100.0;
    if rounded.fract() == 0.0 {
        format!("{rounded:.0}")
    } else {
        format!("{rounded}")
    }
}

/// A gradient length as `WebKit` computes one: `40px` or `75%`.
fn css_length(length: Length) -> String {
    match length {
        Length::Points(points) => format!("{}px", css_number(points)),
        Length::Percent(percent) => format!("{}%", css_number(percent)),
    }
}

/// A stop's position as the web side's records write one: a percentage as text, a length as
/// a number of points.
fn stop_position(length: Length) -> Value {
    match length {
        Length::Points(points) => round(points),
        Length::Percent(_) => Value::from(css_length(length)),
    }
}

/// A radial gradient's first argument as `WebKit` serializes it, or null when it has none:
/// `circle` for a circle (an ellipse is the default), its extent unless `farthest-corner`, and
/// its centre unless the middle.
fn radial_shape(circle: bool, extent: Extent, at: (Length, Length)) -> Value {
    let mut words = Vec::new();
    if circle {
        words.push("circle".to_owned());
    }
    match extent {
        Extent::FarthestCorner => {}
        Extent::ClosestSide => words.push("closest-side".to_owned()),
        Extent::ClosestCorner => words.push("closest-corner".to_owned()),
        Extent::FarthestSide => words.push("farthest-side".to_owned()),
        Extent::Size(x, _) if circle => words.push(css_length(x)),
        Extent::Size(x, y) => words.push(format!("{} {}", css_length(x), css_length(y))),
    }
    if at != (Length::Percent(50.0), Length::Percent(50.0)) {
        words.push(format!("at {} {}", css_length(at.0), css_length(at.1)));
    }
    if words.is_empty() {
        Value::Null
    } else {
        Value::from(words.join(" "))
    }
}

/// A gradient as the web side's records write one (`css.py`'s `image`).
fn gradient(gradient: &Gradient) -> Value {
    let (kind, shape) = match gradient.shape {
        Shape::Linear { angle } => ("linear", round(angle.rem_euclid(360.0))),
        Shape::Radial { circle, extent, at } => ("radial", radial_shape(circle, extent, at)),
    };
    let stops: Vec<Value> = (gradient.stops().iter())
        .map(|stop| {
            json!({
                "color": color(stop.color),
                "at": stop.at.map(|at| vec![stop_position(at)]),
            })
        })
        .collect();
    json!({"kind": kind, "repeating": false, "shape": shape, "stops": stops})
}

/// A filter function other than a drop shadow as the web side's records write one.
fn filter_function(filter: Filter) -> Value {
    let (name, value) = match filter {
        Filter::Brightness(amount) => ("brightness", amount),
        Filter::Grayscale(amount) => ("grayscale", amount),
        Filter::Saturate(amount) => ("saturate", amount),
        Filter::Blur(points) => ("blur", points),
    };
    json!({"fn": name, "value": (f64::from(value) * 10_000.0).round() / 10_000.0})
}

/// The `background-image`, `mask-image` and the filter functions other than drop shadows of
/// element `at`, each image `"none"` when it has none.
fn paints(at: u32, all: &[LookPaint]) -> (Value, Value, Vec<Value>) {
    let (mut images, mut mask, mut filters) = (Vec::new(), Value::from("none"), Vec::new());
    for paint in all.iter().filter(|paint| paint.element() == at) {
        match paint {
            LookPaint::Gradient(_, layer) => images.push(gradient(layer)),
            LookPaint::Mask(_, layer) => mask = Value::Array(vec![gradient(layer)]),
            LookPaint::Filter(_, filter) => filters.push(filter_function(*filter)),
        }
    }
    let images = if images.is_empty() {
        Value::from("none")
    } else {
        Value::Array(images)
    };
    (images, mask, filters)
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
fn style(
    (opacity, transform): (f32, Option<Value>),
    look: Option<&ElementLook>,
    shadows: (Value, Value),
    (images, mask, filters): (Value, Value, Vec<Value>),
) -> Value {
    let mut style = Map::new();
    let none = || Value::from("none");
    style.insert(
        "background-color".into(),
        look.and_then(|look| look.background)
            .map_or_else(|| color([0; 4]), color),
    );
    style.insert("background-image".into(), images);
    let border = look
        .and_then(|look| look.border)
        .unwrap_or(Border::uniform([0; 4], 0.0));
    style.insert("border-width".into(), sides(&border.width.map(round)));
    style.insert("border-color".into(), sides(&border.color.map(color)));
    style.insert(
        "border-radius".into(),
        round(look.map_or(0.0, |look| look.radius)),
    );
    let (box_shadow, drops) = shadows;
    style.insert("box-shadow".into(), box_shadow);
    let mut filter = match drops {
        Value::Array(drops) => drops,
        _ => Vec::new(),
    };
    filter.extend(filters);
    style.insert(
        "filter".into(),
        if filter.is_empty() {
            none()
        } else {
            Value::Array(filter)
        },
    );
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
    style.insert("mask-image".into(), mask);
    style.insert("transform".into(), transform.unwrap_or_else(none));
    Value::Object(style)
}

/// Four sides' values, top, right, bottom and left, as one value when they are equal, as the web
/// side's records write them.
fn sides(values: &[Value; 4]) -> Value {
    if values.iter().all(|value| *value == values[0]) {
        values[0].clone()
    } else {
        Value::Array(values.to_vec())
    }
}

/// The `transform` of a card on the table, as a 2D `matrix()`'s six numbers: the rotation of the
/// top edge of its face on screen, with no scale, moved to the face's top left corner. The web
/// lays its cards out flat and turns them with a CSS transform, where Beryl 3D poses each card in
/// 3D and projects it, so its scale and skew have no CSS counterpart; `just b3d match styles`
/// compares a transform by its rotation alone and leaves the position and size to the boxes.
fn card_transform(element: &Resolved) -> Value {
    // `card_corners` order: bottom left, bottom right, top right and top left of the face.
    let [_, _, top_right, top_left] = element.corners;
    let along = top_right - top_left;
    let angle = along.y.atan2(along.x);
    let (sin, cos) = angle.sin_cos();
    let fine = |value: f32| json!((f64::from(value) * 10_000.0).round() / 10_000.0);
    json!([
        fine(cos),
        fine(sin),
        fine(-sin),
        fine(cos),
        round(top_left.x),
        round(top_left.y)
    ])
}

/// What an effect is on, as the web's dump names it: a card's slot, `a<id>` for an ability, a
/// player's side, or `table` for a shake.
fn subject(effect: &EffectLook) -> Value {
    let Some(on) = effect.on else {
        return Value::from("table");
    };
    match on {
        Subject::Object { slot } => json!(slot.0),
        Subject::Ability { id } => Value::from(format!("a{id}")),
        Subject::Player { player: Side::You } => Value::from("you"),
        Subject::Player {
            player: Side::Opponent,
        } => Value::from("opponent"),
    }
}

/// The box on a window of `size` of the quad from -0.5 to 0.5 that `model` places in the
/// table's space, through the frame's camera, as `[x, y, width, height]` in logical points. A
/// shake's model is only its offset, which is its box's corner.
fn effect_box(frame: &Frame, effect: &EffectLook, size: Size) -> Vec<Value> {
    let m = effect.model;
    if effect.on.is_none() {
        return vec![round(m[3]), round(m[7]), round(0.0), round(0.0)];
    }
    // The camera's matrix by columns, applied by hand: this crate takes `glam` only for its
    // tests.
    let c = frame.view_projection.to_cols_array();
    let (mut low, mut high) = ([f32::MAX; 2], [f32::MIN; 2]);
    for (x, y) in [(-0.5, -0.5), (0.5, -0.5), (0.5, 0.5), (-0.5, 0.5)] {
        let point = [
            m[0] * x + m[1] * y + m[3],
            m[4] * x + m[5] * y + m[7],
            m[8] * x + m[9] * y + m[11],
        ];
        let clip = |row: usize| {
            c[row] * point[0] + c[4 + row] * point[1] + c[8 + row] * point[2] + c[12 + row]
        };
        let w = clip(3);
        let screen = [
            f32::midpoint(clip(0) / w, 1.0) * size.width,
            f32::midpoint(1.0, -clip(1) / w) * size.height,
        ];
        for axis in 0..2 {
            low[axis] = low[axis].min(screen[axis]);
            high[axis] = high[axis].max(screen[axis]);
        }
    }
    vec![
        round(low[0]),
        round(low[1]),
        round(high[0] - low[0]),
        round(high[1] - low[1]),
    ]
}

/// The records of the effects the frame draws, painted after its `elements` elements.
fn effect_records(frame: &Frame, size: Size, elements: usize) -> Vec<Value> {
    (frame.looks.effects.iter().enumerate())
        .map(|(at, effect)| {
            let on = subject(effect);
            let name = effect.name;
            json!({
                "key": format!("effect \"{name}\" on {}", on.as_str().map_or_else(|| on.to_string(), str::to_owned)),
                "role": "effect",
                "name": name,
                "object": on,
                "place": "",
                "box": effect_box(frame, effect, size),
                "z": elements + at,
                "parent": null,
                // The style of an element that paints nothing of its own, seen at the effect's
                // opacity, so that the record keeps the contract.
                "style": style(
                    (effect.opacity.clamp(0.0, 1.0), None),
                    None,
                    (Value::from("none"), Value::from("none")),
                    (Value::from("none"), Value::from("none"), Vec::new()),
                ),
                "text": [],
            })
        })
        .collect()
}

/// The record of `ring`, the mark outline at `z` in paint order, after every element and
/// effect: its box is the line the outline is drawn along, and its look the box shadows it is
/// drawn with in the mark's color (or one whose blur reaches as far as a sprite outline's glow),
/// with the opacity the mark has as it comes and goes. Its parent and object are those of the
/// card, ability or life total it rings.
fn mark_record(frame: &Frame, ring: &Ring, size: Size, z: usize) -> Value {
    let line = ring_rect(ring, frame.view_projection, size);
    let outer = ring_outer(ring, frame.view_projection, size);
    let parent = match ring.on {
        Ringed::Element(id) => frame
            .resolved
            .iter()
            .position(|element| element.id == id && element.place != Place::Layer),
        Ringed::Player(player) => {
            let name = beryl_scene::hud::players::orb_name(player);
            (frame.resolved.iter()).position(|element| {
                element.place == Place::Layer && frame.name(element.name) == name
            })
        }
    };
    // What it rings, as an effect's `subject` names it: a card's slot, `a<id>` for an ability
    // (whose element id is `ABILITY_IDS` more than its id), or a player's side.
    let object = match ring.on {
        Ringed::Element(id) if id.0 >= ABILITY_IDS => {
            Value::from(format!("a{}", id.0 - ABILITY_IDS))
        }
        Ringed::Element(id) => json!(id.0 % REVEALED_FACE),
        Ringed::Player(Side::You) => Value::from("you"),
        Ringed::Player(Side::Opponent) => Value::from("opponent"),
    };
    let [red, green, blue, alpha] = ring.tint;
    // An outline drawn with the stylesheet's box shadows is recorded with them, their lengths
    // in points of the window by the box's width on screen; one drawn as a sprite as a single
    // glow as far as the sprite reaches.
    let points = if ring.frames > 0.0 {
        line.width / ring.frames
    } else {
        0.0
    };
    let layers: Vec<Value> = if ring.shadows.is_empty() {
        vec![shadow(&Shadow {
            inset: false,
            x: 0.0,
            y: 0.0,
            blur: ((outer.width - line.width) / 2.0).max(0.0),
            spread: 0.0,
            color: [red, green, blue, 255],
        })]
    } else {
        (ring.shadows.iter())
            .map(|layer| {
                shadow(&Shadow {
                    blur: layer.blur * points,
                    spread: layer.spread * points,
                    color: [red, green, blue, layer.color[3]],
                    ..*layer
                })
            })
            .collect()
    };
    let shadows = (Value::Array(layers), Value::from("none"));
    let images = (Value::from("none"), Value::from("none"), Vec::new());
    let mut mark_style = style((f32::from(alpha) / 255.0, None), None, shadows, images);
    mark_style["border-radius"] = round(ring.radius * points);
    json!({
        "key": Value::Null,
        "role": "mark",
        "name": ring.mark.name(),
        "object": object,
        "place": parent.map_or_else(String::new, |at| place_word(frame.resolved[at].place)),
        "box": [round(line.x), round(line.y), round(line.width), round(line.height)],
        "z": z,
        "parent": parent,
        "style": mark_style,
        "text": [],
    })
}

/// The frame's elements as records, then its effects' and its marks' outlines', for a window
/// of `size`. The frame must have been built `looking` (`Page::set_looking`), or no flat-layer
/// element has a look or a line its words, and no effect is recorded.
#[must_use]
pub fn records(frame: &Frame, size: Size) -> Vec<Value> {
    let z = paint_order(frame);
    let mut looks: Vec<Option<&ElementLook>> = vec![None; frame.resolved.len()];
    for look in &frame.looks.elements {
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
            // stage names its card nodes by (`data-stage`). An ability's is `ABILITY_IDS` past
            // its number, and the stage names its node `a` and the number (`a1`).
            let object = match element.role {
                Role::Card => Some(Value::from(element.id.0 % REVEALED_FACE)),
                Role::Ability => Some(Value::from(format!("a{}", element.id.0 - ABILITY_IDS))),
                _ => None,
            };
            let transform = (element.role == Role::Card && element.place != Place::Layer)
                .then(|| card_transform(element));
            json!({
                "key": locator_of(frame, at as u32),
                "role": element.role.word(),
                "name": frame.name(element.name),
                "object": object,
                // Where on the table it lies, as the tree writes it (`battlefield.yours.lands`,
                // `stack`), or empty for the flat layer. `just b3d match census` reads it to
                // tell a card in hand from one on the battlefield or the stack.
                "place": place_word(element.place),
                "box": [round(rect.x), round(rect.y), round(rect.width), round(rect.height)],
                "z": z[at],
                "parent": (element.parent != u32::MAX).then_some(element.parent),
                "style": style(
                    (element.opacity, transform),
                    look,
                    shadows(at as u32, &frame.looks.shadows),
                    paints(at as u32, &frame.looks.paints),
                ),
                "text": text,
            })
        })
        .chain(effect_records(frame, size, frame.resolved.len()))
        .chain((frame.rings.iter().enumerate()).map(|(at, ring)| {
            let z = frame.resolved.len() + frame.looks.effects.len() + at;
            mark_record(frame, ring, size, z)
        }))
        .collect()
}
