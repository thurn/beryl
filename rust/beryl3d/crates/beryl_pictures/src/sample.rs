//! `sample`: the Beryl 3D side of `just b3d match samples` (docs/beryl-3d.md, "Matching the web
//! interface's look"). Each conformance sample is one element with a CSS style, centred in a small
//! window on a page color. This command lays each one out as a flat-layer element with the
//! `Style` the sample's CSS stands for, draws it with the real renderer offscreen, and measures
//! how far its pixels are from the screenshot `WebKit` took of the same sample, which `match samples`
//! takes first.
//!
//! The job is a JSON file, so that one run draws every sample on one renderer:
//!
//! ```text
//! {"scale": 2, "out": "<directory>", "samples": [{"name", "window": [w, h], "page": [r, g, b, a],
//!   "box": [w, h], "web": "<png>", "style": {"background": [r, g, b, a] | null,
//!   "border": [[r, g, b, a], width] | null, "radius": r, "opacity": o,
//!   "box_shadow": [shadow], "drop_shadow": [shadow]},
//!   "text": {"string", "color": [r, g, b, a], "size"} | null}]}
//! ```
//!
//! where a shadow is `{"x", "y", "blur", "spread", "color": [r, g, b, a], "inset"}`. Text is set
//! in Open Sans at the line height of Beryl 3D's body text, centred in the box, and its glyphs
//! are rasterized by the real glyph worker. Glyph pixels are left out of the score (decision
//! M2): every pixel Beryl 3D's glyphs cover, found by drawing the text alone in white on black,
//! and the pixels beside those, since the two rasterizers' antialiasing differs.
//!
//! Colors are channels from 0 to 255. For each sample it prints one line of JSON, `{"name",
//! "max", "mean", "p995", "picture"}`: the largest and the mean difference of any color channel,
//! the difference that 99.5% of pixels are within, and a PNG of the web's picture, Beryl 3D's and
//! their difference amplified, side by side.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use beryl_inspect::cli::{Options, Usage};
use beryl_render::{Limits, Renderer, Target};
use std::sync::mpsc::{self, Receiver};

use beryl_assets::Prepared;
use beryl_assets::glyphs::GlyphWorker;
use beryl_assets::light;
use beryl_scene::element::{Align, Element, Justify, Position, Shadow, holder, row};
use beryl_scene::frame::{Frame, FrameBuilder};
use beryl_scene::input::AssetReady;
use beryl_scene::input::{Insets, Size, Tier};
use beryl_scene::layout::Layer;
use beryl_scene::table::{Camera, DESKTOP, Window};
use beryl_scene::text::TypeStyle;
use beryl_scene::text::fonts::Font;
use serde_json::Value;

use crate::Error;

/// How many times a difference is multiplied in the third picture, so that a difference of a
/// few levels shows.
const AMPLIFY: u16 = 16;

/// The space between the three pictures, in pixels.
const GAP: u32 = 8;

/// One sample of the job.
struct Sample {
    name: String,
    window: Size,
    page: [u8; 4],
    size: Size,
    web: PathBuf,
    background: Option<[u8; 4]>,
    border: Option<([u8; 4], f32)>,
    radius: f32,
    opacity: f32,
    box_shadow: Vec<Shadow>,
    drop_shadow: Vec<Shadow>,
    text: Option<SampleText>,
}

/// A sample's text.
struct SampleText {
    string: String,
    color: [u8; 4],
    size: f32,
}

/// How far one sample's picture is from the web's.
#[derive(Debug, PartialEq)]
struct Score {
    max: u8,
    mean: f64,
    p995: u8,
}

pub(crate) fn sample(options: &Options) -> Result<String, Error> {
    let job_path = options.value("--job").ok_or_else(|| {
        Usage("sample needs --job <json>; `just b3d match samples` writes it".to_owned())
    })?;
    let job: Value = std::fs::read_to_string(job_path)
        .map_err(|why| Error::Failed(format!("{job_path}: {why}")))
        .and_then(|text| {
            serde_json::from_str(&text).map_err(|why| Error::Failed(format!("{job_path}: {why}")))
        })?;
    let scale = job["scale"].as_f64().unwrap_or(2.0) as f32;
    let out = PathBuf::from(
        job["out"]
            .as_str()
            .ok_or_else(|| failed("the job has no out"))?,
    );
    let samples = job["samples"]
        .as_array()
        .ok_or_else(|| failed("the job has no samples"))?
        .iter()
        .map(read_sample)
        .collect::<Result<Vec<_>, _>>()?;
    let renderer = Renderer::headless(Limits::Default)
        .map_err(|why| Error::Failed(format!("no GPU to draw with: {why}")))?;
    let mut drawer = Drawer::new(renderer, scale);
    let mut printed = String::new();
    for sample in &samples {
        let web = beryl_render::read_rgba_png(&sample.web).map_err(Error::Failed)?;
        let (pixels, width, height) = drawer.draw(sample, scale, false);
        let mask = sample
            .text
            .is_some()
            .then(|| glyph_mask(&drawer.draw(sample, scale, true)));
        if (web.width, web.height) != (width, height) {
            return Err(failed(&format!(
                "{}: the web's picture is {}x{} px and Beryl 3D's {width}x{height}",
                sample.name, web.width, web.height
            )));
        }
        let score = score(&web.pixels, &pixels, mask.as_deref());
        let picture = out.join(format!("{}.png", sample.name));
        write_beside(&picture, &web.pixels, &pixels, width, height)?;
        let line = serde_json::json!({
            "name": sample.name,
            "max": score.max,
            "mean": (score.mean * 100.0).round() / 100.0,
            "p995": score.p995,
            "picture": picture.display().to_string(),
        });
        let _ = writeln!(printed, "{line}");
    }
    Ok(printed)
}

