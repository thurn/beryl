//! `pixels`: where the web interface's screenshot of a moment and Beryl 3D's frame of it differ,
//! pixel by pixel, and which element on each side is answerable for each difference
//! (docs/beryl-3d.md, "Matching the web interface's look"). `just b3d match pixels` takes the
//! snapshot and pairs its elements first, then runs this on the snapshot directory (`--dir`).
//!
//! Once boxes and styles agree, what is left shows only in pixels: a glow drawn too faintly, a
//! missing outline, a panel behind the wrong layer. This command
//!
//! 1. compares `web.png` and `b3d.png`, which have the same size and scale, and keeps the pixels
//!    whose color differs by more than `diff::LEVELS`;
//! 2. leaves out glyph pixels: each text line of both sides' records, shrunk to its ink, since
//!    text is compared as data and not as glyphs;
//! 3. groups the rest into regions of pixels that touch, and drops specks;
//! 4. gives each differing pixel to the topmost element that paints at its point on each side:
//!    on the web the topmost by paint order (`z`) and box, in Beryl 3D the element its hit test
//!    reaches (`agent::hit`), or the topmost by `z` and box where that element paints nothing;
//! 5. prints one line for each element, or each pair of elements the matcher paired
//!    (`joined.json`), worst first, with a hint when one side has no element there, and writes a
//!    crop of the web, Beryl 3D and their difference, magnified, for the worst `CROPS`.
//!
//! It writes `pixels.json` and the crops (`crops/<n>.png`) into the snapshot directory.

mod diff;

use std::fmt::Write;
use std::path::Path;

use beryl_inspect::cli::{self, Options, Usage};
use beryl_render::{Picture, read_rgba_png, write_rgba_png};
use beryl_scene::agent::hit::hit;
use glam::Vec2;
use serde_json::{Value, json};

use crate::Error;
use crate::compare::beside;
use diff::{Image, Pixels, Row};

/// How many of the worst rows get a crop.
const CROPS: usize = 10;
/// How many times a crop is magnified, unless that would make it larger than `PANEL` pixels.
const MAGNIFY: f32 = 4.0;
/// The widest and tallest a crop's panel is drawn, in pixels. A crop larger than this, such as
/// one of a panel across the whole window, is shrunk to fit.
const PANEL: f32 = 1200.0;
/// How far around a row's differing pixels its crop reaches, in points.
const MARGIN: f32 = 8.0;
/// How many times a difference is amplified in a crop's third panel, so a faint one shows.
const AMPLIFY: u16 = 4;
/// The color of a glyph pixel, left out of the score, in a crop's third panel.
const MASKED: [u8; 4] = [40, 40, 120, 255];

/// A point Beryl 3D's hit test has not been asked about yet.
const ASKED: u32 = u32::MAX - 1;

/// One element of a snapshot's records: what `pixels` needs of `web.json` or `b3d.json`.
#[derive(Clone, Debug)]
pub(crate) struct Record {
    /// Its locator, or its role and name.
    pub label: String,
    /// `[x, y, width, height]` in points.
    pub rect: [f32; 4],
    pub z: i64,
    /// Whether it paints anything of its own: a fill, an image, a shadow, a border, text, a
    /// card or an icon. A layout wrapper paints nothing.
    pub paints: bool,
    /// Its lines of text, as `[x, y, width, height]` in points.
    pub lines: Vec<[f32; 4]>,
    /// The index of its parent element, if it has one.
    pub parent: Option<usize>,
}

/// Leaves painting, in `records`, only to the element at `root` and those inside it, when a
/// snapshot draws that element alone (`--only`).
fn only_inside(records: &mut [Record], root: usize) {
    let inside: Vec<bool> = (0..records.len())
        .map(|at| {
            let mut up = Some(at);
            while let Some(here) = up {
                if here == root {
                    return true;
                }
                up = records[here].parent;
            }
            false
        })
        .collect();
    for (record, inside) in records.iter_mut().zip(inside) {
        record.paints &= inside;
    }
}

fn rect(value: &Value) -> Option<[f32; 4]> {
    let list = value.as_array()?;
    let mut out = [0.0; 4];
    for (slot, number) in out.iter_mut().zip(list) {
        *slot = number.as_f64()? as f32;
    }
    (list.len() == 4).then_some(out)
}

