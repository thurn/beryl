//! Counts the fragments a tier shades, for `just b3d gate --gpu`
//! (`tools/src/beryl_tools/beryl3d/gpu.py`), which holds the counts to the limits under
//! fragment budgets in `data/beryl3d/budgets.json` (docs/beryl-3d.md, "What is
//! counted"). The count is the sum over every render pass of a frame: one in Baseline, up to
//! seven in Enhanced. It needs a GPU adapter with pipeline statistics queries, so it runs in the
//! `beryl3d-bench` virtual machine on Mesa's software Vulkan driver, whose count is exact.
//!
//! `--tier baseline|enhanced` (baseline by default) chooses the tier every frame is drawn in.
//!
//! `--board <name>` (every gated board when neither option is given) draws the 8 frames of a
//! gated board that `beryl_inspect::gate::rank` ranks highest, each into a target of
//! `gate::TARGET`, and prints each frame's count and draw calls.
//!
//! `--recorded <name> --at <ms>` draws one frame of a recording, from the first seat in a 1920
//! by 1080 window, both as the client built it (opaque records with a depth buffer, then the
//! blended ones) and with every record of the table drawn blended from back to front with no
//! depth writes. The doc's Baseline table compares the two.
//!
//! The output is one JSON object: `{"adapter", "tier", "boards": [...], "recorded": [...]}`.

use std::process::ExitCode;

use beryl_inspect::gate::{self, BOARDS, Playback};
use beryl_inspect::pages;
use beryl_protocol::protocol::Side;
use beryl_render::{Limits, Renderer, Target};
use kit_frame::frame::Frame;
use kit_probe::agent::Layout;
use kit_probe::driver::Duration;
use kit_ui::input::{Size, Tier};
use serde_json::{Value, json};

/// The window a recording is drawn in for the doc's comparison, at one pixel per point.
const RECORDED_WINDOW: Size = Size {
    width: 1920.0,
    height: 1080.0,
};

/// Draws `frame` into `target` and returns how many fragments it shaded and how many draw
/// calls it took.
fn count(renderer: &mut Renderer, frame: &Frame, target: &mut Target) -> (u64, u32) {
    let drawn = renderer.render(frame, target);
    let fragments = renderer
        .fragments()
        .expect("a counting renderer counts fragments");
    (fragments, drawn.draw_calls)
}

/// The ranked frames of the gated board `name`, drawn in `tier`, and their counts.
fn board(renderer: &mut Renderer, name: &str, tier: Tier) -> Option<Value> {
    let ranked = gate::rank(name)?;
    let [width, height] = gate::TARGET;
    let mut target = Target::offscreen(renderer, width, height);
    let mut frames = Vec::new();
    let mut playback = Playback::new(name)?.with_tier(tier);
    while let Some(moment) = playback.moment() {
        playback.handle();
        let frame = playback.frame();
        if ranked.contains(&moment.frame) {
            let (fragments, draw_calls) = count(renderer, frame, &mut target);
            frames.push(json!({
                "frame": moment.frame,
                "ms": moment.now / 1_000_000,
                "phase": moment.phase.name(),
                "fragments": fragments,
                "draw_calls": draw_calls,
            }));
        }
    }
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

/// One frame of the recording `name`, `at_ms` after its start, counted both ways, in Baseline.
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
        "opaque_with_depth": count(renderer, &frame, &mut target).0,
        "all_blended": count(renderer, &all_blended(&frame), &mut target).0,
    }))
}

/// `tier` as `--tier` names it.
fn tier_name(tier: Tier) -> &'static str {
    match tier {
        Tier::Baseline => "baseline",
        Tier::Enhanced => "enhanced",
    }
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: fragments [--tier baseline|enhanced] [--board <name>]... \
         [--recorded <name> --at <ms>]...; \
         the gated boards are {BOARDS:?}"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut boards = Vec::new();
    let mut recordings = Vec::new();
    let mut tier = Tier::Baseline;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        match (argument.as_str(), arguments.next()) {
            ("--board", Some(name)) => boards.push(name),
            ("--tier", Some(name)) => {
                tier = match name.as_str() {
                    "baseline" => Tier::Baseline,
                    "enhanced" => Tier::Enhanced,
                    _ => return usage(),
                };
            }
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
    if tier == Tier::Enhanced && !renderer.enhanced_available() {
        eprintln!(
            "fragments: the adapter {:?} cannot draw the Enhanced tier",
            renderer.adapter()
        );
        return ExitCode::from(2);
    }
    let mut counted = Vec::new();
    for name in &boards {
        let Some(board) = board(&mut renderer, name, tier) else {
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
            "tier": tier_name(tier),
            "boards": counted,
            "recorded": frames,
        })
    );
    ExitCode::SUCCESS
}
