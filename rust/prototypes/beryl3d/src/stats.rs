//! `--stats <board>`: the same core and renderer drawing into a texture, five runs, each number
//! printed as the median and the worst of the five with the load average beside them.

use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use beryl_session::GameUpdate;

use crate::core::{Core, Draw, Input};
use crate::headless::Offscreen;
use crate::measure;
use crate::{Board, Setup};

const RUNS: usize = 5;
const FRAME: u64 = 16_666_667;
/// Allocations are counted from this time on: the first second sets the game up.
const SETTLED: u64 = 1_000_000_000;

#[derive(Clone, Copy, Default)]
struct Run {
    load: f64,
    core_mean: f64,
    core_worst: f64,
    encode_mean: f64,
    encode_worst: f64,
    allocations_per_frame: f64,
    allocations_worst: f64,
    records: f64,
    draw_calls: f64,
    opaque_area: f64,
    opaque_area_worst: f64,
    blended_area: f64,
    blended_area_worst: f64,
    gpu_built: f64,
    gpu_naive: f64,
    wall_built: f64,
    wall_naive: f64,
    longest_searching: f64,
    longest_idle: f64,
    searching_frames: f64,
}

/// A live game's core with its session, or a stress board's.
pub struct Live {
    pub sessions: beryl_session::Sessions,
    pub id: beryl_session::protocol::SessionId,
    pub updates: Receiver<GameUpdate>,
}

pub fn make(board: Board, setup: &Setup, width: u32, height: u32) -> (Core, Option<Live>) {
    match board {
        Board::Live => {
            let (sessions, id, updates) = crate::game::start(|| {});
            let core = Core::new(width, height, setup.timing, setup.faces.faces.clone());
            (
                core,
                Some(Live {
                    sessions,
                    id,
                    updates,
                }),
            )
        }
        Board::Stress(cards) => (
            Core::stress(
                width,
                height,
                setup.timing,
                setup.faces.faces.clone(),
                cards,
            ),
            None,
        ),
    }
}

pub fn drain(core: &mut Core, live: Option<&Live>) {
    if let Some(live) = live {
        while let Ok(update) = live.updates.try_recv() {
            core.handle(Input::Update(Box::new(update)));
        }
    }
}

