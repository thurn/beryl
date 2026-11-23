//! Plays one gated board with every frame sent through `gate::frame_counted`, for the
//! instruction-count gate, `just b3d gate --counts` (`tools/src/beryl_tools/beryl3d/counts.py`).
//!
//! The gate runs this under Callgrind in `beryl3d-bench` with collection off at the start. Around
//! each frame the program asks Callgrind, through Valgrind's client requests, to turn collection
//! on and then off, and after it to dump its counts, which also zeroes them. So dump `n` holds
//! exactly the instructions of frame `n - 1`, and one playback gives every frame's count.
//!
//! Client requests, not Callgrind's `--toggle-collect` and `--dump-after` options: on arm64,
//! Callgrind lost track of `frame_counted`'s returns, so it saw each call nested in the one
//! before (`frame_counted'2` in its output) and dumped only once, after the outermost. Played
//! natively, a client request is a few instructions that change nothing, so the program does
//! the same work and counts nothing.
//!
//! Usage: `b3d-frame-counts <board>`. It prints how many frames it played, so that the gate can
//! check that it read one dump for each.

use std::process::ExitCode;

use beryl_inspect::gate::{BOARDS, Playback, frame_counted};

/// Callgrind's client requests: `VG_USERREQ_TOOL_BASE('C', 'T')` and its offsets, from
/// Valgrind's `callgrind.h`.
const CALLGRIND_BASE: usize = (b'C' as usize) << 24 | (b'T' as usize) << 16;
const DUMP_STATS: usize = CALLGRIND_BASE;
const TOGGLE_COLLECT: usize = CALLGRIND_BASE + 2;

/// Makes the Valgrind client request `request` with no arguments. Under Valgrind the four
/// rotations of `x12`, which add up to a full turn, followed by `orr x10, x10, x10` are the
/// signal to read the request from the block `x4` points to (Valgrind's `valgrind.h`, arm64).
/// Natively they change no register.
#[cfg(target_arch = "aarch64")]
fn client_request(request: usize) {
    let block: [usize; 6] = [request, 0, 0, 0, 0, 0];
    #[expect(
        unsafe_code,
        reason = "Valgrind's client requests are an inline instruction sequence"
    )]
    // SAFETY: the sequence leaves every register as it found it except `x3`, which is declared
    // as an output, and only reads the six words `x4` points to, which live until it ends.
    unsafe {
        std::arch::asm!(
            "ror x12, x12, #3",
            "ror x12, x12, #13",
            "ror x12, x12, #51",
            "ror x12, x12, #61",
            "orr x10, x10, x10",
            inout("x3") 0_usize => _,
            in("x4") block.as_ptr(),
            options(nostack, readonly),
        );
    }
}

/// Valgrind's amd64 request marker is four rotations of `rdi` and `xchg rbx, rbx`.
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
fn client_request(request: usize) {
    let block: [usize; 6] = [request, 0, 0, 0, 0, 0];
    #[expect(
        unsafe_code,
        reason = "Valgrind's client requests are an inline instruction sequence"
    )]
    // SAFETY: the rotations restore `rdi`. Valgrind reads the live request at `rax`
    // and returns its result in `rdx`, which is declared as an output.
    unsafe {
        std::arch::asm!(
            "rol rdi, 3",
            "rol rdi, 13",
            "rol rdi, 61",
            "rol rdi, 51",
            "xchg rbx, rbx",
            in("rax") block.as_ptr(),
            inout("rdx") 0_usize => _,
            options(nostack, readonly),
        );
    }
}

/// Other targets can play back the board without Callgrind collecting anything.
#[cfg(not(any(
    target_arch = "aarch64",
    all(target_os = "linux", target_arch = "x86_64")
)))]
fn client_request(_request: usize) {}

fn main() -> ExitCode {
    let mut arguments = std::env::args().skip(1);
    let (Some(name), None) = (arguments.next(), arguments.next()) else {
        eprintln!("usage: b3d-frame-counts <board>; the boards are {BOARDS:?}");
        return ExitCode::from(2);
    };
    let Some(mut playback) = Playback::new(&name) else {
        eprintln!("b3d-frame-counts: no gated board {name:?}; the boards are {BOARDS:?}");
        return ExitCode::from(2);
    };
    let mut frames = 0_usize;
    while playback.moment().is_some() {
        client_request(TOGGLE_COLLECT);
        frame_counted(&mut playback);
        client_request(TOGGLE_COLLECT);
        client_request(DUMP_STATS);
        frames += 1;
    }
    println!("{frames}");
    ExitCode::SUCCESS
}
