//! The allocation gate: no `Client::frame` allocates once the game has started
//! (docs/beryl-3d.md, "Performance gates"). The `gate` binary plays every gated board under a
//! counting global allocator and prints each frame's allocations, with a backtrace of the
//! first, which this test prints when it fails.

use std::process::Command;

use serde_json::Value;

#[test]
fn no_frame_of_a_gated_board_allocates() {
    let output = Command::new(env!("CARGO_BIN_EXE_gate"))
        .output()
        .expect("the gate binary runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let printed: Value = serde_json::from_slice(&output.stdout).expect("the gate prints JSON");
    let boards = printed["boards"].as_array().expect("a list of boards");
    assert!(!boards.is_empty());
    for board in boards {
        let first = &board["first_frame_allocation"];
        assert!(
            first.is_null(),
            "{} frame {}: Client::frame allocated, first at\n{}",
            board["name"],
            first["frame"],
            first["backtrace"].as_str().unwrap_or("")
        );
    }
}