fn run(
    board: Board,
    setup: &Setup,
    offscreen: &mut Offscreen,
    frames: usize,
    first: &mut Option<f64>,
) -> (Run, Vec<Draw>) {
    let (width, height) = (offscreen.width, offscreen.height);
    let (mut core, live) = make(board, setup, width, height);
    let started = Instant::now();
    let mut result = Run {
        load: measure::load_average(),
        ..Run::default()
    };
    let (mut core_total, mut encode_total, mut allocations, mut counted) = (0.0, 0.0, 0.0, 0.0);
    let (mut opaque_total, mut blended_total) = (0.0, 0.0);
    // A stress board normally draws frames back to back; paced, it sleeps to the next 60 Hz
    // tick as a live game and a window do, so the clocks fall between frames.
    let paced = live.is_some() || std::env::var_os("BERYL3D_PACED").is_some();
    let mut now = 0;
    for frame in 0..frames {
        now = if live.is_some() {
            started.elapsed().as_nanos() as u64
        } else {
            frame as u64 * FRAME
        };
        drain(&mut core, live.as_ref());
        let t0 = Instant::now();
        let (stats, made) = measure::count_allocations(|| core.frame(now).stats);
        let core_ns = t0.elapsed().as_nanos() as f64;
        let t1 = Instant::now();
        let drawn = offscreen.renderer.draw(
            &offscreen.view,
            width,
            height,
            core.camera.view_proj,
            &core.draw.records,
            stats.opaque,
            false,
        );
        let encode_ns = t1.elapsed().as_nanos() as f64;
        offscreen.renderer.wait_idle();
        if first.is_none() && (live.is_none() || core.has_game) {
            *first = Some(setup.started.elapsed().as_secs_f64() * 1e3);
        }
        core_total += core_ns;
        encode_total += encode_ns;
        result.core_worst = result.core_worst.max(core_ns);
        result.encode_worst = result.encode_worst.max(encode_ns);
        if now >= SETTLED {
            allocations += made as f64;
            counted += 1.0;
            result.allocations_worst = result.allocations_worst.max(made as f64);
        }
        result.records = result.records.max(stats.records as f64);
        result.draw_calls = result.draw_calls.max(f64::from(drawn.draw_calls));
        opaque_total += f64::from(stats.opaque_area);
        blended_total += f64::from(stats.blended_area);
        result.opaque_area_worst = result.opaque_area_worst.max(f64::from(stats.opaque_area));
        result.blended_area_worst = result.blended_area_worst.max(f64::from(stats.blended_area));
        let cpu = core_ns + encode_ns;
        if core.thinking {
            result.longest_searching = result.longest_searching.max(cpu);
            result.searching_frames += 1.0;
        } else {
            result.longest_idle = result.longest_idle.max(cpu);
        }
        if paced {
            let next = Duration::from_nanos((frame as u64 + 1) * FRAME);
            if let Some(wait) = next.checked_sub(started.elapsed()) {
                std::thread::sleep(wait);
            }
        }
    }
    let n = frames as f64;
    result.core_mean = core_total / n;
    result.encode_mean = encode_total / n;
    result.allocations_per_frame = if counted > 0.0 {
        allocations / counted
    } else {
        0.0
    };
    result.opaque_area = opaque_total / n;
    result.blended_area = blended_total / n;
    // The same frame drawn as built and drawn all blended with no depth buffer: the GPU's own
    // timestamps where they work, and the wall-clock time from submitting to the GPU finishing.
    let (mut built, mut naive, mut built_wall, mut naive_wall) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for _ in 0..31 {
        let stats = core.frame(now).stats;
        let t = Instant::now();
        offscreen.renderer.draw(
            &offscreen.view,
            width,
            height,
            core.camera.view_proj,
            &core.draw.records,
            stats.opaque,
            false,
        );
        built.extend(offscreen.renderer.wait());
        offscreen.renderer.wait_idle();
        built_wall.push(t.elapsed().as_nanos() as f64);
        let records = core.frame_naive(now).to_vec();
        let t = Instant::now();
        offscreen.renderer.draw(
            &offscreen.view,
            width,
            height,
            core.camera.view_proj,
            &records,
            0,
            true,
        );
        naive.extend(offscreen.renderer.wait());
        offscreen.renderer.wait_idle();
        naive_wall.push(t.elapsed().as_nanos() as f64);
    }
    result.gpu_built = median(built.iter().map(|&v| v as f64).collect());
    result.gpu_naive = median(naive.iter().map(|&v| v as f64).collect());
    result.wall_built = median(built_wall);
    result.wall_naive = median(naive_wall);
    let records = core.draw.records.clone();
    if let Some(live) = live {
        let _ = live.sessions.concede(live.id);
    }
    (result, records)
}

fn median(mut values: Vec<f64>) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    values.sort_by(f64::total_cmp);
    values[values.len() / 2]
}

/// Whether every record of `records` draws on a device with WebGL 2's limits, the floor's
/// feature set.
fn webgl2(
    setup: &Setup,
    width: u32,
    height: u32,
    records: &[Draw],
    view_proj: glam::Mat4,
) -> Result<(), String> {
    let (_adapter, device, queue, _) =
        crate::headless::device(Some(wgpu::Limits::downlevel_webgl2_defaults()));
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let mut offscreen = Offscreen::with_device(device, queue, width, height, &setup.faces, false);
    let opaque = records
        .iter()
        .take_while(|r| r.material != crate::core::draw::MATERIAL_SHADOW)
        .count();
    offscreen.renderer.draw(
        &offscreen.view,
        width,
        height,
        view_proj,
        records,
        opaque,
        false,
    );
    offscreen.renderer.wait_idle();
    match pollster::block_on(scope.pop()) {
        None => Ok(()),
        Some(error) => Err(error.to_string()),
    }
}

