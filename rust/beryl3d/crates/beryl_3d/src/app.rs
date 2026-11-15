//! The running app (docs/beryl-3d.md, "Hosts", "Threads" and "Driving the running app"): the
//! client on the frame thread, under the desktop host or the windowless one, fed by the
//! session thread or by a replayed recording, writing its frame log.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(not(any(target_os = "ios", target_os = "android")))]
use beryl_host::{Desktop, WindowMode};
use beryl_host::{Feed, Inbox, Opened, Ticked, Windowless, WindowlessOptions};
use beryl_scene::frame::Wake;
use beryl_scene::input::{Input, Size};
use beryl_scene::{Client, Config};
use beryl_session::protocol::Status;
use beryl_session::script::Verb;

use crate::fixture::Fixture;
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
/// The line the app prints when a replayed recording's last script has ended.
pub const FIXTURE_ENDED: &str = "beryl3d: fixture ended";

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
    Fixture(Fixture),
}

/// What the app adds to the host's frame loop.
struct AppFeed {
    source: Option<Source>,
    /// The frame log, made when the host opens and read back after the run.
    log: Rc<RefCell<Option<Logged>>>,
    /// The folder the frame log is written in.
    log_dir: PathBuf,
    /// When the frame log is finished.
    finish: Finish,
    /// Whether a replayed recording has ended.
    ended: bool,
    /// The run as known before the host opened; the host fills in the display and adapter.
    run: Run,
    /// Whether the latest view says the AI is searching.
    searching: bool,
    shown_first: bool,
}

/// When the frame log is finished and its summary written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Finish {
    /// When the run ends: on the desktop.
    AtExit,
    /// When a replayed recording ends: on a phone, whose app never exits by itself.
    AtFixtureEnd,
}

/// The frame log, or why there is none.
type Logged = Result<FrameLog, String>;

impl AppFeed {
    /// Gives the client an input from the feed itself, noting what the view says first.
    fn give(&mut self, client: &mut Client, input: Input) {
        self.input(&input);
        client.handle(input);
    }

    /// Finishes the frame log and prints its summary: done once, when a phone's replay ends.
    /// Finishing waits for the log's writer, so it happens on a thread of its own rather than
    /// on the frame thread.
    fn finish_log(&mut self) {
        let Some(Ok(log)) = self.log.borrow_mut().take() else {
            return;
        };
        let run = self.run.clone();
        let finishing = std::thread::Builder::new()
            .name("frame log summary".to_owned())
            .spawn(move || {
                let path = log.path().to_owned();
                match log.finish() {
                    Ok(summary) => {
                        println!("{}", summary.print(&run));
                        println!("frame log: {}", path.display());
                    }
                    Err(error) => {
                        eprintln!("beryl3d: the frame log could not be written: {error}");
                    }
                }
            });
        if let Err(error) = finishing {
            eprintln!("beryl3d: the frame log could not be finished: {error}");
        }
    }
}

impl Feed for AppFeed {
    fn opened(&mut self, opened: &Opened) {
        self.run.display_hz = opened.display_hz;
        self.run.adapter.clone_from(&opened.adapter);
        let log = FrameLog::create(&self.log_dir, &self.run)
            .map_err(|error| format!("the frame log could not be written: {error}"));
        *self.log.borrow_mut() = Some(log);
    }

    fn input(&mut self, input: &Input) {
        if let Input::Update(update) = input {
            self.searching = matches!(update.view.status, Status::Thinking);
        }
    }

    fn before(&mut self, client: &mut Client, clock: u64) {
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
        if let Some(Source::Fixture(fixture)) = &mut self.source {
            fixture.settled(client);
            if !self.ended && fixture.ended(client) {
                self.ended = true;
                println!("{FIXTURE_ENDED} at {:.1} ms", ticked.clock as f64 / 1e6);
                if self.finish == Finish::AtFixtureEnd {
                    self.finish_log();
                }
            }
        }
        let mut log = self.log.borrow_mut();
        let Some(Ok(log)) = log.as_mut() else {
            return;
        };
        while let Some(verb) = client.next_unbuilt() {
            log.unbuilt(verb.name());
        }
        // Verbs are listed only for a late frame, so a frame on time collects none.
        let late = FrameLog::is_late(ticked);
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
            Some(Source::Fixture(fixture)) => fixture.wake(),
            _ => Wake::Never,
        }
    }
}