/// Whether a record's style paints: as the web dump's `paints` decides, by its fill, image,
/// shadow and border.
fn style_paints(style: &Value) -> bool {
    let alpha = |name: &str| style[name].get(3).and_then(Value::as_f64).unwrap_or(0.0);
    // A gradient or shadow list is parsed into structures; only "none" or nothing paints none.
    let set = |name: &str| !style[name].is_null() && style[name].as_str() != Some("none");
    alpha("background-color") > 0.0
        || set("background-image")
        || set("box-shadow")
        || (style["border-width"].as_f64().unwrap_or(0.0) > 0.0 && alpha("border-color") > 0.0)
}

/// The records of a snapshot's `web.json` or `b3d.json`, and its scale.
fn records(path: &Path) -> Result<(Vec<Record>, f32), Error> {
    let text = std::fs::read_to_string(path)
        .map_err(|why| Error::Failed(format!("{}: {why}", path.display())))?;
    let document: Value = serde_json::from_str(&text)
        .map_err(|why| Error::Failed(format!("{}: {why}", path.display())))?;
    let scale = document["scale"].as_f64().unwrap_or(1.0) as f32;
    let elements = document["elements"].as_array().cloned().unwrap_or_default();
    let records = (elements.iter().enumerate())
        .map(|(at, element)| {
            let lines: Vec<[f32; 4]> = (element["text"].as_array().into_iter().flatten())
                .filter_map(|line| rect(&line["box"]))
                .collect();
            let role = element["role"].as_str().unwrap_or("");
            let label = match element["key"].as_str() {
                Some(key) if !key.is_empty() => key.to_owned(),
                _ => format!(
                    "{role} \"{}\" [{at}]",
                    element["name"].as_str().unwrap_or("")
                ),
            };
            Record {
                label,
                rect: rect(&element["box"]).unwrap_or_default(),
                z: element["z"].as_i64().unwrap_or(0),
                paints: style_paints(&element["style"])
                    || !lines.is_empty()
                    || !element["object"].is_null()
                    || matches!(role, "card" | "ability" | "pile" | "icon" | "arrow"),
                lines,
                parent: element["parent"]
                    .as_u64()
                    .and_then(|parent| usize::try_from(parent).ok()),
            }
        })
        .collect();
    Ok((records, scale))
}

/// The matcher's pairs in a snapshot's `joined.json`, as (web index, Beryl 3D index).
fn pairs(path: &Path) -> Result<Vec<(usize, usize)>, Error> {
    let text = std::fs::read_to_string(path).map_err(|why| {
        Error::Failed(format!(
            "{}: {why}; `just b3d match pixels` pairs the elements first",
            path.display()
        ))
    })?;
    let document: Value = serde_json::from_str(&text)
        .map_err(|why| Error::Failed(format!("{}: {why}", path.display())))?;
    Ok((document["pairs"].as_array().into_iter().flatten())
        .filter_map(|pair| {
            Some((
                usize::try_from(pair["web"].as_u64()?).ok()?,
                usize::try_from(pair["b3d"].as_u64()?).ok()?,
            ))
        })
        .collect())
}

fn picture(path: &Path) -> Result<Picture, Error> {
    read_rgba_png(path).map_err(Error::Failed)
}