fn failed(why: &str) -> Error {
    Error::Failed(format!("sample: {why}"))
}

fn color(value: &Value) -> Option<[u8; 4]> {
    let list = value.as_array()?;
    let mut out = [0; 4];
    for (slot, channel) in out.iter_mut().zip(list) {
        *slot = u8::try_from(channel.as_u64()?).ok()?;
    }
    (list.len() == 4).then_some(out)
}

fn pair(value: &Value) -> Option<Size> {
    Some(Size {
        width: value.get(0)?.as_f64()? as f32,
        height: value.get(1)?.as_f64()? as f32,
    })
}

fn read_sample(value: &Value) -> Result<Sample, Error> {
    let name = value["name"].as_str().unwrap_or("?").to_owned();
    let bad = |what: &str| failed(&format!("{name}: {what}"));
    let style = &value["style"];
    let border = match &style["border"] {
        Value::Null => None,
        given => Some((
            color(&given[0]).ok_or_else(|| bad("border color"))?,
            given[1].as_f64().ok_or_else(|| bad("border width"))? as f32,
        )),
    };
    let background = match &style["background"] {
        Value::Null => None,
        given => Some(color(given).ok_or_else(|| bad("background"))?),
    };
    let box_shadow = shadows(&style["box_shadow"]).ok_or_else(|| bad("box_shadow"))?;
    let drop_shadow = shadows(&style["drop_shadow"]).ok_or_else(|| bad("drop_shadow"))?;
    let text = match &value["text"] {
        Value::Null => None,
        given => Some(SampleText {
            string: given["string"]
                .as_str()
                .ok_or_else(|| bad("text"))?
                .to_owned(),
            color: color(&given["color"]).ok_or_else(|| bad("text color"))?,
            size: given["size"].as_f64().ok_or_else(|| bad("text size"))? as f32,
        }),
    };
    Ok(Sample {
        box_shadow,
        drop_shadow,
        text,
        window: pair(&value["window"]).ok_or_else(|| bad("window"))?,
        page: color(&value["page"]).ok_or_else(|| bad("page"))?,
        size: pair(&value["box"]).ok_or_else(|| bad("box"))?,
        web: PathBuf::from(value["web"].as_str().ok_or_else(|| bad("web"))?),
        background,
        border,
        radius: style["radius"].as_f64().unwrap_or(0.0) as f32,
        opacity: style["opacity"].as_f64().unwrap_or(1.0) as f32,
        name,
    })
}

/// A list of shadows as the job writes them, or `None` if one is malformed. A missing list is
/// empty.
fn shadows(value: &Value) -> Option<Vec<Shadow>> {
    let Some(list) = value.as_array() else {
        return value.is_null().then(Vec::new);
    };
    list.iter()
        .map(|shadow| {
            let length = |name: &str| shadow[name].as_f64().map(|v| v as f32);
            Some(Shadow {
                x: length("x")?,
                y: length("y")?,
                blur: length("blur")?,
                spread: length("spread")?,
                color: color(&shadow["color"])?,
                inset: shadow["inset"].as_bool()?,
            })
        })
        .collect()
}

/// Fills the whole window from its corner: an element placed at offsets of 0 from every edge.
fn filling(element: Element) -> Element {
    element.position(Position::Absolute).inset([Some(0.0); 4])
}