/// The feed of a phone's app (`beryl_3d`'s C functions): it replays `fixture`, the recording
/// `name`, writes its frame log in `log_dir`, and finishes the log when the replay ends.
pub(crate) fn phone_feed(name: &str, fixture: Fixture, log_dir: PathBuf) -> Box<dyn Feed> {
    Box::new(AppFeed {
        source: Some(Source::Fixture(fixture)),
        log: Rc::new(RefCell::new(None)),
        log_dir,
        finish: Finish::AtFixtureEnd,
        ended: false,
        run: Run {
            checkout: checkout(),
            pid: std::process::id(),
            started_ms: now_ms(),
            window: "phone".to_owned(),
            display_hz: 0.0,
            adapter: String::new(),
            load: None,
            fixture: Some(name.to_owned()),
            pause_at_ms: None,
            drive: None,
            load_threads: None,
            slow_gpu_ms: None,
        },
        searching: false,
        shown_first: false,
    })
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
        started_ms: now_ms(),
        window: options.window.word().to_owned(),
        display_hz: 0.0,
        adapter: String::new(),
        load: load(),
        fixture: options.fixture.clone(),
        pause_at_ms: pause_at.map(|at| at as f64 / 1e6),
        drive: options.drive.clone(),
        load_threads: options
            .load
            .map(|(threads, class)| format!("{threads}:{}", class.word())),
        slow_gpu_ms: options.slow_gpu.map(|slow| slow.as_secs_f64() * 1e3),
    };
    // Started before the host, from the main thread, and stopped when the run ends.
    let _load = options
        .load
        .map(|(threads, class)| crate::load::start(threads, class));
    #[cfg(feature = "tools")]
    let drive = options.drive.as_deref().map(drive).transpose()?;
    let slow_gpu = options.slow_gpu.unwrap_or_default();
    let log = Rc::new(RefCell::new(None));
    let mut feed = AppFeed {
        source: None,
        log: Rc::clone(&log),
        log_dir: frames_dir(),
        finish: Finish::AtExit,
        ended: false,
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
                slow_gpu,
            });
            #[cfg(feature = "tools")]
            let host = host.driven(drive);
            feed.source = Some(source(options, host.inbox())?);
            host.run(client, Box::new(feed), |_| {})
        }
        // A phone's app is its Swift or Kotlin host, which calls `crate::ffi`.
        #[cfg(any(target_os = "ios", target_os = "android"))]
        _ => Err("a phone has no desktop window: its host calls beryl_3d's C functions".to_owned()),
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
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
                slow_gpu,
            })?;
            #[cfg(feature = "tools")]
            let host = host.driven(drive);
            feed.source = Some(source(options, host.inbox())?);
            host.run(client, Box::new(feed), |_| {})
        }
    };
    let log = log.borrow_mut().take();
    if let Err(error) = hosted {
        // The log still gets its summary, so that the frames before the error can be read.
        if let Some(Ok(log)) = log {
            let _ = log.finish();
        }
        return Err(error);
    }
    let log = log.ok_or("the host ended before it opened")??;
    let path = log.path().to_owned();
    let summary = log
        .finish()
        .map_err(|error| format!("the frame log could not be written: {error}"))?;
    let run = crate::frame_log::read(&path)?.run;
    Ok(Finished { summary, run, path })
}

/// The session thread, or with `--fixture` the recording.
// Only the recording can fail to start, and only a build with `tools` has one.
#[cfg_attr(not(feature = "tools"), expect(clippy::unnecessary_wraps))]
fn source(options: &Options, inbox: Inbox) -> Result<Source, String> {
    #[cfg(feature = "tools")]
    if let Some(name) = &options.fixture {
        return Fixture::new(name).map(Source::Fixture);
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

/// The drive script `name` names: a file in `rust/beryl3d/drive` named `<name>.drive`, or a
/// path.
#[cfg(feature = "tools")]
fn drive(name: &str) -> Result<beryl_host::Drive, String> {
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../drive");
    let path = if name.contains(['/', '\\']) {
        PathBuf::from(name)
    } else {
        scripts.join(format!("{name}.drive"))
    };
    if !path.exists() && !name.contains(['/', '\\']) {
        let known: Vec<String> = std::fs::read_dir(&scripts)
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|entry| {
                let path = entry.path();
                if path.extension()? != "drive" {
                    return None;
                }
                Some(path.file_stem()?.to_string_lossy().into_owned())
            })
            .collect();
        return Err(format!(
            "--drive {name}: no such script; the scripts are {}",
            known.join(", ")
        ));
    }
    beryl_host::Drive::load(&path)
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

/// The time, in milliseconds since the Unix epoch.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_millis() as u64)
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