pub(crate) fn pixels(options: &Options) -> Result<String, Error> {
    let dir = Path::new(options.value("--dir").ok_or_else(|| {
        Usage(
            "pixels needs --dir <snapshot directory>; `just b3d match pixels` gives it".to_owned(),
        )
    })?);
    let web = picture(&dir.join("web.png"))?;
    let b3d = picture(&dir.join("b3d.png"))?;
    if (web.width, web.height) != (b3d.width, b3d.height) {
        return Err(Error::Failed(format!(
            "web.png is {}x{} px and b3d.png {}x{}: take the snapshot again (--fresh)",
            web.width, web.height, b3d.width, b3d.height
        )));
    }
    let (mut web_records, scale) = records(&dir.join("web.json"))?;
    let (mut b3d_records, _) = records(&dir.join("b3d.json"))?;
    let pairs = pairs(&dir.join("joined.json"))?;
    let (width, height) = (web.width, web.height);
    let web_image = Image {
        pixels: &web.pixels,
        width,
        height,
    };
    let b3d_image = Image {
        pixels: &b3d.pixels,
        width,
        height,
    };

    // Beryl 3D's frame of the moment, for its hit test.
    let mut page = cli::open(options)?;
    cli::seek_and_act(&mut page, options, &options.at)?;
    let frame = page.frame();
    if frame.resolved.len() != b3d_records.len() {
        return Err(Error::Failed(format!(
            "b3d.json has {} elements and the frame {}: take the snapshot again (--fresh)",
            b3d_records.len(),
            frame.resolved.len()
        )));
    }
    // A snapshot drawn with `--only` holds the whole window's records, but only one element's
    // subtree is drawn on each side, so only those elements can be answerable.
    if let Some(locator) = options.value("--only") {
        let root = beryl_scene::agent::locate(frame, locator)
            .map_err(|error| Usage(format!("--only: {error}")))?;
        only_inside(&mut b3d_records, root as usize);
        let web_root = (options.value("--web-only"))
            .and_then(|at| at.parse::<usize>().ok())
            .ok_or_else(|| Usage("--only needs --web-only <index of its web pair>".to_owned()))?;
        only_inside(&mut web_records, web_root);
    }

    let mask = diff::glyph_mask(
        [(web_image, &web_records), (b3d_image, &b3d_records)],
        scale,
    );
    let differ = diff::differing(web_image, b3d_image, &mask);
    let (label, sizes) = diff::regions(&differ, width);
    let points = (
        (width as f32 / scale).ceil() as u32,
        (height as f32 / scale).ceil() as u32,
    );
    let web_grid = diff::owners(&web_records, points.0, points.1);
    let b3d_grid = diff::owners(&b3d_records, points.0, points.1);
    let cell = |x: u32, y: u32| (y.min(points.1 - 1) * points.0 + x.min(points.0 - 1)) as usize;
    let owner = |grid: &[u32], x: u32, y: u32| {
        let at = grid[cell(x, y)];
        (at != u32::MAX).then_some(at as usize)
    };
    let web_owner = |x: u32, y: u32| owner(&web_grid, x, y);
    // The hit test is the slowest step, so each point is asked once.
    let mut asked = vec![ASKED; b3d_grid.len()];
    let mut b3d_owner = |x: u32, y: u32| {
        let slot = &mut asked[cell(x, y)];
        if *slot == ASKED {
            let centre = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
            let found = hit(frame, centre)
                .map(|at| at as usize)
                .filter(|&at| b3d_records[at].paints)
                .or_else(|| owner(&b3d_grid, x, y));
            *slot = found.map_or(u32::MAX, |at| at as u32);
        }
        (*slot != u32::MAX).then_some(*slot as usize)
    };
    let (rows, nobody) = diff::attribute(
        (&label, &sizes),
        (width, scale),
        &web_owner,
        &mut b3d_owner,
        (&pairs, [web_records.len(), b3d_records.len()]),
    );

    let mut out = format!("{}\n", options.command_line());
    let found = Found {
        images: [web_image, b3d_image],
        records: [&web_records, &b3d_records],
        mask: &mask,
        differ: &differ,
        sizes: &sizes,
        rows: &rows,
        nobody,
        scale,
    };
    report(&mut out, dir, &found)?;
    Ok(out)
}