/// The sample as the screen the flat layer shows: the page's color filling the window and the
/// sample's element in its middle, with its text centred in it. The screen's own element is a
/// clear holder, since a screen's top element is otherwise drawn on the usual panel. A `mask`
/// screen draws only the text, in white on black, to find the glyphs' pixels.
fn screen(sample: &Sample, mask: bool) -> Element {
    let mut element = row()
        .width(sample.size.width)
        .height(sample.size.height)
        .align_items(Align::Center)
        .justify_content(Justify::Center);
    if !mask {
        element.style.opacity = sample.opacity;
        element.style.background = sample.background;
        element.style.border = sample.border;
        // A radius of 0 means the usual one, which is square for an element that is not a
        // button.
        element.style.radius = sample.radius;
        element.style.box_shadow.clone_from(&sample.box_shadow);
    }
    if let Some(text) = &sample.text {
        let mut words = beryl_scene::element::text(&text.string)
            .style(TypeStyle {
                font: Font::OpenSans,
                weight: 400,
                size: text.size,
                line_height: TEXT_LINE_HEIGHT,
            })
            .color(if mask { [255; 4] } else { text.color });
        if !mask {
            words.style.drop_shadow.clone_from(&sample.drop_shadow);
        }
        element = element.child(words);
    }
    let page = if mask { [0, 0, 0, 255] } else { sample.page };
    let page = filling(row().background(page))
        .align_items(Align::Center)
        .justify_content(Justify::Center)
        .child(element);
    filling(holder()).child(page)
}

/// The line height a sample's text is set at, which `match samples` gives the web's page too:
/// Beryl 3D's body text's.
const TEXT_LINE_HEIGHT: f32 = 1.4;

/// The renderer the samples are drawn with, and the glyph worker that rasterizes their text.
struct Drawer {
    renderer: Renderer,
    glyphs: GlyphWorker,
    pages: Receiver<Prepared>,
    /// The glyphs and icons the worker has delivered, which every sample's layer is given.
    ready: Vec<AssetReady>,
}

impl Drawer {
    fn new(mut renderer: Renderer, scale: f32) -> Self {
        renderer.upload_page(&light::prepared());
        let (out, pages) = mpsc::channel();
        let glyphs = GlyphWorker::spawn(out, || {}, || {});
        glyphs.icons(scale);
        Self {
            renderer,
            glyphs,
            pages,
            ready: Vec::new(),
        }
    }

    /// Draws `sample` (or its `mask` screen) at `scale` and returns its pixels and their size.
    fn draw(&mut self, sample: &Sample, scale: f32, mask: bool) -> (Vec<u8>, u32, u32) {
        let window = Window {
            size: sample.window,
            scale,
            safe_area: Insets::default(),
        };
        let mut layer = Layer::new();
        layer.resize(window);
        self.give(&mut layer);
        layer.show(Some(screen(sample, mask)));
        // Laying the screen out asked for its glyphs; the worker rasterizes them before the
        // frame is drawn, as the headless host's does.
        let wanted: Vec<_> = layer.glyphs_mut().drain_wanted().collect();
        self.glyphs.request(wanted);
        self.glyphs.flush();
        while let Ok(prepared) = self.pages.try_recv() {
            if self.renderer.upload_page(&prepared) {
                self.ready.push(prepared.ready);
            }
        }
        self.give(&mut layer);
        let camera = Camera::fit(DESKTOP.camera, window);
        let mut frame = Frame::with_capacity(16, 16, 64);
        let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
        builder.camera(camera.view_projection);
        layer.write(&mut builder, &camera, true);
        let frame = builder.finish();
        let pixels = |points: f32| (points * scale).round().max(1.0) as u32;
        let (width, height) = (pixels(sample.window.width), pixels(sample.window.height));
        let mut target = Target::offscreen(&self.renderer, width, height);
        self.renderer.render(frame, &mut target);
        (self.renderer.read(&target), width, height)
    }

    /// Gives `layer` every glyph and icon delivered so far.
    fn give(&self, layer: &mut Layer) {
        for ready in &self.ready {
            match ready {
                AssetReady::Glyphs { page, glyphs } => layer.glyphs_mut().add(*page, glyphs),
                AssetReady::Icons { page, cells } => layer.icons_mut().add(*page, cells),
                _ => {}
            }
        }
    }
}

/// Which pixels of a mask screen's picture (`screen`) a glyph covers, or lies next to one that
/// does: those left out of the score.
fn glyph_mask((pixels, width, height): &(Vec<u8>, u32, u32)) -> Vec<bool> {
    let (width, height) = (*width as usize, *height as usize);
    let inked = |x: usize, y: usize| pixels[(y * width + x) * 4..][..3].iter().any(|&c| c > 0);
    let mut mask = vec![false; width * height];
    for y in 0..height {
        for x in 0..width {
            let near = (y.saturating_sub(1)..=(y + 1).min(height - 1))
                .any(|ny| (x.saturating_sub(1)..=(x + 1).min(width - 1)).any(|nx| inked(nx, ny)));
            mask[y * width + x] = near;
        }
    }
    mask
}

