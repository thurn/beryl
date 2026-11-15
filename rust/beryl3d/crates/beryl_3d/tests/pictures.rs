//! `b3d png` and `b3d film`, run as an agent runs them: the picture shows the card that the
//! tree says is in flight, a film is one picture of bounded width with a line for each frame,
//! and two runs started together write two files. They need a GPU adapter (Metal on a Mac).

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::{Command, Output};

use beryl_host::Headless;
use beryl_inspect::pages;
use beryl_scene::agent::{Layout, uncovered};
use beryl_scene::driver::Moment;
use beryl_scene::frame::StateBits;
use beryl_scene::input::Size;
use beryl_session::protocol::Side;

const WINDOW: Size = Size {
    width: 1600.0,
    height: 900.0,
};

fn render(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_b3d-render"))
        .args(args)
        .output()
        .expect("the program starts")
}

/// The file a successful run says it wrote, and its size from the PNG header.
fn written(output: &Output) -> (PathBuf, (u32, u32)) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        output.status.success(),
        "failed: {}\n{stdout}",
        String::from_utf8_lossy(&output.stderr)
    );
    let line = stdout
        .lines()
        .find_map(|line| line.strip_prefix("wrote "))
        .unwrap_or_else(|| panic!("no file named in:\n{stdout}"));
    let path = PathBuf::from(line.split(" (").next().expect("a path"));
    let bytes = std::fs::read(&path).expect("the file exists");
    assert_eq!(&bytes[..8], b"\x89PNG\r\n\x1a\n");
    let number = |at: usize| u32::from_be_bytes(bytes[at..at + 4].try_into().expect("four bytes"));
    (path, (number(16), number(20)))
}

#[test]
fn the_picture_shows_the_card_the_tree_says_is_in_flight() {
    let page = pages::page("verbs-zones", Side::You, Layout::Desktop, WINDOW).expect("a page");
    let mut head = Headless::new(page, WINDOW.width, WINDOW.height, 1.0).expect("a GPU");
    head.seek(&"1500ms".parse::<Moment>().expect("a moment"))
        .expect("a moment of the recording");
    let frame = head.frame();
    let flying = (0..frame.resolved.len() as u32)
        .find(|&index| {
            let element = &frame.resolved[index as usize];
            element.state.has(StateBits::MOVING) && uncovered(frame, index).is_some()
        })
        .expect("a card is in flight at 1500 ms");
    let inside = uncovered(frame, flying).expect("part of it is uncovered");
    let (x, y) = (
        (inside.x + inside.width / 2.0) as u32,
        (inside.y + inside.height / 2.0) as u32,
    );
    let pixels = head.pixels();
    let pixel = |x: u32, y: u32| {
        let at = ((y * WINDOW.width as u32 + x) * 4) as usize;
        [pixels[at], pixels[at + 1], pixels[at + 2]]
    };
    // The table is most of the picture, so its colour is the most common one.
    let mut counts: HashMap<[u8; 3], u32> = HashMap::new();
    for y in 0..WINDOW.height as u32 {
        for x in 0..WINDOW.width as u32 {
            *counts.entry(pixel(x, y)).or_default() += 1;
        }
    }
    let table = counts
        .into_iter()
        .max_by_key(|&(_, n)| n)
        .expect("pixels")
        .0;
    assert_ne!(
        pixel(x, y),
        table,
        "the card at ({x}, {y}) is the table's colour"
    );
}

#[test]
fn a_film_is_one_narrow_picture_with_a_line_for_each_frame() {
    let output = render(&[
        "film",
        "verbs-zones",
        "--from",
        "0ms",
        "--to",
        "3s",
        "--frames",
        "12",
    ]);
    let (path, (width, height)) = written(&output);
    assert!(width <= 2000 && height > 0, "{width}x{height}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let lines = stdout
        .lines()
        .filter(|line| line.starts_with("  #"))
        .count();
    assert_eq!(lines, 12, "{stdout}");
    let _ = std::fs::remove_file(path);
}

#[test]
fn two_runs_started_together_write_two_files() {
    let args = ["png", "verbs-zones", "--at", "1500ms"];
    let first = Command::new(env!("CARGO_BIN_EXE_b3d-render"))
        .args(args)
        .spawn_output();
    let second = render(&args);
    let (a, _) = written(&first());
    let (b, _) = written(&second);
    assert_ne!(a, b);
    let _ = std::fs::remove_file(a);
    let _ = std::fs::remove_file(b);
}

#[test]
fn a_film_too_long_for_its_grid_is_refused_with_what_would_work() {
    let output = render(&["film", "verbs-zones", "--frames", "30"]);
    assert_eq!(output.status.code(), Some(2));
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(
        message.contains("30 frames") && message.contains("--frames 24"),
        "{message}"
    );
}

/// Starts a command and returns a function that waits for its output.
trait SpawnOutput {
    fn spawn_output(&mut self) -> Box<dyn FnOnce() -> Output>;
}

impl SpawnOutput for Command {
    fn spawn_output(&mut self) -> Box<dyn FnOnce() -> Output> {
        let child = self
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .expect("the program starts");
        Box::new(move || child.wait_with_output().expect("it finishes"))
    }
}
