//! Generates spatial constants from the shared tuning and layout profiles.
//! It also generates the design tokens and embeds the serif font files.

#[path = "build/gradients.rs"]
mod gradients;

#[path = "build/profiles.rs"]
mod profiles;

use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let data = manifest.join("../../../../data/beryl3d");
    let space = data.join("space-constants.json");
    println!("cargo:rerun-if-changed={}", space.display());
    let space_text = fs::read_to_string(&space).unwrap();
    let mut space_json: serde_json::Value = serde_json::from_str(&space_text).unwrap();
    profiles::legacy(&data, &mut space_json);
    let mut out = String::new();
    write_space(&space_json, &mut out);
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    fs::write(out_dir.join("data.rs"), out).unwrap();
    write_serif(&out_dir);
    let tokens = manifest.join("../../../../data/design-tokens.json");
    println!("cargo:rerun-if-changed={}", tokens.display());
    let tokens_json: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&tokens).unwrap()).unwrap();
    let tokens_dir = out_dir.join("tokens");
    fs::create_dir_all(&tokens_dir).unwrap();
    for (module, text) in write_tokens(&tokens_json) {
        fs::write(tokens_dir.join(format!("{module}.rs")), text).unwrap();
    }
}

/// The folder of the fonts every Mac ships beside the system's own, which holds Times New Roman.
const MAC_SUPPLEMENTAL_FONTS: &str = "/System/Library/Fonts/Supplemental";

/// Writes the files that draw the web's serif face, `--font-serif` (`"Times New Roman", Times,
/// serif`), as the constants `src/typography/fonts.rs` includes. When the build is for macOS and the
/// building Mac has Times New Roman's regular, bold, italic and bold italic files, they are
/// embedded from the system's font folder, so the client draws the face the web's CSS names.
/// Elsewhere, such as on Windows, every face is the bundled Crimson Pro, the nearest serif the
/// repository holds, which has no italic: its italics are drawn upright.
fn write_serif(out_dir: &std::path::Path) {
    let folder = std::path::Path::new(MAC_SUPPLEMENTAL_FONTS);
    let faces = [
        (
            "REGULAR",
            "SERIF_REGULAR",
            folder.join("Times New Roman.ttf"),
        ),
        (
            "BOLD",
            "SERIF_BOLD_DATA",
            folder.join("Times New Roman Bold.ttf"),
        ),
        (
            "ITALIC",
            "SERIF_ITALIC_DATA",
            folder.join("Times New Roman Italic.ttf"),
        ),
        (
            "BOLD_ITALIC",
            "SERIF_BOLD_ITALIC_DATA",
            folder.join("Times New Roman Bold Italic.ttf"),
        ),
    ];
    for (_, _, path) in &faces {
        println!("cargo:rerun-if-changed={}", path.display());
    }
    let for_mac = std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "macos");
    let found = for_mac && faces.iter().all(|(_, _, path)| path.is_file());
    let mut text = String::from("/// The family the serif face is drawn in.\n");
    if found {
        text.push_str(
            "pub const SERIF_FAMILY: &str = \"Times New Roman\";\n\
             /// Whether the serif face has italic files of its own.\n\
             pub const SERIF_HAS_ITALIC: bool = true;\n",
        );
    } else {
        text.push_str(
            "pub const SERIF_FAMILY: &str = \"Crimson Pro\";\n\
             /// Whether the serif face has italic files of its own.\n\
             pub const SERIF_HAS_ITALIC: bool = false;\n",
        );
    }
    for (name, data, path) in &faces {
        if found {
            let path = path.display().to_string();
            writeln!(
                text,
                "const SERIF_{name}_FILE: &str = {path:?};\nstatic {data}: &[u8] = include_bytes!({path:?});"
            )
            .unwrap();
        } else {
            writeln!(
                text,
                "const SERIF_{name}_FILE: &str = \"CrimsonPro.ttf\";\nstatic {data}: &[u8] = CRIMSON_PRO;"
            )
            .unwrap();
        }
    }
    fs::write(out_dir.join("serif.rs"), text).unwrap();
}

