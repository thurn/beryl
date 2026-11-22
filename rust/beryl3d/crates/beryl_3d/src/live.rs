//! Driving the running app (docs/beryl-3d.md, "Driving the running app"): a Unix socket at
//! `$TMPDIR/beryl3d-<process id>.sock` that `just b3d live` speaks to. Only a build with the
//! `tools` feature has it.
//!
//! A client connects, sends one line (`tree`, `png`, `do <action>` or `quit`) and reads the
//! answer to the end of the connection. The answer's first line is `ok` or `error`, and the
//! rest is the tree, the picture's path, or what went wrong. Connection threads queue requests
//! for the frame thread and write its replies back. The frame thread writes the reply to
//! `quit` itself so the connection is closed before the host can exit.

use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};

use app_client::Client;
use app_client::Input;
use beryl_host::{Drive, Inbox};
use beryl_render::Picture;
use kit_probe::agent::{TreeOptions, tree};

/// What a request is answered with: the text after `ok`, or the reason for `error`.
type Answer = Result<String, String>;

/// A request the server thread has read and the frame thread has not answered.
enum Request {
    Tree(Sender<Answer>),
    Png(Sender<Answer>),
    /// An action, parsed as a one-line drive script.
    Do(Drive, Sender<Answer>),
    /// Ends the run as closing the window does, so that the frame log gets its summary and a
    /// live game stays saved to be resumed.
    Quit(UnixStream),
}

/// The folder a socket is made in: `$TMPDIR`, or `/tmp`.
#[must_use]
pub fn socket_dir() -> PathBuf {
    std::env::var_os("TMPDIR").map_or_else(|| PathBuf::from("/tmp"), PathBuf::from)
}

/// The socket path of the process `pid` in `dir`.
#[must_use]
pub fn socket_path(dir: &Path, pid: u32) -> PathBuf {
    dir.join(format!("beryl3d-{pid}.sock"))
}

/// The server, and the requests the frame thread is working on.
pub struct Live {
    path: PathBuf,
    queue: Arc<Mutex<VecDeque<Request>>>,
    /// Where pictures are written: `live-<pid>-<n>.png`.
    out: PathBuf,
    pictures: u32,
    /// Scripts taken from the queue, to give to the frame loop (`Feed::scripts`).
    new_scripts: Vec<(u64, Drive)>,
    /// Each running script's id and the connection waiting for its end.
    scripts: Vec<(u64, Sender<Answer>)>,
    next_script: u64,
    trees: Vec<Sender<Answer>>,
    /// Pictures asked for and not yet drawn, then groups of askers waiting for one picture.
    pictures_asked: Vec<Sender<Answer>>,
    pictures_waiting: VecDeque<Vec<Sender<Answer>>>,
    /// Whether the frame thread has handled a `quit` request.
    quit: bool,
}

