//! `tree --records`: a frame's elements as the element records that `just b3d match` compares
//! with the web interface's (docs/beryl-3d.md, "Matching the web interface's look"). Each
//! record is `{key, role, name, object, place, box, z, parent, style, text}`, with `style`
//! in CSS's property names and values normalized as the web side's dump script writes them
//! (`tools/src/beryl_tools/beryl3d/match/dump.mjs`): colors as `[r, g, b, a]` with the alpha
//! from 0 to 1, lengths in points, shadows as `{inset, x, y, blur, spread, color}` (`box-shadow`
//! and each `drop-shadow` of `filter`), gradients as `{kind, repeating, shape, stops}`
//! (`background-image` and `mask-image`), the other filter functions as `{fn, value}`, and
//! `"none"` for what Beryl 3D's `Style` cannot say yet (transforms).
//!
//! After the elements come the effects the frame draws (`Looks::effects`), one record each of
//! role `effect`, named by the effect, with what it is on as its `object` (a card's slot, an
//! ability's `a<id>` or a player's side) as the web's dump names its `.stage-effect` nodes
//! (`tools/src/beryl_tools/beryl3d/match/page.mjs`), the box its quad covers on the window and
//! its opacity. `shake`, which moves the whole picture of the table, is on `table`, and its box
//! is the offset of the picture, of no size.

use beryl_protocol::protocol::Side;
use beryl_protocol::script::Subject;
use beryl_scene::agent::hit::bounds;
use beryl_scene::agent::tree::{locator_of, place_word};
use beryl_scene::effects::Effect;
use beryl_scene::element::{Extent, Filter, Gradient, Length, Shadow, Shape};
use beryl_scene::frame::{
    EffectLook, ElementLook, Frame, LookPaint, LookShadow, Place, REVEALED_FACE, Role,
};
use beryl_scene::input::Size;
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
    opacity: f32,
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
    style.insert("transform".into(), none());
    Value::Object(style)
}

/// What an effect is on, as the web's dump names it: a card's slot, `a<id>` for an ability, a
/// player's side, or `table` for a shake.
fn subject(effect: &EffectLook) -> Value {
    if effect.effect == Effect::Shake {
        return Value::from("table");
    }
    match effect.on {
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
    if effect.effect == Effect::Shake {
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
            let name = effect.effect.name();
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
                    effect.opacity.clamp(0.0, 1.0),
                    None,
                    (Value::from("none"), Value::from("none")),
                    (Value::from("none"), Value::from("none"), Vec::new()),
                ),
                "text": [],
            })
        })
        .collect()
}

/// The frame's elements as records, then its effects' for a window of `size`. The frame must
/// have been built `looking` (`Page::set_looking`), or no flat-layer element has a look or a
/// line its words, and no effect is recorded.
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
            // stage names its card nodes by (`data-stage`).
            let object = (element.role == Role::Card).then_some(element.id.0 % REVEALED_FACE);
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
                    element.opacity,
                    look,
                    shadows(at as u32, &frame.looks.shadows),
                    paints(at as u32, &frame.looks.paints),
                ),
                "text": text,
            })
        })
        .chain(effect_records(frame, size, frame.resolved.len()))
        .collect()
}
