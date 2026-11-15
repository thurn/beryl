//! Times the gated boards' frames on this machine, paced at 60 frames a second as a window
//! draws them, for the doc's measurements (docs/beryl-3d.md, "Measurements"). Frame times are
//! not gated: the instruction and fragment counts are, because they are the same on every run.
//!
//! Each frame is timed three ways with the wall clock:
//!
//! - `client`: the host's work in the client core, the `handle` calls before the frame and
//!   `Client::frame`;
//! - `encode`: `Renderer::render`, which writes the records, encodes the frame and submits it;
//! - `done`: from the end of the submission to the GPU finishing the frame. GPU timestamps do not
//!   work on the Mac, so this includes the driver's latency.
//!
//! Between frames the thread sleeps to the next sixtieth of a second, because a frame drawn
//! back to back after another costs several times less than one drawn after a sleep, when the
//! processor and the GPU have lowered their clocks.
//!
//! Usage: `frame_times [--board <name>]... [--size <width>x<height>] [--runs <n>] [--downlevel]`.
//! `--downlevel` makes the device with wgpu's downlevel limits, the floor's feature set, where a
//! validation error ends the program with a panic. The boards
//! are drawn in the gates' 1440 by 900 point window at two pixels per point unless `--size`
//! gives a window in pixels at one pixel per point. The faces are not uploaded, so cards sample
//! empty layers of the face arrays at the same cost. The output is one JSON object per board,
//! with each run's mean and worst of each time in microseconds and the load average before it.

use std::process::{Command, ExitCode};
use std::time::{Duration, Instant};

use beryl_inspect::gate::{BOARDS, FRAME_NANOS, Playback, TARGET};
use beryl_render::{Limits, Renderer, Target};
use beryl_scene::input::Size;
use serde_json::{Value, json};

/// One time's mean and worst over a run's frames, in microseconds.
#[derive(Default)]
struct Spread {
    total: f64,
    worst: f64,
    frames: usize,
}

impl Spread {
    fn add(&mut self, time: Duration) {
        let micros = time.as_secs_f64() * 1e6;
        self.total += micros;
        self.worst = self.worst.max(micros);
        self.frames += 1;
    }

    fn json(&self) -> Value {
        json!({
            "mean_us": self.total / self.frames.max(1) as f64,
            "worst_us": self.worst,
        })
    }
}

/// The one-minute load average, or `None` where `sysctl` cannot give it.
fn load() -> Option<f64> {
    let out = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()?;
    // `{ 51.20 55.31 60.02 }`
    String::from_utf8(out.stdout)
        .ok()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

/// One paced run of the board `name`.
fn run(renderer: &mut Renderer, name: &str, window: Option<Size>) -> Value {
    let mut playback = match window {
        Some(size) => Playback::in_window(name, size, 1.0),
        None => Playback::new(name),
    }
    .expect("a gated board");
    let [width, height] = window.map_or(TARGET, |size| [size.width as u32, size.height as u32]);
    let mut target = Target::offscreen(renderer, width, height);
    let load = load();
    let (mut client, mut encode, mut done) =
        (Spread::default(), Spread::default(), Spread::default());
    let tick = Duration::from_nanos(FRAME_NANOS);
    let started = Instant::now();
    let mut next = started;
    while playback.moment().is_some() {
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            std::thread::sleep(wait);
        }
        next += tick;
        let t0 = Instant::now();
        playback.handle();
        let frame = playback.frame();
        let t1 = Instant::now();
        renderer.render(frame, &mut target);
        let t2 = Instant::now();
        renderer.finish();
        let t3 = Instant::now();
        client.add(t1 - t0);
        encode.add(t2 - t1);
        done.add(t3 - t2);
    }
    json!({
        "load": load,
        "frames": client.frames,
        "client": client.json(),
        "encode": encode.json(),
        "done": done.json(),
    })
}

fn usage() -> ExitCode {
    eprintln!(
        "usage: frame_times [--board <name>]... [--size <width>x<height>] [--runs <n>] \
         [--downlevel]; \
         the gated boards are {BOARDS:?}"
    );
    ExitCode::from(2)
}

fn main() -> ExitCode {
    let mut boards = Vec::new();
    let mut window = None;
    let mut runs = 5;
    let mut limits = Limits::Default;
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        if argument == "--downlevel" {
            limits = Limits::Downlevel;
            continue;
        }
        match (argument.as_str(), arguments.next()) {
            ("--board", Some(name)) if BOARDS.contains(&name.as_str()) => boards.push(name),
            ("--size", Some(size)) => {
                let Some((w, h)) = size.split_once('x') else {
                    return usage();
                };
                let (Ok(width), Ok(height)) = (w.parse::<f32>(), h.parse::<f32>()) else {
                    return usage();
                };
                window = Some(Size { width, height });
            }
            ("--runs", Some(n)) => match n.parse() {
                Ok(n) => runs = n,
                Err(_) => return usage(),
            },
            _ => return usage(),
        }
    }
    if boards.is_empty() {
        boards = BOARDS.iter().map(|name| (*name).to_owned()).collect();
    }
    let mut renderer = match Renderer::headless(limits) {
        Ok(renderer) => renderer,
        Err(error) => {
            eprintln!("frame_times: {error}");
            return ExitCode::from(2);
        }
    };
    for name in &boards {
        // One unmeasured run first, so that the first measured one does not pay for the
        // pipelines' first use and the target's first allocation.
        run(&mut renderer, name, window);
        let measured: Vec<Value> = (0..runs)
            .map(|_| run(&mut renderer, name, window))
            .collect();
        let [width, height] = window.map_or(TARGET, |size| [size.width as u32, size.height as u32]);
        println!(
            "{}",
            json!({
                "board": name,
                "adapter": renderer.adapter(),
                "downlevel": limits == Limits::Downlevel,
                "target": [width, height],
                "runs": measured,
            })
        );
    }
    ExitCode::SUCCESS
}
