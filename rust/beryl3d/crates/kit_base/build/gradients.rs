//! The module `gradient` of `src/tokens.rs`: each entry of `design-tokens.json`'s
//! `flat.gradient`, a CSS `linear-gradient()` or `radial-gradient()` as the web interface's CSS
//! writes it, as a `Gradient` constant. The gradient is the entry's `value`, or the value of the
//! token of kind `css` its `token` names; a `note` becomes the doc comment.

use std::fmt::Write;

use super::{Tokens, parse_color, top_level_commas};

pub(super) fn write_gradients(tokens: &Tokens) -> String {
    let find = |name: &str| tokens.find(name);
    let mut out = String::new();
    for (key, entry) in tokens.entries("gradient") {
        let css = match (entry["token"].as_str(), entry["value"].as_str()) {
            (Some(token), _) => find(token)["value"].as_str().unwrap(),
            (None, Some(value)) => value,
            (None, None) => panic!("`flat.gradient.{key}` names its `token` or gives its `value`"),
        };
        let css = css.split_whitespace().collect::<Vec<_>>().join(" ");
        let doc = entry["note"]
            .as_str()
            .map_or(format!("`{css}`."), str::to_string);
        let gradient = parse_gradient(&css, &find);
        writeln!(out, "/// {doc}\npub const {key}: Gradient = {gradient};").unwrap();
    }
    out
}

/// A CSS gradient as the Rust expression of a `Gradient`.
fn parse_gradient<'a>(css: &str, find: &dyn Fn(&str) -> &'a serde_json::Value) -> String {
    let (function, inner) = css
        .strip_suffix(')')
        .and_then(|css| css.split_once('('))
        .unwrap_or_else(|| panic!("`{css}` is not a CSS function"));
    let mut parts = top_level_commas(inner);
    let first_is_shape = !is_color(parts[0].split_whitespace().next().unwrap_or(""));
    let shape = if first_is_shape {
        Some(parts.remove(0))
    } else {
        None
    };
    let stops: Vec<String> = parts
        .iter()
        .flat_map(|part| parse_stops(part, find))
        .collect();
    let stops = stops.join(", ");
    match function {
        "linear-gradient" => {
            let angle = shape.map_or(180.0, linear_angle);
            format!("Gradient::linear({angle:?}, &[{stops}])")
        }
        "radial-gradient" => {
            let (circle, extent, at) = radial_shape(shape.unwrap_or(""));
            format!("Gradient::radial({circle}, {extent}, {at}, &[{stops}])")
        }
        _ => panic!("`{css}` is not a linear or radial gradient"),
    }
}

/// Whether `word` starts a color: a hex color, `rgb(`, `transparent` or a `var(`.
fn is_color(word: &str) -> bool {
    word.starts_with('#') || word.starts_with("rgb(") || word == "transparent"
}

/// A linear gradient's angle in degrees from its first argument: `<n>deg` or `to <side>`.
fn linear_angle(text: &str) -> f32 {
    match text {
        "to top" => 0.0,
        "to right" => 90.0,
        "to bottom" => 180.0,
        "to left" => 270.0,
        _ => text
            .strip_suffix("deg")
            .and_then(|degrees| degrees.parse().ok())
            .unwrap_or_else(|| panic!("`{text}` is not an angle the build script reads")),
    }
}

/// A gradient length, `0`, `<n>px` or `<n>%`, as the Rust expression of a `Length`.
fn length(text: &str) -> String {
    if let Some(percent) = text.strip_suffix('%') {
        let percent: f32 = percent.parse().unwrap();
        return format!("Length::Percent({percent:?})");
    }
    let points: f32 = text
        .strip_suffix("px")
        .unwrap_or(text)
        .parse()
        .unwrap_or_else(|_| panic!("`{text}` is not a length the build script reads"));
    format!("Length::Points({points:?})")
}

/// A radial gradient's first argument, `[circle | ellipse] [<extent> | <x> <y>] [at <x> <y>]`,
/// as the Rust expressions of its `circle`, `Extent` and centre.
fn radial_shape(text: &str) -> (bool, String, String) {
    let (size, at) = match text.split_once("at ") {
        Some((size, at)) => (size, at),
        None => (text, "50% 50%"),
    };
    let mut words: Vec<&str> = size.split_whitespace().collect();
    let circle = words.contains(&"circle");
    words.retain(|word| *word != "circle" && *word != "ellipse");
    let extent = match words.as_slice() {
        [] | ["farthest-corner"] => "Extent::FarthestCorner".to_owned(),
        ["closest-side"] => "Extent::ClosestSide".to_owned(),
        ["closest-corner"] => "Extent::ClosestCorner".to_owned(),
        ["farthest-side"] => "Extent::FarthestSide".to_owned(),
        [radius] => format!("Extent::Size({}, {})", length(radius), length(radius)),
        [x, y] => format!("Extent::Size({}, {})", length(x), length(y)),
        _ => panic!("`{text}` is not a radial shape the build script reads"),
    };
    let at: Vec<String> = at.split_whitespace().map(length).collect();
    assert_eq!(at.len(), 2, "`{text}` gives its centre as two lengths");
    (circle, extent, format!("({}, {})", at[0], at[1]))
}

/// One color stop, `<color> [<position> [<position>]]`, as the Rust expressions of its
/// `Stop`s: a stop of two positions is two stops.
fn parse_stops<'a>(text: &str, find: &dyn Fn(&str) -> &'a serde_json::Value) -> Vec<String> {
    let (color, rest) = if let Some(rest) = text.strip_prefix("transparent") {
        ([0; 4], rest)
    } else if let Some(hex) = text.strip_prefix('#') {
        let end = hex.find(' ').unwrap_or(hex.len());
        // A three-digit hex color is each digit twice.
        let digits: String = match end {
            3 => hex[..3].chars().flat_map(|digit| [digit, digit]).collect(),
            _ => hex[..end].to_owned(),
        };
        (parse_color(&format!("#{digits}"), find), &hex[end..])
    } else {
        let end = {
            text.find(')')
                .unwrap_or_else(|| panic!("`{text}` closes its color"))
                + 1
        };
        (parse_color(&text[..end], find), &text[end..])
    };
    let positions: Vec<&str> = rest.split_whitespace().collect();
    if positions.is_empty() {
        return vec![format!("Stop {{ color: {color:?}, at: None }}")];
    }
    positions
        .iter()
        .map(|at| format!("Stop {{ color: {color:?}, at: Some({}) }}", length(at)))
        .collect()
}
