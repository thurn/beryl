//! The live socket: an app paused on the first frame of `pay-default` answers `tree` with
//! the table, `do` with the result of the action, and `png` with the path of a picture.

#![cfg(any(unix, windows))]

use std::io::{BufRead, BufReader, Read, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use beryl_client::app::{FIRST_SHOWN, LIVE_SOCKET};
#[cfg(windows)]
use socket2::{Domain, SockAddr, Socket as UnixStream, Type};

const CARD: &str = "card \"Staff of the Storyteller\"";

/// The app, stopped when dropped.
pub(super) struct App(pub(super) Child);

impl Drop for App {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[cfg(unix)]
fn connect(path: &Path) -> std::io::Result<UnixStream> {
    UnixStream::connect(path)
}

#[cfg(windows)]
fn connect(path: &Path) -> std::io::Result<UnixStream> {
    let stream = UnixStream::new(Domain::UNIX, Type::STREAM, None)?;
    stream.connect(&SockAddr::unix(path)?)?;
    Ok(stream)
}

/// Sends `request` to the socket at `path` and reads the answer through the end of the
/// connection.
pub(super) fn exchange(path: &str, request: &str) -> String {
    let mut stream = connect(Path::new(path)).expect("the socket accepts");
    writeln!(stream, "{request}").expect("the request is sent");
    let mut answer = String::new();
    stream
        .read_to_string(&mut answer)
        .expect("the answer is read");
    answer
}

/// Returns the first line of the answer and the rest.
fn ask(path: &str, request: &str) -> (String, String) {
    let answer = exchange(path, request);
    let (status, body) = answer.split_once('\n').unwrap_or((&answer, ""));
    (status.to_owned(), body.to_owned())
}

#[test]
fn a_running_app_answers_tree_do_and_png() {
    // Short, because a socket's path may not be much over 100 bytes, and a temporary folder
    // on macOS is already half that.
    let temp = if cfg!(windows) {
        std::env::temp_dir()
    } else {
        PathBuf::from("/tmp")
    };
    let folder = temp.join(format!("client-live-{}", std::process::id()));
    std::fs::create_dir_all(&folder).expect("a folder for the socket and pictures");
    let mut child = Command::new(env!("CARGO_BIN_EXE_beryl"))
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
    assert_eq!(Path::new(&path).parent(), Some(folder.as_path()));

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
    let clock = tree.lines().next().unwrap_or_default();
    assert!(clock.starts_with("t=") && clock.ends_with("ms"), "{tree}");

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
    assert!(why.contains("tree, png, do <action> or quit"), "{why}");

    // Quit must send the complete reply before ending the run and removing the socket.
    assert_eq!(exchange(&path, "quit"), "ok\n");
    let mut app = app;
    let ended = app.0.wait().expect("the app ends after quit");
    assert!(ended.success(), "{ended}");
    assert!(
        !std::path::Path::new(&path).exists(),
        "the socket is removed"
    );
    drop(app);
    let _ = std::fs::remove_dir_all(&folder);
}
