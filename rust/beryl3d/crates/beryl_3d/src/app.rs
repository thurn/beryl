//! The running app (docs/beryl-3d.md, "Hosts", "Threads" and "Driving the running app"): the
//! client on the frame thread, under the desktop host or the windowless one, fed by the
//! session thread or by a replayed recording, writing its frame log.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

use beryl_host::{Desktop, Feed, Inbox, Opened, Ticked, WindowMode, Windowless, WindowlessOptions};
use beryl_scene::frame::Wake;
use beryl_scene::input::{Input, Size};
use beryl_scene::{Client, Config};
use beryl_session::protocol::Status;
use beryl_session::script::Verb;

use crate::frame_log::{FrameLog, Run, Summary};
use crate::options::{Options, Window};
use crate::session::{self, Commands, Live};

/// The window the app opens, and the windowless host stands in for, in logical points: the
/// size `just b3d` draws a recording at unless told otherwise.
const SIZE: Size = Size {
    width: 1600.0,
    height: 900.0,
};
/// The rate of the windowless host's ticks: the floor's displays are 60 Hz.
const WINDOWLESS_HZ: f32 = 60.0;
/// The line the app prints when its first frame is shown, which `just b3d smoke` waits for.
pub const FIRST_SHOWN: &str = "beryl3d: first frame shown";

/// What a finished run left.
pub struct Finished {
    pub summary: Summary,
    pub run: Run,
    pub path: PathBuf,
}

/// Where the client's inputs come from.
enum Source {
    /// The session thread, which takes the client's commands.
    Live(Commands),
    /// A recording, replayed on the frame thread.
    #[cfg(feature = "tools")]
    Fixture(crate::fixture::Fixture),
}

/// What the app adds to the host's frame loop.
struct AppFeed {
    source: Option<Source>,
    /// The frame log, made when the host opens and read back after the run.
    log: Rc<RefCell<Option<Logged>>>,
    /// The run as known before the host opened; the host fills in the display and adapter.
    run: Run,
    /// Whether the latest view says the AI is searching.
    searching: bool,
    shown_first: bool,
}

/// The frame log, or why there is none.
type Logged = Result<FrameLog, String>;

#[cfg(feature = "tools")]
impl AppFeed {
    /// Gives the client an input from the feed itself, noting what the view says first.
    fn give(&mut self, client: &mut Client, input: Input) {
        self.input(&input);
        client.handle(input);
    }
}

impl Feed for AppFeed {
    fn opened(&mut self, opened: &Opened) {
        self.run.display_hz = opened.display_hz;
        self.run.adapter.clone_from(&opened.adapter);
        let log = FrameLog::create(&frames_dir(), &self.run)
            .map_err(|error| format!("the frame log could not be written: {error}"));
        *self.log.borrow_mut() = Some(log);
    }

    fn input(&mut self, input: &Input) {
        if let Input::Update(update) = input {
            self.searching = matches!(update.view.status, Status::Thinking);
        }
    }

    #[cfg_attr(not(feature = "tools"), expect(unused_variables))]
    fn before(&mut self, client: &mut Client, clock: u64) {
        #[cfg(feature = "tools")]
        if let Some(Source::Fixture(fixture)) = &mut self.source {
            for update in fixture.due(clock) {
                self.give(client, Input::Update(Box::new(update)));
            }
        }
    }

    fn after(&mut self, client: &mut Client, ticked: &Ticked) {
        while let Some(command) = client.next_command() {
            match &mut self.source {
                Some(Source::Live(commands)) => commands.send(command),
                #[cfg(feature = "tools")]
                Some(Source::Fixture(fixture)) => match fixture.answer(&command) {
                    Ok((reply, views)) => {
                        client.handle(Input::Reply(reply));
                        for update in views {
                            self.give(client, Input::Update(Box::new(update)));
                        }
                    }
                    Err(error) => eprintln!("beryl3d: {error}"),
                },
                None => {}
            }
        }
        #[cfg(feature = "tools")]
        if let Some(Source::Fixture(fixture)) = &mut self.source {
            fixture.settled(client);
        }
        let mut log = self.log.borrow_mut();
        let Some(Ok(log)) = log.as_mut() else {
            return;
        };
        while let Some(verb) = client.next_unbuilt() {
            log.unbuilt(verb.name());
        }
        // Verbs are listed only for a late frame, so a frame on time collects none.
        let late = ticked
            .since_last
            .is_some_and(|gap| gap as f64 > 1.5e9 / f64::from(self.run.display_hz.max(1.0)));
        let verbs: Vec<&str> = if late {
            client.playing_verbs().map(Verb::name).collect()
        } else {
            Vec::new()
        };
        log.frame(ticked, &verbs, self.searching);
        if !self.shown_first && ticked.shown.is_some() {
            self.shown_first = true;
            // One short line, once, which `just b3d smoke` waits for.
            println!("{FIRST_SHOWN} at {:.1} ms", ticked.tick as f64 / 1e6);
        }
    }

    fn wake(&self) -> Wake {
        match &self.source {
            #[cfg(feature = "tools")]
            Some(Source::Fixture(fixture)) => fixture.wake(),
            _ => Wake::Never,
        }
    }
}

