//! Counts the fragments the Baseline tier shades, for `just b3d gate --gpu`
//! (`tools/src/beryl_tools/beryl3d/gpu.py`), which holds the counts to the limits under
//! `tiers.baseline.fragments` in `data/beryl3d/gates.json` (docs/beryl-3d.md, "What is
//! counted"). It needs a GPU adapter with pipeline statistics queries, so it runs in the
//! `beryl3d-bench` virtual machine on Mesa's software Vulkan driver, whose count is exact.
//!
//! `--board <name>` (every gated board when neither option is given) draws the 8 frames of a gated board that `beryl_inspect::gate::rank` ranks
//! highest, each into a target of `gate::TARGET`, and prints each frame's count.
//!
//! `--recorded <name> --at <ms>` draws one frame of a recording, from the first seat in a 1920
//! by 1080 window, both as the client built it (opaque records with a depth buffer, then the
//! blended ones) and with every record of the table drawn blended from back to front with no
//! depth writes. The doc's Baseline table compares the two.
//!
//! The output is one JSON object: `{"adapter", "boards": [...], "recorded": [...]}`.

use std::process::ExitCode;

use beryl_inspect::gate::{self, BOARDS, Moment};
use beryl_inspect::pages;
use beryl_render::{Limits, Renderer, Target};
use beryl_scene::agent::Layout;
use beryl_scene::driver::Duration;
use beryl_scene::frame::Frame;
use beryl_scene::input::Size;
use beryl_session::protocol::Side;
use serde_json::{Value, json};

/// The window a recording is drawn in for the doc's comparison, at one pixel per point.
const RECORDED_WINDOW: Size = Size {
    width: 1920.0,
    height: 1080.0,
};

/// Draws `frame` into `target` and returns how many fragments it shaded.
fn count(renderer: &mut Renderer, frame: &Frame, target: &mut Target) -> u64 {
    renderer.render(frame, target);
    renderer
        .fragments()
        .expect("a counting renderer counts fragments")
}

/// The ranked frames of the gated board `name` and their counts.
fn board(renderer: &mut Renderer, name: &str) -> Option<Value> {
    let ranked = gate::rank(name)?;
    let [width, height] = gate::TARGET;
    let mut target = Target::offscreen(renderer, width, height);
    let mut frames = Vec::new();
    gate::play(name, |moment: Moment, frame| {
        if ranked.contains(&moment.frame) {
            frames.push(json!({
                "frame": moment.frame,
                "ms": moment.now / 1_000_000,
                "phase": moment.phase.name(),
                "fragments": count(renderer, frame, &mut target),
            }));
        }
    });
    Some(json!({
        "name": name,
        "pixels": u64::from(width) * u64::from(height),
        "frames": frames,
    }))
}

/// `frame` with every record of the table drawn blended, farthest first, and none writing the
/// depth buffer: how the frame would be drawn with no depth buffer to reject hidden pixels.
fn all_blended(frame: &Frame) -> Frame {
    let mut blended = frame.clone();
    let layer = (blended.layer as usize).min(blended.draws.len());
    let depth = |model: &[f32; 12]| {
        let clip = frame.view_projection * glam::Vec4::new(model[3], model[7], model[11], 1.0);
        clip.z / clip.w
    };
    // Depth runs from 0 at the near plane to 1 at the far one.
    blended.draws[..layer].sort_by(|a, b| depth(&b.model).total_cmp(&depth(&a.model)));
    blended.opaque = 0;
    blended
}

/// One frame of the recording `name`, `at_ms` after its start, counted both ways.
fn recorded(renderer: &mut Renderer, name: &str, at_ms: u64) -> Result<Value, String> {
    let mut page = pages::page(name, Side::You, Layout::Desktop, RECORDED_WINDOW)?;
    page.step(Duration::from_millis(at_ms));
    let frame = page.frame().clone();
    let (width, height) = (RECORDED_WINDOW.width as u32, RECORDED_WINDOW.height as u32);
    let mut target = Target::offscreen(renderer, width, height);
    Ok(json!({
        "name": name,
        "ms": at_ms,
        "pixels": u64::from(width) * u64::from(height),
        "records": frame.draws.len(),
        "opaque_records": frame.opaque,
        "opaque_with_depth": count(renderer, &frame, &mut target),
        "all_blended": count(renderer, &all_blended(&frame), &mut target),
    }))
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: fragments [--board <name>]... [--recorded <name> --at <ms>]...; \
         the gated boards are {BOARDS:?}"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut boards = Vec::new();
    let mut recordings = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match (argument.as_str(), arguments.next()) {
            ("--board", Some(name)) => boards.push(name),
            ("--recorded", Some(name)) => {
                let at = match (arguments.next().as_deref(), arguments.next()) {
                    (Some("--at"), Some(ms)) => ms.parse::<u64>().ok(),
                    _ => None,
                };
                let Some(at) = at else {
                    return usage();
                };
                recordings.push((name, at));
            }
            _ => return usage(),
        }
    }
    if boards.is_empty() && recordings.is_empty() {
        boards = BOARDS.iter().map(|name| (*name).to_owned()).collect();
    }
    let mut renderer = match Renderer::counting(Limits::Default) {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("fragments: {error}");
            return ExitCode::from(2);
        }
    };
    let mut counted = Vec::new();
    for name in &boards {
        let Some(board) = board(&mut renderer, name) else {
            eprintln!("fragments: no gated board {name:?}; the boards are {BOARDS:?}");
            return ExitCode::from(2);
        };
        counted.push(board);
    }
    let mut frames = Vec::new();
    for (name, at) in &recordings {
        match recorded(&mut renderer, name, *at) {
            Ok(frame) => frames.push(frame),
            Err(error) => {
                eprintln!("fragments: {error}");
                return ExitCode::from(2);
            }
        }
    }
    println!(
        "{}",
        json!({
            "adapter": renderer.counting_adapter(),
            "boards": counted,
            "recorded": frames,
        })
    );
    ExitCode::SUCCESS
}
