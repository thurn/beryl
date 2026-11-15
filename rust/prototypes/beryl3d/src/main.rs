//! A throwaway prototype of Beryl 3D (docs/beryl-3d.md): a real game, played by the real session
//! with the AI searching on its own threads, drawn as a 3D table with wgpu.
//!
//! - no arguments: opens the window (keys 1, 2, 3, Space, S);
//! - `--stats live|stress-255|stress-1024 [--frames N]`: five runs with no window, printing what
//!   a frame costs;
//! - `--png <ms> <path> [--board B] [--size WxH] [--sweep]`: a picture of the board at a time;
//!   with `--sweep`, after the pointer has crossed the viewer's hand quickly.

mod core;
mod faces;
mod game;
mod headless;
mod measure;
mod render;
mod stats;
mod window;

use std::path::Path;
use std::time::{Duration, Instant};

use crate::core::timing::Timings;
use crate::faces::FaceImages;

#[global_allocator]
static ALLOCATOR: measure::CountingAllocator = measure::CountingAllocator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Board {
    Live,
    Stress(usize),
}

impl Board {
    fn parse(name: &str) -> Board {
        match name {
            "live" => Board::Live,
            "stress-255" => Board::Stress(255),
            "stress-1024" => Board::Stress(1024),
            _ => panic!("unknown board {name}: live, stress-255 or stress-1024"),
        }
    }

    pub fn name(self) -> String {
        match self {
            Board::Live => "live".to_owned(),
            Board::Stress(cards) => format!("stress-{cards}"),
        }
    }
}

/// What every mode loads at startup.
pub struct Setup {
    pub started: Instant,
    pub timing: Timings,
    pub faces: FaceImages,
}

fn setup(started: Instant) -> Setup {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../../data/ui-conventions.json"
    );
    let text = std::fs::read_to_string(path).expect("data/ui-conventions.json is readable");
    let conventions = serde_json::from_str(&text).expect("data/ui-conventions.json parses");
    Setup {
        started,
        timing: Timings::from_json(&conventions),
        faces: faces::load(),
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn png(setup: &Setup, board: Board, at_ms: u64, path: &Path, size: (u32, u32), sweep: bool) {
    let (width, height) = size;
    let mut offscreen = headless::Offscreen::new(width, height, &setup.faces, None);
    let (mut core, live) = stats::make(board, setup, width, height);
    let started = Instant::now();
    let end = at_ms * 1_000_000;
    let mut now = 0;
    while now < end {
        now = if live.is_some() {
            started.elapsed().as_nanos() as u64
        } else {
            (now + 16_666_667).min(end)
        };
        stats::drain(&mut core, live.as_ref());
        core.frame(now);
        if live.is_some() {
            std::thread::sleep(Duration::from_millis(16));
        }
    }
    if sweep {
        // The pointer crosses the viewer's hand from left to right in 15 ms steps and stops
        // two thirds of the way along, as a quick hand would.
        for step in 0..=20 {
            let x = -0.5 + 0.04 * step as f32;
            core.handle(core::Input::Pointer(Some(glam::Vec2::new(x, -0.85))));
            now += 15_000_000;
            core.frame(now);
        }
        now += 300_000_000;
    }
    let stats = core.frame(now).stats;
    offscreen.renderer.draw(
        &offscreen.view,
        width,
        height,
        core.camera.view_proj,
        &core.draw.records,
        stats.opaque,
        false,
    );
    offscreen.renderer.wait_idle();
    offscreen.png(path);
    println!(
        "{} at {} ms: {} records, {} draw calls, {} updates waiting, AI {}, wrote {}",
        board.name(),
        now / 1_000_000,
        stats.records,
        stats.draw_calls,
        core.queued(),
        if core.thinking { "searching" } else { "idle" },
        path.display()
    );
    if let Some(live) = live {
        let _ = live.sessions.concede(live.id);
    }
}

fn main() {
    let started = Instant::now();
    let interactive = measure::interactive();
    let args: Vec<String> = std::env::args().collect();
    if let Some(board) = flag(&args, "--stats") {
        let board = Board::parse(board);
        let default = if board == Board::Live { 1800 } else { 1200 };
        let frames = flag(&args, "--frames").map_or(default, |f| f.parse().expect("--frames N"));
        println!("frame thread in the user-interactive scheduling class: {interactive}");
        stats::stats(board, &setup(started), frames);
    } else if let Some(i) = args.iter().position(|a| a == "--png") {
        let at: u64 = args
            .get(i + 1)
            .and_then(|a| a.parse().ok())
            .expect("--png <ms> <path>");
        let path = args.get(i + 2).expect("--png <ms> <path>");
        let board = flag(&args, "--board").map_or(Board::Live, Board::parse);
        let (width, height) = flag(&args, "--size")
            .and_then(|s| s.split_once('x'))
            .map_or((1920, 1080), |(w, h)| {
                (w.parse().expect("width"), h.parse().expect("height"))
            });
        png(
            &setup(started),
            board,
            at,
            Path::new(path),
            (width, height),
            args.iter().any(|a| a == "--sweep"),
        );
    } else {
        window::run(setup(started));
    }
}