/// The modules of `src/tokens.rs` from `design-tokens.json`, each as its name and its
/// constants. `color` holds a constant for every token of kind `color`, named for the token in
/// upper case with `_` for `-`, then the colors of `flat.color`. `space`, `type_style` and
/// `size` and `ratio` hold the entries of `flat` under the same names, `mana` the colors of
/// `flat.mana`, `shadow` the shadow lists of `flat.shadow`, and `gradient` the gradients of
/// `flat.gradient` (`build/gradients.rs`). A number in `flat` is a `value`, or the `token` of kind `length` whose pixels it
/// takes; a `note` becomes the doc comment.
fn write_tokens(json: &serde_json::Value) -> Vec<(&'static str, String)> {
    let tokens = Tokens {
        list: json["tokens"].as_array().expect("`tokens` is an array"),
        flat: &json["flat"],
    };
    let mut space = String::new();
    let mut size = String::new();
    let mut ratio = String::new();
    for (module, out) in [
        ("space", &mut space),
        ("size", &mut size),
        ("ratio", &mut ratio),
    ] {
        for (key, entry) in tokens.entries(module) {
            let (value, doc) = tokens.number(entry, &format!("flat.{module}.{key}"));
            writeln!(out, "/// {doc}\npub const {key}: f32 = {value:?};").unwrap();
        }
    }
    vec![
        ("space", space),
        ("type_style", write_type_styles(&tokens)),
        ("size", size),
        ("ratio", ratio),
        ("color", write_colors(&tokens)),
        ("mana", write_mana(&tokens)),
        ("shadow", write_shadows(&tokens)),
        ("gradient", gradients::write_gradients(&tokens)),
    ]
}

/// The module `shadow`: each entry of `flat.shadow`, a list of CSS shadows as `box-shadow` or
/// a chain of `drop-shadow()` filters gives them, as an array of `Shadow`. The list is the
/// entry's `value`, or the value of the token of kind `css` its `token` names.
fn write_shadows(tokens: &Tokens) -> String {
    let find = |name: &str| tokens.find(name);
    let mut out = String::new();
    for (key, entry) in tokens.entries("shadow") {
        let css = match (entry["token"].as_str(), entry["value"].as_str()) {
            (Some(token), _) => find(token)["value"].as_str().unwrap(),
            (None, Some(value)) => value,
            (None, None) => panic!("`flat.shadow.{key}` names its `token` or gives its `value`"),
        };
        let shadows: Vec<String> = top_level_commas(css)
            .into_iter()
            .map(|part| parse_shadow(part, &find))
            .collect();
        let doc = entry["note"]
            .as_str()
            .map_or(format!("`{css}`."), str::to_string);
        writeln!(
            out,
            "/// {doc}\npub const {key}: [Shadow; {}] = [{}];",
            shadows.len(),
            shadows.join(", ")
        )
        .unwrap();
    }
    out
}

/// `text` split at the commas outside parentheses.
fn top_level_commas(text: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0_i32, 0);
    for (at, character) in text.char_indices() {
        match character {
            '(' => depth += 1,
            ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(text[start..at].trim());
                start = at + 1;
            }
            _ => {}
        }
    }
    parts.push(text[start..].trim());
    parts
}