/// What `pixels` found, for `report`.
struct Found<'a> {
    images: [Image<'a>; 2],
    records: [&'a [Record]; 2],
    mask: &'a [bool],
    differ: &'a [bool],
    sizes: &'a [usize],
    rows: &'a [Row],
    nobody: usize,
    scale: f32,
}

/// Writes `found`'s summary and rows to `out`, and its crops and `pixels.json` into `dir`.
fn report(out: &mut String, dir: &Path, found: &Found<'_>) -> Result<(), Error> {
    let Found {
        images: [web_image, b3d_image],
        records: [web_records, b3d_records],
        mask,
        differ,
        sizes,
        rows,
        nobody,
        scale,
    } = *found;
    let (width, height) = (web_image.width, web_image.height);
    let crops = dir.join("crops");
    let _ = std::fs::remove_dir_all(&crops);
    std::fs::create_dir_all(&crops)
        .map_err(|why| Error::Failed(format!("{}: {why}", crops.display())))?;
    let differing = differ.iter().filter(|&&d| d).count();
    let glyphs = mask.iter().filter(|&&m| m).count();
    let specks: usize = sizes.iter().filter(|&&size| size < diff::SPECK).sum();
    let _ = writeln!(
        *out,
        "{width}x{height} px at scale {scale}: {differing} pixels differ by more than {} levels, \
         in {} regions ({specks} in specks, not counted); {glyphs} glyph pixels left out; \
         {nobody} differing pixels lie where neither side has an element that paints",
        diff::LEVELS,
        sizes.len(),
    );
    let mut listed = Vec::new();
    for (rank, row) in rows.iter().enumerate() {
        let crop = match (rank < CROPS, row.area) {
            (true, Some(area)) => {
                let path = crops.join(format!("{:02}.png", rank + 1));
                write_crop(&path, area, [web_image, b3d_image], mask, scale)?;
                Some(path)
            }
            _ => None,
        };
        line(out, row, (web_records, b3d_records), crop.as_deref());
        listed.push(json!({
            "web": row.web,
            "b3d": row.b3d,
            "pixels": row.pixels(),
            "web_pixels": row.web_pixels,
            "b3d_pixels": row.b3d_pixels,
            "hint": row.hint(),
            "crop": crop.map(|path| path.display().to_string()),
        }));
    }
    let summary = json!({
        "differing": differing,
        "counted": differing - specks,
        "glyphs": glyphs,
        "regions": sizes.len(),
        "nobody": nobody,
        "rows": listed,
    });
    let path = dir.join("pixels.json");
    std::fs::write(&path, format!("{summary:#}\n"))
        .map_err(|why| Error::Failed(format!("{}: {why}", path.display())))?;
    let _ = writeln!(out, "wrote {} and {}", path.display(), crops.display());
    Ok(())
}

/// Writes one row's line: its pixels, its element on each side and their boxes, its hint and
/// its crop.
fn line(out: &mut String, row: &Row, (web, b3d): (&[Record], &[Record]), crop: Option<&Path>) {
    let name = row
        .b3d
        .map(|at| b3d[at].label.as_str())
        .or_else(|| row.web.map(|at| web[at].label.as_str()))
        .unwrap_or("");
    let boxed = |records: &[Record], at: Option<usize>| {
        at.map_or_else(
            || "none".to_owned(),
            |at| {
                let [x, y, w, h] = records[at].rect;
                format!("[{x} {y} {w} {h}]")
            },
        )
    };
    let _ = write!(
        out,
        "{:>8} px  {name}  (web {} px, Beryl 3D {} px)",
        row.pixels(),
        row.web_pixels,
        row.b3d_pixels
    );
    if let Some(hint) = row.hint() {
        let _ = write!(out, "  {hint}");
    }
    let _ = write!(
        out,
        "\n            web {}  b3d {}",
        boxed(web, row.web),
        boxed(b3d, row.b3d)
    );
    if row.web.is_some() && row.b3d.is_none() {
        let _ = write!(out, "  web element: {}", web[row.web.unwrap_or(0)].label);
    }
    if let Some(crop) = crop {
        let _ = write!(out, "\n            crop {}", crop.display());
    }
    out.push('\n');
}

/// Writes the crop of `area` and a margin around it: the web's pixels, Beryl 3D's and their
/// difference amplified, side by side and magnified (or shrunk to fit `PANEL`), with glyph
/// pixels in `MASKED`.
fn write_crop(
    path: &Path,
    area: Pixels,
    [web, b3d]: [Image<'_>; 2],
    mask: &[bool],
    scale: f32,
) -> Result<(), Error> {
    let area = area.grown((MARGIN * scale) as u32, web.width, web.height);
    let (w, h) = (area.right - area.left, area.bottom - area.top);
    let factor = MAGNIFY.min(PANEL / w as f32).min(PANEL / h as f32);
    let size = (
        ((w as f32 * factor) as u32).max(1),
        ((h as f32 * factor) as u32).max(1),
    );
    let panel = |pick: &dyn Fn(usize) -> [u8; 4]| {
        let mut pixels = Vec::with_capacity((size.0 * size.1 * 4) as usize);
        for y in 0..size.1 {
            let row = area.top + ((y as f32 / factor) as u32).min(h - 1);
            for x in 0..size.0 {
                let column = area.left + ((x as f32 / factor) as u32).min(w - 1);
                pixels.extend_from_slice(&pick((row * web.width + column) as usize));
            }
        }
        pixels
    };
    let amplified = |at: usize| {
        if mask[at] {
            return MASKED;
        }
        let (a, b) = (opaque(web, at), opaque(b3d, at));
        let level = u16::from(diff::difference(a, b)) * AMPLIFY;
        let level = u8::try_from(level.min(255)).unwrap_or(255);
        [level, level, level, 255]
    };
    let (two, two_width, two_height) = beside(
        &panel(&|at| opaque(web, at)),
        size,
        &panel(&|at| opaque(b3d, at)),
        size,
    );
    let (all, all_width, all_height) =
        beside(&two, (two_width, two_height), &panel(&amplified), size);
    write_rgba_png(path, &all, all_width, all_height).map_err(Error::Failed)
}

/// The pixel at `at` of `image`, made opaque.
fn opaque(image: Image<'_>, at: usize) -> [u8; 4] {
    let i = at * 4;
    [
        image.pixels[i],
        image.pixels[i + 1],
        image.pixels[i + 2],
        255,
    ]
}
