//! Plays the gated boards natively and prints what each frame did as JSON, for `just b3d gate`
//! (`tools/src/beryl_tools/beryl3d/gate.py`), which holds the numbers to the limits in
//! `data/beryl3d/gates.json` (docs/beryl-3d.md, "Performance gates").
//!
//! For each frame it prints the client core's work counters, the draw list's load, and the heap
//! allocations made inside `Client::frame`, which a counting global allocator sees. For each
//! board it prints the allocations made in `Client::handle`, the backtrace of the first
//! allocation inside a frame, if any, and the frames `rank` picks, among which the
//! instruction-count gate expects the frame with the most instructions.
//!
//! Usage: `gate [--board <name>]...`, every board of `beryl_inspect::gate::BOARDS` when none is
//! named.

use std::alloc::{GlobalAlloc, Layout, System};
use std::backtrace::Backtrace;
use std::process::ExitCode;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering};

use beryl_inspect::gate::{BOARDS, Playback, rank};
use serde_json::{Value, json};

/// What the program is doing, so that the allocator knows which count an allocation joins.
const ELSEWHERE: u8 = 0;
const HANDLE: u8 = 1;
const FRAME: u8 = 2;

static REGION: AtomicU8 = AtomicU8::new(ELSEWHERE);
static HANDLE_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static FRAME_ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
/// Set while the first frame allocation's backtrace is taken, which itself allocates.
static CAPTURING: AtomicBool = AtomicBool::new(false);
/// The backtrace of the first allocation inside a frame, once there has been one.
static FIRST: Mutex<Option<Backtrace>> = Mutex::new(None);

/// The system allocator, counting the allocations made inside `Client::handle` and
/// `Client::frame`.
struct Counting;

impl Counting {
    fn count() {
        if CAPTURING.load(Ordering::Relaxed) {
            return;
        }
        match REGION.load(Ordering::Relaxed) {
            HANDLE => {
                HANDLE_ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
            }
            FRAME if FRAME_ALLOCATIONS.fetch_add(1, Ordering::Relaxed) == 0 => {
                CAPTURING.store(true, Ordering::Relaxed);
                let backtrace = Backtrace::force_capture();
                if let Ok(mut first) = FIRST.lock() {
                    *first = Some(backtrace);
                }
                CAPTURING.store(false, Ordering::Relaxed);
            }
            _ => {}
        }
    }
}

#[expect(unsafe_code, reason = "a global allocator is an unsafe trait")]
// SAFETY: every call is passed straight to the system allocator.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        Self::count();
        // SAFETY: the caller upholds `alloc`'s contract, which is the system allocator's.
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        // SAFETY: `pointer` came from `alloc` above, so from the system allocator.
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        Self::count();
        // SAFETY: as for `dealloc`, and the caller upholds `realloc`'s contract.
        unsafe { System.realloc(pointer, layout, size) }
    }
}

#[global_allocator]
static GLOBAL: Counting = Counting;

/// Plays the board called `name` and returns what it did.
fn measure(name: &str) -> Option<Value> {
    let mut playback = Playback::new(name)?;
    HANDLE_ALLOCATIONS.store(0, Ordering::Relaxed);
    FRAME_ALLOCATIONS.store(0, Ordering::Relaxed);
    *FIRST.lock().ok()? = None;
    let mut frames = Vec::new();
    let mut first_allocation = None;
    while let Some(moment) = playback.moment() {
        REGION.store(HANDLE, Ordering::Relaxed);
        playback.handle();
        let before = FRAME_ALLOCATIONS.load(Ordering::Relaxed);
        REGION.store(FRAME, Ordering::Relaxed);
        let frame = playback.frame();
        REGION.store(ELSEWHERE, Ordering::Relaxed);
        let allocations = FRAME_ALLOCATIONS.load(Ordering::Relaxed) - before;
        if allocations > 0 && first_allocation.is_none() {
            first_allocation = Some(moment.frame);
        }
        let (work, load) = (frame.work, frame.load);
        frames.push(json!({
            "frame": moment.frame,
            "ms": moment.now / 1_000_000,
            "phase": moment.phase.name(),
            "elements": work.elements,
            "segments": work.segments,
            "records": work.records,
            "layout_nodes": work.layout_nodes,
            "text_runs": work.text_runs,
            "placed": work.placed,
            "steps": work.steps,
            "draw_calls": load.draw_calls,
            "instances": load.instances,
            "upload_bytes": load.upload_bytes,
            "blended_area": frame.areas().blended,
            "frame_allocations": allocations,
        }));
    }
    let backtrace = FIRST
        .lock()
        .ok()?
        .take()
        .map(|backtrace| backtrace.to_string());
    Some(json!({
        "name": name,
        "handle_allocations": HANDLE_ALLOCATIONS.load(Ordering::Relaxed),
        "frames": frames,
        "ranked": rank(name)?,
        "first_frame_allocation": first_allocation.map(|frame| json!({
            "frame": frame,
            "backtrace": backtrace,
        })),
    }))
}

fn main() -> ExitCode {
    let mut names = Vec::new();
    let mut arguments = std::env::args().skip(1);
    while let Some(argument) = arguments.next() {
        let ("--board", Some(name)) = (argument.as_str(), arguments.next()) else {
            eprintln!("usage: gate [--board <name>]...; the boards are {BOARDS:?}");
            return ExitCode::from(2);
        };
        names.push(name);
    }
    if names.is_empty() {
        names = BOARDS.iter().map(|name| (*name).to_owned()).collect();
    }
    let mut boards = Vec::new();
    for name in &names {
        let Some(board) = measure(name) else {
            eprintln!("gate: no gated board {name:?}; the boards are {BOARDS:?}");
            return ExitCode::from(2);
        };
        boards.push(board);
    }
    println!("{}", json!({ "boards": boards }));
    ExitCode::SUCCESS
}