/// One CSS shadow, `[inset] <x> <y> [<blur> [<spread>]] <color>`, with lengths of `0` or in
/// `px`, as the Rust expression of a `Shadow`.
fn parse_shadow<'a>(text: &str, find: &dyn Fn(&str) -> &'a serde_json::Value) -> String {
    let (color, rest) = match text.find("rgb(").or_else(|| text.find('#')) {
        Some(at) => {
            let end = if text[at..].starts_with('#') {
                at + 7
            } else {
                at + text[at..].find(')').expect("a closed `rgb(`") + 1
            };
            (
                parse_color(&text[at..end], find),
                format!("{} {}", &text[..at], &text[end..]),
            )
        }
        None => panic!("the shadow `{text}` names its color"),
    };
    let inset = rest.split_whitespace().any(|word| word == "inset");
    let lengths: Vec<f32> = (rest.split_whitespace())
        .filter(|word| *word != "inset")
        .map(|word| {
            word.strip_suffix("px")
                .unwrap_or(word)
                .parse()
                .unwrap_or_else(|_| panic!("`{word}` in the shadow `{text}` is not a length"))
        })
        .collect();
    let length = |at: usize| lengths.get(at).copied().unwrap_or(0.0);
    format!(
        "Shadow {{ x: {:?}, y: {:?}, blur: {:?}, spread: {:?}, color: {color:?}, inset: {inset} }}",
        length(0),
        length(1),
        length(2),
        length(3)
    )
}

/// The module `mana`: each entry of `flat.mana`, a mana symbol's disc or ink as the hex color
/// in its `value`.
fn write_mana(tokens: &Tokens) -> String {
    let mut out = String::new();
    for (key, entry) in tokens.entries("mana") {
        let value = entry["value"]
            .as_str()
            .unwrap_or_else(|| panic!("`flat.mana.{key}` has a color `value`"));
        let rgba = parse_color(value, &|name| tokens.find(name));
        let doc = entry["note"].as_str().unwrap_or("A mana symbol's color.");
        writeln!(out, "/// {doc}\npub const {key}: [u8; 4] = {rgba:?};").unwrap();
    }
    out
}

/// The tokens of `design-tokens.json` and its `flat` object.
struct Tokens<'a> {
    list: &'a [serde_json::Value],
    flat: &'a serde_json::Value,
}

impl<'a> Tokens<'a> {
    /// The token named `name`.
    fn find(&self, name: &str) -> &'a serde_json::Value {
        self.list
            .iter()
            .find(|token| token["name"] == name)
            .unwrap_or_else(|| panic!("design-tokens.json has no token `{name}`"))
    }

    /// The entries of `flat.<module>`.
    fn entries(&self, module: &str) -> &'a serde_json::Map<String, serde_json::Value> {
        self.flat[module]
            .as_object()
            .unwrap_or_else(|| panic!("design-tokens.json's `flat.{module}` is an object"))
    }

    /// A number of `flat` at `path` and its doc comment: a plain number, or an object with its
    /// `value` or the `token` whose length in pixels it takes, and an optional `note`.
    fn number(&self, entry: &serde_json::Value, path: &str) -> (f32, String) {
        let doc = entry["note"].as_str().map(str::to_string);
        if let Some(name) = entry["token"].as_str() {
            let value = self.find(name)["value"].as_str().unwrap_or_default();
            let pixels = value
                .strip_suffix("px")
                .and_then(|n| n.parse::<f32>().ok())
                .unwrap_or_else(|| panic!("`{path}` names `{name}`, which is not a length"));
            let doc = doc.map_or(format!("`--{name}`."), |d| format!("`--{name}`: {d}"));
            return (pixels, doc);
        }
        #[allow(clippy::cast_possible_truncation)]
        let value = entry
            .as_f64()
            .or_else(|| entry["value"].as_f64())
            .unwrap_or_else(|| panic!("`{path}` is a number, or has a `value` or a `token`"))
            as f32;
        (value, doc.unwrap_or_else(|| format!("`{path}`.")))
    }
}