/// Runs the app as `options` say, until its window closes or `--for` has passed.
///
/// # Errors
///
/// When an option's value names nothing, the host cannot start, or the frame log cannot be
/// written.
pub fn run(options: &Options) -> Result<Finished, String> {
    let pause_at = pause_at(options)?;
    let run = Run {
        checkout: checkout(),
        pid: std::process::id(),
        started_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |since| since.as_millis() as u64),
        window: options.window.word().to_owned(),
        display_hz: 0.0,
        adapter: String::new(),
        load: load(),
        fixture: options.fixture.clone(),
        pause_at_ms: pause_at.map(|at| at as f64 / 1e6),
    };
    let log = Rc::new(RefCell::new(None));
    let mut feed = AppFeed {
        source: None,
        log: Rc::clone(&log),
        run: run.clone(),
        searching: false,
        shown_first: false,
    };
    let client = Client::new(Config::LARGEST);
    let hosted = match options.window {
        Window::None => {
            let host = Windowless::new(WindowlessOptions {
                size: SIZE,
                scale: 1.0,
                hz: WINDOWLESS_HZ,
                pause_at,
                end_after: options.run_for,
            });
            feed.source = Some(source(options, host.inbox())?);
            host.run(client, Box::new(feed), |_| {})
        }
        window => {
            let mode = match window {
                Window::Background => WindowMode::Background,
                Window::Foreground => WindowMode::Foreground,
                _ => WindowMode::Fullscreen,
            };
            let host = Desktop::new(beryl_host::Options {
                mode,
                title: "Beryl 3D".to_owned(),
                size: (f64::from(SIZE.width), f64::from(SIZE.height)),
                pause_at,
                end_after: options.run_for,
            })?;
            feed.source = Some(source(options, host.inbox())?);
            host.run(client, Box::new(feed), |_| {})
        }
    };
    hosted?;
    let log = log
        .borrow_mut()
        .take()
        .ok_or("the host ended before it opened")??;
    let path = log.path().to_owned();
    let summary = log
        .finish()
        .map_err(|error| format!("the frame log could not be written: {error}"))?;
    let run = crate::frame_log::read(&path)?.run;
    Ok(Finished { summary, run, path })
}

/// The session thread, or with `--fixture` the recording.
fn source(options: &Options, inbox: Inbox) -> Result<Source, String> {
    #[cfg(feature = "tools")]
    if let Some(name) = &options.fixture {
        return crate::fixture::Fixture::new(name).map(Source::Fixture);
    }
    #[cfg(not(feature = "tools"))]
    let _ = options;
    let data = data_dir();
    Ok(Source::Live(session::spawn(
        {
            let inbox = inbox.clone();
            move || Live::new(data, inbox)
        },
        inbox,
    )))
}

/// Where `--pause-at` stops the clock, in nanoseconds.
fn pause_at(options: &Options) -> Result<Option<u64>, String> {
    let Some(text) = &options.pause_at else {
        return Ok(None);
    };
    #[cfg(feature = "tools")]
    return crate::fixture::moment(text, options.fixture.as_deref(), SIZE).map(Some);
    #[cfg(not(feature = "tools"))]
    Err(format!("--pause-at {text}: this build cannot pause"))
}

/// The checkout the app was built from: four directories above this crate's.
fn checkout() -> String {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = manifest.ancestors().nth(4).unwrap_or(manifest);
    root.canonicalize()
        .unwrap_or_else(|_| root.to_owned())
        .display()
        .to_string()
}

/// `$BERYL_CACHE/b3d/frames`, where frame logs go. The cache is `~/Library/Caches/beryl`, or
/// on Windows `%LOCALAPPDATA%\beryl`, unless `$BERYL_CACHE` names another.
fn frames_dir() -> PathBuf {
    let cache = std::env::var_os("BERYL_CACHE").map_or_else(
        || {
            if cfg!(windows) {
                PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("beryl")
            } else {
                home().join("Library").join("Caches").join("beryl")
            }
        },
        PathBuf::from,
    );
    cache.join("b3d").join("frames")
}

/// Where the app keeps its replays and ledger: its own folder beside the web app's, so that
/// neither resumes a game the other is playing. `~/Library/Application Support/beryl3d`, or on
/// Windows `%APPDATA%\beryl3d`.
fn data_dir() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|dir| PathBuf::from(dir).join("beryl3d"))
    } else {
        std::env::var_os("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("beryl3d")
        })
    }
}

fn home() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The machine's load averages, where the system reports them: `sysctl` on macOS, the proc
/// file on Linux. Read once, before the host starts.
fn load() -> Option<[f64; 3]> {
    let text = if cfg!(target_os = "macos") {
        let output = std::process::Command::new("sysctl")
            .args(["-n", "vm.loadavg"])
            .output()
            .ok()?;
        String::from_utf8(output.stdout).ok()?
    } else {
        std::fs::read_to_string("/proc/loadavg").ok()?
    };
    let numbers: Vec<f64> = text
        .split_whitespace()
        .filter_map(|word| word.parse().ok())
        .take(3)
        .collect();
    numbers.try_into().ok()
}