impl Live {
    /// Listens on the socket for `pid` in `dir`, on a thread of its own. Each request wakes the
    /// host through `inbox`.
    ///
    /// # Errors
    ///
    /// When the socket cannot be made.
    pub fn start(dir: &Path, pid: u32, out: PathBuf, inbox: Inbox) -> Result<Self, String> {
        let path = socket_path(dir, pid);
        // A socket left by a process that died with the same id.
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)
            .map_err(|error| format!("the live socket {}: {error}", path.display()))?;
        let queue = Arc::new(Mutex::new(VecDeque::new()));
        let shared = Arc::clone(&queue);
        std::thread::Builder::new()
            .name("live socket".to_owned())
            .spawn(move || {
                for stream in listener.incoming().flatten() {
                    let queue = Arc::clone(&shared);
                    let inbox = inbox.clone();
                    // A thread for each connection, so that a slow answer holds up no other.
                    let _ = std::thread::Builder::new()
                        .name("live connection".to_owned())
                        .spawn(move || serve(stream, &queue, &inbox));
                }
            })
            .map_err(|error| format!("the live socket's thread: {error}"))?;
        Ok(Self {
            path,
            queue,
            out,
            pictures: 0,
            new_scripts: Vec::new(),
            scripts: Vec::new(),
            next_script: 0,
            trees: Vec::new(),
            pictures_asked: Vec::new(),
            pictures_waiting: VecDeque::new(),
            quit: false,
        })
    }

    /// Whether the frame thread has a request to take up: it must draw a frame for it.
    #[must_use]
    pub fn has_work(&self) -> bool {
        !self.queue.lock().is_ok_and(|queue| queue.is_empty())
    }

    /// Moves what the server thread queued into the frame thread's own lists.
    fn take(&mut self) {
        let taken: Vec<Request> = self
            .queue
            .lock()
            .map(|mut queue| queue.drain(..).collect())
            .unwrap_or_default();
        for request in taken {
            match request {
                Request::Tree(reply) => self.trees.push(reply),
                Request::Png(reply) => self.pictures_asked.push(reply),
                Request::Do(script, reply) => {
                    self.next_script += 1;
                    self.new_scripts.push((self.next_script, script));
                    self.scripts.push((self.next_script, reply));
                }
                Request::Quit(stream) => {
                    // This is the only write to the connection, so the three bytes fit in its
                    // send buffer even if the client has yet to read. The host may exit as
                    // soon as it sees the flag, so write and close the socket first.
                    respond(stream, &Ok(String::new()));
                    self.quit = true;
                }
            }
        }
    }

    /// Whether a `quit` request asked the run to end.
    #[must_use]
    pub fn quit(&self) -> bool {
        self.quit
    }

    /// The scripts to start on this tick.
    pub fn scripts(&mut self) -> Vec<(u64, Drive)> {
        self.take();
        std::mem::take(&mut self.new_scripts)
    }

    /// The script `id` ended, or stopped on an error.
    pub fn script_done(&mut self, id: u64, result: Result<(), String>) {
        if let Some(at) = self.scripts.iter().position(|(found, _)| *found == id) {
            let (_, reply) = self.scripts.remove(at);
            let _ = reply.send(result.map(|()| String::new()));
        }
    }

    /// Whether a picture of the frame just drawn is wanted. The askers so far share it.
    pub fn picture_wanted(&mut self) -> bool {
        if self.pictures_asked.is_empty() {
            return false;
        }
        self.pictures_waiting
            .push_back(std::mem::take(&mut self.pictures_asked));
        true
    }

    /// The picture of an earlier frame is ready: writes it and answers those who asked, on a
    /// thread of its own, since encoding a window-sized PNG takes longer than a frame.
    pub fn picture(&mut self, picture: Picture) {
        let Some(askers) = self.pictures_waiting.pop_front() else {
            return;
        };
        self.pictures += 1;
        let out = self.out.clone();
        let path = out.join(format!("live-{}-{}.png", std::process::id(), self.pictures));
        let written = std::thread::Builder::new()
            .name("live picture".to_owned())
            .spawn(move || {
                let written = std::fs::create_dir_all(&out)
                    .map_err(|error| format!("{}: {error}", out.display()))
                    .and_then(|()| picture.write_png(&path))
                    .map(|()| path.display().to_string());
                for asker in askers {
                    let _ = asker.send(written.clone());
                }
            });
        if let Err(error) = written {
            eprintln!("beryl3d: the live picture's thread did not start: {error}");
        }
    }

    /// Answers the `tree` requests with the client's frame at `clock`, after a first line that
    /// gives the clock as `just b3d tree`'s first line does (`t=1500ms`), so that a driver can
    /// time how long a prompt has been open on the app's own clock.
    pub fn trees(&mut self, client: &mut Client, clock: u64) {
        if self.trees.is_empty() {
            return;
        }
        client.prepare(clock);
        let printed = tree(client.frame(clock), &TreeOptions::default())
            .map(|printed| format!("t={}ms\n{printed}", clock / 1_000_000))
            .map_err(|error| error.to_string());
        for asker in self.trees.drain(..) {
            let _ = asker.send(printed.clone());
        }
    }
}

impl Drop for Live {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Reads one request from `stream` and queues it. For commands other than `quit`, this
/// thread waits for the answer and writes it.
fn serve(stream: UnixStream, queue: &Mutex<VecDeque<Request>>, inbox: &Inbox) {
    let mut line = String::new();
    if BufReader::new(&stream).read_line(&mut line).is_err() {
        return;
    }
    let line = line.trim();
    if line == "quit" {
        // Hold the queue lock while waking the host so the next tick sees the quit request.
        if let Ok(mut queue) = queue.lock()
            && inbox.send(Input::Nudge)
        {
            queue.push_back(Request::Quit(stream));
            return;
        }
        return respond(stream, &Err("the app has ended".to_owned()));
    }
    let (reply, answer) = channel();
    let request = match parse(line, reply) {
        Ok(request) => request,
        Err(error) => return respond(stream, &Err(error)),
    };
    if let Ok(mut queue) = queue.lock() {
        queue.push_back(request);
    }
    if !inbox.send(Input::Nudge) {
        return respond(stream, &Err("the app has ended".to_owned()));
    }
    let answer = answer
        .recv()
        .unwrap_or_else(|_| Err("the app ended before it answered".to_owned()));
    respond(stream, &answer);
}

fn parse(line: &str, reply: Sender<Answer>) -> Result<Request, String> {
    match line.split_once(' ').unwrap_or((line, "")) {
        ("tree", "") => Ok(Request::Tree(reply)),
        ("png", "") => Ok(Request::Png(reply)),
        ("do", action) if !action.trim().is_empty() => {
            // An action that gives no time starts when the script does.
            Drive::parse("live", action.trim()).map(|script| Request::Do(script, reply))
        }
        _ => Err(format!(
            "'{line}' is not a request: send tree, png, do <action> or quit"
        )),
    }
}

fn respond(mut stream: UnixStream, answer: &Answer) {
    let text = match answer {
        Ok(body) => format!("ok\n{body}"),
        Err(why) => format!("error\n{why}"),
    };
    let _ = stream.write_all(text.as_bytes());
}