/// The module `type_style`: a `TypeStyle` for each entry of `flat.type_style`, and `ALL`.
fn write_type_styles(tokens: &Tokens) -> String {
    let mut out = String::new();
    let styles = tokens.entries("type_style");
    for (key, entry) in styles {
        let path = format!("flat.type_style.{key}");
        let font = entry["font"]
            .as_str()
            .expect("a type style names its `font`");
        let weight = entry["weight"]
            .as_u64()
            .expect("a type style has a `weight`");
        // An optional `italic`, for CSS `font-style: italic`.
        let italic = entry["italic"].as_bool().unwrap_or(false);
        let (size, _) = tokens.number(&entry["size"], &path);
        let (line, _) = tokens.number(&entry["line_height"], &path);
        // An optional `letter_spacing`, in ems of the size, as CSS writes it.
        let spacing = entry["letter_spacing"]
            .as_f64()
            .map_or(0.0, |em| (em * f64::from(size) * 100.0).round() / 100.0);
        let doc = entry["note"]
            .as_str()
            .map_or(format!("`{path}`."), str::to_string);
        writeln!(
            out,
            "/// {doc}\npub const {key}: TypeStyle = TypeStyle {{\n    font: Font::{font},\n    \
             weight: {weight},\n    italic: {italic},\n    size: {size:?},\n    line_height: {line:?},\n    letter_spacing: {spacing:?},\n}};"
        )
        .unwrap();
    }
    let names: Vec<&str> = styles.keys().map(String::as_str).collect();
    writeln!(
        out,
        "/// Every type style, for the glyph rasterizer to prepare before the first frame.\n\
         pub const ALL: [TypeStyle; {}] = [{}];",
        names.len(),
        names.join(", ")
    )
    .unwrap();
    out
}

/// The module `color`: each token of kind `color`, then each entry of `flat.color`, the
/// `color` token it names with its own `alpha`.
fn write_colors(tokens: &Tokens) -> String {
    let find = |name: &str| tokens.find(name);
    let mut out = String::new();
    for token in tokens.list.iter().filter(|token| token["kind"] == "color") {
        let name = token["name"].as_str().unwrap();
        let rgba = parse_color(token["value"].as_str().unwrap(), &find);
        let constant = name.to_uppercase().replace('-', "_");
        let doc = token["note"]
            .as_str()
            .map_or(format!("`--{name}`."), |note| format!("`--{name}`: {note}"));
        writeln!(out, "/// {doc}\npub const {constant}: [u8; 4] = {rgba:?};").unwrap();
    }
    for (key, entry) in tokens.entries("color") {
        // A flat color is a token's, named by `color`, or a CSS color of its own in `value`.
        let (mut rgba, base) = match (entry["color"].as_str(), entry["value"].as_str()) {
            (Some(base), _) => (
                parse_color(find(base)["value"].as_str().unwrap(), &find),
                format!("`--{base}`."),
            ),
            (None, Some(value)) => (parse_color(value, &find), format!("`{value}`.")),
            (None, None) => panic!("`flat.color.{key}` names its `color` or gives its `value`"),
        };
        if let Some(alpha) = entry["alpha"].as_u64() {
            rgba[3] = u8::try_from(alpha).expect("`alpha` is from 0 to 255");
        }
        let doc = entry["note"].as_str().map_or(base, str::to_string);
        writeln!(out, "/// {doc}\npub const {key}: [u8; 4] = {rgba:?};").unwrap();
    }
    out
}

/// A CSS color as RGBA: `#rrggbb`, `rgb(r g b)`, `rgb(r g b / a%)`, or `rgb(var(--name))`
/// where the token `name` holds `r g b`. `find` looks a token up by name.
fn parse_color<'a>(value: &str, find: &dyn Fn(&str) -> &'a serde_json::Value) -> [u8; 4] {
    if let Some(hex) = value.strip_prefix('#') {
        let channel = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).unwrap();
        assert_eq!(hex.len(), 6, "`{value}` is a six-digit hex color");
        return [channel(0), channel(2), channel(4), 0xff];
    }
    let inner = value
        .strip_prefix("rgb(")
        .and_then(|v| v.strip_suffix(')'))
        .unwrap_or_else(|| panic!("`{value}` is not a color the build script reads"));
    let inner = match inner
        .strip_prefix("var(--")
        .and_then(|v| v.strip_suffix(')'))
    {
        Some(name) => find(name)["value"].as_str().unwrap(),
        None => inner,
    };
    let (channels, alpha) = inner.split_once('/').unwrap_or((inner, "100%"));
    let mut rgba = [0xff; 4];
    for (slot, channel) in rgba.iter_mut().zip(channels.split_whitespace()) {
        *slot = channel
            .parse()
            .unwrap_or_else(|_| panic!("`{value}` has a bad channel"));
    }
    let percent: f64 = alpha
        .trim()
        .strip_suffix('%')
        .and_then(|a| a.parse().ok())
        .unwrap_or_else(|| panic!("`{value}` has a bad alpha"));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let byte = (percent * 255.0 / 100.0).round() as u8;
    rgba[3] = byte;
    rgba
}