/// How far `found` is from `expected`, two RGBA pictures of one size, by their color channels,
/// leaving out the pixels `mask` marks.
fn score(expected: &[u8], found: &[u8], mask: Option<&[bool]>) -> Score {
    let mut counts = [0_u64; 256];
    let mut total = 0_u64;
    for (at, (a, b)) in expected
        .as_chunks::<4>()
        .0
        .iter()
        .zip(found.as_chunks::<4>().0.iter())
        .enumerate()
    {
        if mask.is_some_and(|mask| mask[at]) {
            continue;
        }
        let worst = (0..3).map(|c| a[c].abs_diff(b[c])).max().unwrap_or(0);
        counts[usize::from(worst)] += 1;
        total += u64::from(worst);
    }
    let pixels: u64 = counts.iter().sum();
    let max = counts.iter().rposition(|&n| n > 0).unwrap_or(0) as u8;
    // The smallest difference that at least 99.5% of pixels are within.
    let within = pixels - pixels / 200;
    let mut seen = 0;
    let mut p995 = 0;
    for (level, &n) in counts.iter().enumerate() {
        seen += n;
        if seen >= within {
            p995 = level as u8;
            break;
        }
    }
    Score {
        max,
        mean: if pixels == 0 {
            0.0
        } else {
            total as f64 / pixels as f64
        },
        p995,
    }
}

/// Writes the web's picture, Beryl 3D's and their difference amplified side by side to `path`.
fn write_beside(
    path: &Path,
    web: &[u8],
    native: &[u8],
    width: u32,
    height: u32,
) -> Result<(), Error> {
    let difference: Vec<u8> = web
        .as_chunks::<4>()
        .0
        .iter()
        .zip(native.as_chunks::<4>().0.iter())
        .flat_map(|(a, b)| {
            let level = |c: usize| (u16::from(a[c].abs_diff(b[c])) * AMPLIFY).min(255) as u8;
            [level(0), level(1), level(2), 255]
        })
        .collect();
    let whole = width * 3 + GAP * 2;
    let mut pixels = [128, 128, 128, 255].repeat((whole * height) as usize);
    let row_bytes = (width * 4) as usize;
    for (index, source) in [web, native, &difference].into_iter().enumerate() {
        let x = index as u32 * (width + GAP);
        for row in 0..height {
            let from = row as usize * row_bytes;
            let to = ((row * whole + x) * 4) as usize;
            pixels[to..to + row_bytes].copy_from_slice(&source[from..from + row_bytes]);
        }
    }
    beryl_render::write_rgba_png(path, &pixels, whole, height).map_err(Error::Failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_score_counts_the_worst_channel_of_each_pixel() {
        let expected = [10, 10, 10, 255].repeat(400);
        let mut found = expected.clone();
        // One pixel of 400 is 30 levels off in green and two are 3 off in blue: 398 pixels,
        // 99.5%, are within 3.
        found[1] = 40;
        found[6] = 13;
        found[10] = 13;
        let score = score(&expected, &found, None);
        assert_eq!((score.max, score.p995), (30, 3));
        assert!((score.mean - 36.0 / 400.0).abs() < 1e-9);
    }

    #[test]
    fn a_plain_panel_draws_its_color_on_the_page() {
        let renderer = Renderer::headless(Limits::Default).expect("a GPU adapter and device");
        let mut drawer = Drawer::new(renderer, 2.0);
        let sample = Sample {
            name: "plain".to_owned(),
            window: Size {
                width: 40.0,
                height: 30.0,
            },
            page: [20, 40, 60, 255],
            size: Size {
                width: 20.0,
                height: 10.0,
            },
            web: PathBuf::new(),
            background: Some([200, 100, 50, 255]),
            border: None,
            radius: 0.0,
            opacity: 1.0,
            box_shadow: Vec::new(),
            drop_shadow: Vec::new(),
            text: None,
        };
        let (pixels, width, height) = drawer.draw(&sample, 2.0, false);
        assert_eq!((width, height), (80, 60));
        let at = |x: u32, y: u32| {
            let i = ((y * width + x) * 4) as usize;
            [pixels[i], pixels[i + 1], pixels[i + 2]]
        };
        let near = |found: [u8; 3], expected: [u8; 3]| {
            found.iter().zip(expected).all(|(&a, b)| a.abs_diff(b) <= 2)
        };
        assert!(
            near(at(40, 30), [200, 100, 50]),
            "the middle {:?}",
            at(40, 30)
        );
        assert!(near(at(2, 2), [20, 40, 60]), "the corner {:?}", at(2, 2));
    }
}
