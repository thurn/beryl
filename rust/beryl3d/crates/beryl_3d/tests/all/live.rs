//! The live socket: an app paused on the first frame of `pay-default` answers `tree` with
//! the table, `do` with the result of the action, and `png` with the path of a picture.

#![cfg(unix)]

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::UnixStream;
use std::process::{Child, Command, Stdio};

use beryl_3d::app::{FIRST_SHOWN, LIVE_SOCKET};

const CARD: &str = "card \"Staff of the Storyteller\"";

/// The app, stopped when dropped.
struct App(Child);

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Sends `request` to the socket at `path` and returns the first line of the answer and the
/// rest.
fn ask(path: &str, request: &str) -> (String, String) {
    let mut stream = UnixStream::connect(path).expect("the socket accepts");
    writeln!(stream, "{request}").expect("the request is sent");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    let (status, body) = answer.split_once('\n').unwrap_or((&answer, ""));
    (status.to_owned(), body.to_owned())
}

#[test]
fn a_running_app_answers_tree_do_and_png() {
    // Short, because a socket's path may not be much over 100 bytes, and a temporary folder
    // on macOS is already half that.
    let folder = std::path::PathBuf::from(format!("/tmp/b3d-live-{}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a folder for the socket and pictures");
    let mut child = Command::new(env!("CARGO_BIN_EXE_beryl3d"))
        .args(["--fixture", "pay-default", "--pause-at", "0ms"])
        .args(["--window", "none"])
        .env("TMPDIR", &folder)
        .stdout(Stdio::piped())
        .spawn()
        .expect("the app starts");
    let stdout = child.stdout.take().expect("the app's output is a pipe");
    let app = App(child);
    // Kept open to the end: the app dies when it prints to a closed pipe.
    let mut lines = BufReader::new(stdout).lines();
    let path = lines
        .by_ref()
        .map_while(Result::ok)
        .find_map(|line| {
            line.strip_prefix(&format!("{LIVE_SOCKET} "))
                .map(str::to_owned)
        })
        .expect("the app names its socket");
    assert!(
        path.starts_with(folder.to_str().expect("a UTF-8 path")),
        "{path}"
    );

    // The socket opens before the renderer is ready. The first frame shows the fixture's
    // initial board, even if the host misses ticks while starting up.
    assert!(
        lines
            .by_ref()
            .map_while(Result::ok)
            .any(|line| line.starts_with(FIRST_SHOWN)),
        "the app exited before showing its first frame"
    );
    let (status, tree) = ask(&path, "tree");
    assert_eq!(status, "ok", "{tree}");
    assert!(tree.contains(CARD), "{tree}");

    let (status, picture) = ask(&path, "png");
    assert_eq!(status, "ok", "{picture}");
    let size = std::fs::metadata(picture.trim())
        .expect("the picture was written")
        .len();
    assert!(size > 1000, "{picture} holds {size} bytes");
    let _ = std::fs::remove_file(picture.trim());

    let (status, body) = ask(&path, &format!("do hover {CARD}"));
    assert_eq!(status, "ok", "{body}");

    let (status, why) = ask(&path, "do press button \"No Such Button\"");
    assert_eq!(status, "error");
    assert!(why.contains("No Such Button"), "{why}");

    let (status, why) = ask(&path, "jump");
    assert_eq!(status, "error");
    assert!(why.contains("tree, png or do"), "{why}");
    drop(app);
    let _ = std::fs::remove_dir_all(&folder);
}