/// Writes the module `space` from `space-constants.json` by one rule, whatever the keys are. An object
/// becomes a `pub mod` named for its key. A number becomes a `pub const` named for its key in
/// upper case: a `usize` when its dotted path is listed in the top-level `counts` array, and
/// an `f32` otherwise. An array of two or three numbers becomes a `glam::Vec2` or `glam::Vec3`,
/// and an array of such arrays a fixed-size array of them. `counts` itself is not written.
fn write_space(json: &serde_json::Value, out: &mut String) {
    let counts: Vec<&str> = json["counts"]
        .as_array()
        .map(|paths| paths.iter().filter_map(serde_json::Value::as_str).collect())
        .unwrap_or_default();
    out.push_str("/// Spatial constants from `space-constants.json` and the layout profiles.\npub mod space {\n");
    let object = json.as_object().expect("space-constants.json is an object");
    write_members(object, "", 1, &counts, out);
    out.push_str("}\n");
}

/// Writes the members of `object`, whose own dotted path is `path`, at `depth` levels of
/// indentation.
fn write_members(
    object: &serde_json::Map<String, serde_json::Value>,
    path: &str,
    depth: usize,
    counts: &[&str],
    out: &mut String,
) {
    let indent = "    ".repeat(depth);
    for (key, value) in object {
        if path.is_empty() && key == "counts" {
            continue;
        }
        let dotted = if path.is_empty() {
            key.clone()
        } else {
            format!("{path}.{key}")
        };
        writeln!(out, "{indent}/// `{dotted}` in the spatial tuning.").unwrap();
        if let Some(members) = value.as_object() {
            let module: String = key
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect();
            writeln!(out, "{indent}pub mod {module} {{").unwrap();
            write_members(members, &dotted, depth + 1, counts, out);
            writeln!(out, "{indent}}}").unwrap();
        } else {
            let name = key.to_uppercase();
            let (kind, expression) = if counts.contains(&dotted.as_str()) {
                let count = value
                    .as_u64()
                    .unwrap_or_else(|| panic!("space-constants.json's `{dotted}` is a count"));
                ("usize".to_string(), count.to_string())
            } else {
                value_expression(value).unwrap_or_else(|| {
                    panic!("space-constants.json's `{dotted}` is not a number or vector")
                })
            };
            writeln!(out, "{indent}pub const {name}: {kind} = {expression};").unwrap();
        }
    }
}

/// The type and the constant expression of a number, a vector or an array of vectors.
fn value_expression(value: &serde_json::Value) -> Option<(String, String)> {
    if let Some(number) = value.as_f64() {
        return Some(("f32".to_string(), format!("{number:?}_f32")));
    }
    let items = value.as_array()?;
    if items.iter().all(serde_json::Value::is_number) && matches!(items.len(), 2 | 3) {
        let numbers: Vec<String> = items
            .iter()
            .map(|item| format!("{:?}_f32", item.as_f64().unwrap()))
            .collect();
        let kind = format!("glam::Vec{}", items.len());
        return Some((kind.clone(), format!("{kind}::new({})", numbers.join(", "))));
    }
    let elements: Vec<(String, String)> =
        items.iter().map(value_expression).collect::<Option<_>>()?;
    let kind = &elements.first()?.0;
    if kind == "f32" || elements.iter().any(|(other, _)| other != kind) {
        return None;
    }
    let expressions: Vec<&str> = elements.iter().map(|(_, e)| e.as_str()).collect();
    Some((
        format!("[{kind}; {}]", elements.len()),
        format!("[{}]", expressions.join(", ")),
    ))
}
