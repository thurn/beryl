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
//!   "border": [[r, g, b, a], width] | null, "radius": r, "opacity": o}}]}
//! ```
//!
//! Colors are channels from 0 to 255. For each sample it prints one line of JSON, `{"name",
//! "max", "mean", "p995", "picture"}`: the largest and the mean difference of any color channel,
//! the difference that 99.5% of pixels are within, and a PNG of the web's picture, Beryl 3D's and
//! their difference amplified, side by side.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use beryl_inspect::cli::{Options, Usage};
use beryl_render::{Limits, Renderer, Target};
use beryl_scene::element::{Element, Position, holder, row};
use beryl_scene::frame::{Frame, FrameBuilder};
use beryl_scene::input::{Insets, Size, Tier};
use beryl_scene::layout::Layer;
use beryl_scene::table::{Camera, DESKTOP, Window};
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
    let mut renderer = Renderer::headless(Limits::Default)
        .map_err(|why| Error::Failed(format!("no GPU to draw with: {why}")))?;
    let mut printed = String::new();
    for sample in &samples {
        let web = beryl_render::read_rgba_png(&sample.web).map_err(Error::Failed)?;
        let (pixels, width, height) = draw(&mut renderer, sample, scale);
        if (web.width, web.height) != (width, height) {
            return Err(failed(&format!(
                "{}: the web's picture is {}x{} px and Beryl 3D's {width}x{height}",
                sample.name, web.width, web.height
            )));
        }
        let score = score(&web.pixels, &pixels);
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
    Ok(Sample {
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

/// Fills the whole window from its corner: an element placed at offsets of 0 from every edge.
fn filling(element: Element) -> Element {
    element.position(Position::Absolute).inset([Some(0.0); 4])
}

/// The sample as the screen the flat layer shows: the page's color filling the window and the
/// sample's element in its middle. The screen's own element is a clear holder, since a screen's
/// top element is otherwise drawn on the usual panel.
fn screen(sample: &Sample) -> Element {
    let mut element = row()
        .width(sample.size.width)
        .height(sample.size.height)
        .opacity(sample.opacity);
    element.style.background = sample.background;
    element.style.border = sample.border;
    // A radius of 0 means the usual one, which is square for an element that is not a button.
    element.style.radius = sample.radius;
    let page = filling(row().background(sample.page))
        .align_items(beryl_scene::element::Align::Center)
        .justify_content(beryl_scene::element::Justify::Center)
        .child(element);
    filling(holder()).child(page)
}

/// Draws `sample` at `scale` and returns its pixels and their size.
fn draw(renderer: &mut Renderer, sample: &Sample, scale: f32) -> (Vec<u8>, u32, u32) {
    let window = Window {
        size: sample.window,
        scale,
        safe_area: Insets::default(),
    };
    let mut layer = Layer::new();
    layer.resize(window);
    layer.show(Some(screen(sample)));
    let camera = Camera::fit(DESKTOP.camera, window);
    let mut frame = Frame::with_capacity(16, 16, 64);
    let mut builder = FrameBuilder::new(&mut frame, Tier::Baseline);
    builder.camera(camera.view_projection);
    layer.write(&mut builder, &camera, true);
    let frame = builder.finish();
    let pixels = |points: f32| (points * scale).round().max(1.0) as u32;
    let (width, height) = (pixels(sample.window.width), pixels(sample.window.height));
    let mut target = Target::offscreen(renderer, width, height);
    renderer.render(frame, &mut target);
    (renderer.read(&target), width, height)
}

/// How far `found` is from `expected`, two RGBA pictures of one size, by their color channels.
fn score(expected: &[u8], found: &[u8]) -> Score {
    let mut counts = [0_u64; 256];
    let mut total = 0_u64;
    for (a, b) in expected
        .as_chunks::<4>()
        .0
        .iter()
        .zip(found.as_chunks::<4>().0.iter())
    {
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
        let score = score(&expected, &found);
        assert_eq!((score.max, score.p995), (30, 3));
        assert!((score.mean - 36.0 / 400.0).abs() < 1e-9);
    }

    #[test]
    fn a_plain_panel_draws_its_color_on_the_page() {
        let mut renderer = Renderer::headless(Limits::Default).expect("a GPU adapter and device");
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
        };
        let (pixels, width, height) = draw(&mut renderer, &sample, 2.0);
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