pub fn stats(board: Board, setup: &Setup, frames: usize) {
    let (width, height) = (1920, 1080);
    let mut offscreen = Offscreen::new(width, height, &setup.faces, None);
    if std::env::var_os("BERYL3D_PACED").is_some() {
        println!("paced at 60 Hz");
    }
    if std::env::var_os("BERYL3D_PACED").is_some() {
        println!("paced at 60 Hz");
    }
    println!(
        "board {}  frames {frames} per run  {RUNS} runs  {width}x{height}  faces: {} scans, {} placeholders",
        board.name(),
        setup.faces.scans,
        setup.faces.missing
    );
    println!(
        "GPU timestamps: {}",
        if offscreen.renderer.has_timestamps() {
            "yes"
        } else {
            "no"
        }
    );
    let mut runs = Vec::new();
    let mut first = None;
    let mut records = Vec::new();
    for i in 0..RUNS {
        let (result, last) = run(board, setup, &mut offscreen, frames, &mut first);
        println!(
            "  run {}: load {:.1}, core {:.0} ns/frame, encode+submit {:.0} ns/frame",
            i + 1,
            result.load,
            result.core_mean,
            result.encode_mean
        );
        runs.push(result);
        records = last;
    }
    let column = |f: fn(&Run) -> f64| {
        let mut values: Vec<f64> = runs.iter().map(f).collect();
        values.sort_by(f64::total_cmp);
        (values[RUNS / 2], values[RUNS - 1])
    };
    let row = |name: &str, f: fn(&Run) -> f64, unit: &str| {
        let (median, worst) = column(f);
        println!("{name:<52} {median:>14.3} {worst:>14.3}  {unit}");
    };
    let loads: Vec<String> = runs.iter().map(|r| format!("{:.1}", r.load)).collect();
    println!("load average at each run's start: {}", loads.join(", "));
    println!(
        "{:<52} {:>14} {:>14}",
        "measure", "median of 5", "worst of 5"
    );
    row("core time per frame, mean", |r| r.core_mean, "ns");
    row("core time, worst single frame", |r| r.core_worst, "ns");
    row("encode and submit per frame, mean", |r| r.encode_mean, "ns");
    row(
        "encode and submit, worst single frame",
        |r| r.encode_worst,
        "ns",
    );
    row(
        "heap allocations per core frame after 1 s, mean",
        |r| r.allocations_per_frame,
        "",
    );
    row(
        "heap allocations in one core frame after 1 s, most",
        |r| r.allocations_worst,
        "",
    );
    row("draw records, most in a frame", |r| r.records, "");
    row("draw calls, most in a frame", |r| r.draw_calls, "");
    row(
        "opaque records' screen area, mean",
        |r| r.opaque_area,
        "screens",
    );
    row(
        "opaque records' screen area, most",
        |r| r.opaque_area_worst,
        "screens",
    );
    row(
        "blended records' screen area, mean",
        |r| r.blended_area,
        "screens",
    );
    row(
        "blended records' screen area, most",
        |r| r.blended_area_worst,
        "screens",
    );
    row(
        "GPU timestamp end minus start, frame as built",
        |r| r.gpu_built,
        "ns",
    );
    row(
        "GPU timestamp end minus start, all blended",
        |r| r.gpu_naive,
        "ns",
    );
    row("submit to GPU done, frame as built", |r| r.wall_built, "ns");
    row(
        "submit to GPU done, all blended, no depth",
        |r| r.wall_naive,
        "ns",
    );
    if board == Board::Live {
        row(
            "longest frame (core + encode) while AI searched",
            |r| r.longest_searching,
            "ns",
        );
        row(
            "longest frame (core + encode) while it did not",
            |r| r.longest_idle,
            "ns",
        );
        row(
            "frames drawn while the AI searched",
            |r| r.searching_frames,
            "",
        );
    }
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| std::fs::metadata(p).ok())
        .map_or(0, |m| m.len());
    println!("peak memory: {:.1} MB", measure::peak_memory() as f64 / 1e6);
    println!(
        "executable size (release, stripped): {:.1} MB",
        exe as f64 / 1e6
    );
    println!(
        "time from start to first frame: {:.0} ms",
        first.unwrap_or(f64::NAN)
    );
    let (core, _) = make(Board::Stress(4), setup, width, height);
    match webgl2(setup, width, height, &records, core.camera.view_proj) {
        Ok(()) => println!("draws on a device with downlevel_webgl2_defaults limits: yes"),
        Err(error) => {
            println!("draws on a device with downlevel_webgl2_defaults limits: NO: {error}")
        }
    }
}
