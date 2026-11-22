//! The running app (docs/beryl-3d.md, "Hosts", "Threads" and "Driving the running app"): the
//! client on the frame thread, under the desktop host or the windowless one, fed by the
//! session thread or by a replayed recording, writing its frame log.

use kit_ui::input::Input as DeviceInput;
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use app_client::Input;
use app_client::command::Request;
use app_client::{Client, Config};
#[cfg(not(any(target_os = "ios", target_os = "android")))]
use beryl_host::{Desktop, WindowMode};
use beryl_host::{Feed, Inbox, Opened, Ticked, Windowless, WindowlessOptions};
#[cfg(feature = "tools")]
use beryl_session::protocol::Side;
use beryl_session::protocol::Status;
use beryl_session::script::Verb;
use kit_frame::frame::Wake;
use kit_ui::input::{Lifecycle, Size};

use crate::audio::{Audio, Output};
use crate::bugs::Bugs;
use crate::fixture::Fixture;
use crate::frame_log::{FrameLog, Run, Summary};
use crate::options::{Options, Window};
#[cfg(all(feature = "tools", unix))]
use crate::places::out_dir;
use crate::places::{checkout, frames_dir, now_ms};
use crate::session::{self, Commands, Live};
use crate::settings::{self, Saver};

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
/// The line the app prints when a view first says the game is over.
pub const GAME_OVER: &str = "beryl3d: game over";
/// The line the app prints, followed by the path, when its live socket is listening.
pub const LIVE_SOCKET: &str = "beryl3d: live socket";

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
    /// A recording, replayed on the frame thread; boxed, being several times the size of
    /// `Live`.
    Fixture(Box<Fixture>),
}

/// What the app adds to the host's frame loop.
#[expect(
    clippy::struct_excessive_bools,
    reason = "each flag is a separate fact about the run"
)]
struct AppFeed {
    source: Option<Source>,
    /// The frame log, made when the host opens and read back after the run.
    log: Rc<RefCell<Option<Logged>>>,
    /// The folder the frame log is written in.
    log_dir: PathBuf,
    /// When the frame log is summarized.
    finish: Finish,
    /// Whether the source has ended: a replayed recording has played to its end, or a view of
    /// the live game has said the game is over.
    ended: bool,
    /// Whether the player asked to quit (`Request::Quit`), which ends the run.
    quit: bool,
    /// The run as known before the host opened; the host fills in the display and adapter.
    run: Run,
    /// Whether the latest view says the AI is searching.
    searching: bool,
    shown_first: bool,
    /// The audio thread, which plays each frame's sounds and the music the client asks for.
    audio: Option<Audio>,
    /// Writes the settings the client asks to save, in a live game.
    saver: Option<Saver>,
    /// The bug report's picture, tree and views, and where reports go.
    bugs: Bugs,
    /// The host's clock at the latest frame, in nanoseconds.
    clock: u64,
    /// How the tier was chosen, which the host's setup writes before it opens and `opened`
    /// moves into the frame log.
    tier: crate::tier::Pending,
    /// The socket `just b3d live` speaks to, in a build that has one.
    #[cfg(all(feature = "tools", unix))]
    live: Option<crate::live::Live>,
}

/// When the frame log is finished and its summary written.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Finish {
    /// When the run ends: on the desktop.
    AtExit,
    /// When a replayed recording ends: on a phone, whose app never exits by itself.
    AtFixtureEnd,
    /// When the run ends, which is when the game is over: with `--agent`, which plays a whole
    /// game with nobody at the table.
    GameOver,
    /// Never, since a phone's live game never exits by itself: instead its log is summarized
    /// each time the app is suspended and when the game ends, so that a run reading the log
    /// from outside (`just b3d phone-search`) finds the counts so far.
    AtSuspend,
}

/// The frame log, or why there is none.
type Logged = Result<FrameLog, String>;

impl AppFeed {
    /// Connects the feed to the host's inbox: starts the session thread or the replay, and the
    /// live socket in a build that has one.
    fn attach(&mut self, options: &Options, inbox: Inbox) -> Result<(), String> {
        self.bugs.attach(inbox.clone());
        #[cfg(all(feature = "tools", unix))]
        {
            self.live = start_live(inbox.clone());
        }
        self.source = Some(source(options, inbox)?);
        Ok(())
    }

    /// Replays the recording `name` from its start as `seat` sees it, in place of the game or
    /// the recording that fed the client until now: the testing screen's press on a recording.
    /// A recording that cannot be had leaves the source as it was, and says why.
    #[cfg(feature = "tools")]
    fn replay(&mut self, name: &str, seat: Side) {
        match Fixture::new(name, seat) {
            Ok(fixture) => {
                self.source = Some(Source::Fixture(Box::new(fixture)));
                self.ended = false;
            }
            Err(error) => eprintln!("beryl3d: {error}"),
        }
    }

    /// Gives the client an input from the feed itself, noting what the view says first.
    fn give(&mut self, client: &mut Client, input: Input) {
        self.input(&input);
        client.handle(input);
    }
}

impl Drop for AppFeed {
    /// A phone's app, which nothing else holds the log of, finishes its log when it ends, and
    /// waits until it is written. The desktop's `run` holds the log too, and finishes it itself.
    fn drop(&mut self) {
        if Rc::strong_count(&self.log) > 1 {
            return;
        }
        if let Some(Ok(log)) = self.log.borrow_mut().take()
            && let Err(error) = log.finish()
        {
            eprintln!("beryl3d: the frame log could not be written: {error}");
        }
    }
}

impl Feed for AppFeed {
    fn opened(&mut self, opened: &Opened) {
        self.run.display_hz = opened.display_hz;
        self.run.adapter.clone_from(&opened.adapter);
        let log = FrameLog::create(&self.log_dir, &self.run)
            .map_err(|error| format!("the frame log could not be written: {error}"));
        if let (Ok(log), Some(tiered)) = (&log, self.tier.borrow_mut().take()) {
            log.tier(tiered);
        }
        *self.log.borrow_mut() = Some(log);
    }

    fn input(&mut self, input: &Input) {
        if let Input::DataError(error) = input {
            eprintln!("beryl3d: data reload failed: {error}");
        }
        if let Some(Ok(log)) = self.log.borrow_mut().as_mut() {
            log.input(input);
        }
        self.bugs.seen(input);
        let mut summarize = false;
        if let Input::Update(update) = input {
            self.searching = matches!(update.view.status, Status::Thinking);
            // A replayed recording ends when its last script has played, not at its game's end.
            let live = matches!(self.source, Some(Source::Live(_)));
            if live && !self.ended && matches!(update.view.status, Status::Over { .. }) {
                self.ended = true;
                summarize = self.finish == Finish::AtSuspend;
                println!("{GAME_OVER}");
            }
        }
        if let Input::Device(DeviceInput::Lifecycle(Lifecycle::Suspended)) = input {
            summarize = self.finish == Finish::AtSuspend;
        }
        if summarize && let Some(Ok(log)) = self.log.borrow_mut().as_mut() {
            log.summarize();
        }
    }

    fn before(&mut self, client: &mut Client, clock: u64) {
        self.clock = clock;
        if let Some(Source::Fixture(fixture)) = &mut self.source {
            if let Some(cards) = fixture.cards() {
                client.handle(Input::Cards(Box::new(cards)));
            }
            for update in fixture.due(clock) {
                self.give(client, Input::Update(Box::new(update)));
            }
        }
    }

    fn after(&mut self, client: &mut Client, ticked: &Ticked) {
        self.bugs.after(client);
        for sound in client.sounds() {
            if let Some(audio) = &self.audio {
                audio.play(*sound);
            }
            if let Some(Ok(log)) = self.log.borrow_mut().as_mut() {
                log.sound(sound.id);
            }
        }
        #[cfg(all(feature = "tools", unix))]
        if let Some(live) = &mut self.live {
            live.trees(client, ticked.clock);
        }
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
        let mut summarize = false;
        if let Some(Source::Fixture(fixture)) = &mut self.source {
            fixture.settled(client);
            if !self.ended && fixture.ended(client) {
                self.ended = true;
                fixture.finished_at(ticked.clock);
                println!("{FIXTURE_ENDED} at {:.1} ms", ticked.clock as f64 / 1e6);
                summarize = self.finish == Finish::AtFixtureEnd;
            }
        }
        let mut log = self.log.borrow_mut();
        let Some(Ok(log)) = log.as_mut() else {
            return;
        };
        while let Some(verb) = client.next_unbuilt() {
            log.unbuilt(verb.name());
        }
        log.unplaced(client.unplaced());
        log.assets(
            client.last_frame().faceless_cards(),
            client.missing_glyphs(),
        );
        // Verbs are listed only for a late frame, so a frame on time collects none.
        let late = FrameLog::is_late(ticked);
        let verbs: Vec<&str> = if late {
            client.playing_verbs().map(Verb::name).collect()
        } else {
            Vec::new()
        };
        log.frame(ticked, &verbs, self.searching);
        if summarize {
            log.summarize();
        }
        if !self.shown_first && ticked.shown.is_some() {
            self.shown_first = true;
            // One short line, once, which `just b3d smoke` waits for.
            println!("{FIRST_SHOWN} at {:.1} ms", ticked.tick as f64 / 1e6);
        }
    }

    fn request(&mut self, request: &Request) {
        match request {
            Request::Music(music) => {
                if let Some(audio) = &self.audio {
                    audio.music(*music);
                }
            }
            Request::Audio {
                music,
                effects,
                muted,
            } => {
                if let Some(audio) = &self.audio {
                    audio.levels(*music, *effects, *muted);
                }
            }
            Request::Quit => self.quit = true,
            Request::SaveSettings(settings) => {
                if let Some(saver) = &self.saver {
                    saver.save(settings.clone());
                }
            }
            #[cfg(feature = "tools")]
            Request::Replay { name, seat } => self.replay(name, *seat),
            Request::FreezeFrame => {
                let replayed = match &self.source {
                    Some(Source::Fixture(fixture)) => Some(fixture.replayed_at(self.clock)),
                    _ => None,
                };
                self.bugs.freeze(replayed);
            }
            Request::SaveBug { .. } => {
                let log = self.log.borrow();
                let frame_log = log.as_ref().and_then(|log| log.as_ref().ok());
                let live = match &self.source {
                    Some(Source::Live(commands)) => Some(commands),
                    _ => None,
                };
                self.bugs.save(request, frame_log.map(FrameLog::path), live);
            }
            #[cfg(not(any(target_os = "ios", target_os = "android")))]
            Request::ExportBugs => self.bugs.export(),
            _ => {}
        }
    }

    fn wake(&self) -> Wake {
        #[cfg(all(feature = "tools", unix))]
        if self.live.as_ref().is_some_and(crate::live::Live::has_work) {
            return Wake::Now;
        }
        match &self.source {
            Some(Source::Fixture(fixture)) => fixture.wake(),
            _ => Wake::Never,
        }
    }

    fn capture_wanted(&mut self) -> bool {
        #[cfg(all(feature = "tools", unix))]
        let live = (self.live.as_mut()).is_some_and(crate::live::Live::picture_wanted);
        #[cfg(not(all(feature = "tools", unix)))]
        let live = false;
        self.bugs.capture_wanted(live)
    }

    fn captured(&mut self, picture: beryl_render::Picture) {
        let picture = self.bugs.captured(picture);
        #[cfg(all(feature = "tools", unix))]
        if let (Some(live), Some(picture)) = (&mut self.live, picture) {
            live.picture(picture);
        }
        #[cfg(not(all(feature = "tools", unix)))]
        drop(picture);
    }

    #[cfg(all(feature = "tools", unix))]
    fn scripts(&mut self) -> Vec<(u64, beryl_host::Drive)> {
        self.live
            .as_mut()
            .map(crate::live::Live::scripts)
            .unwrap_or_default()
    }

    #[cfg(all(feature = "tools", unix))]
    fn script_done(&mut self, id: u64, result: Result<(), String>) {
        if let Some(live) = &mut self.live {
            live.script_done(id, result);
        }
    }

    fn ended(&self) -> bool {
        #[cfg(all(feature = "tools", unix))]
        if self.live.as_ref().is_some_and(crate::live::Live::quit) {
            return true;
        }
        self.quit || (self.finish == Finish::GameOver && self.ended)
    }
}

/// The feed of a phone's app (`beryl_3d`'s C functions): it replays `fixture`, the recording
/// `name`, writes its frame log in `log_dir`, and summarizes it when the replay ends. The
/// log counts frames against `display_hz`, the display's highest rate, where the host's rate
/// rule starts.
pub(crate) fn phone_feed(
    name: &str,
    fixture: Fixture,
    log_dir: PathBuf,
    display_hz: f32,
) -> Box<dyn Feed> {
    phone_app(
        Source::Fixture(Box::new(fixture)),
        Some(name.to_owned()),
        log_dir,
        display_hz,
        (Finish::AtFixtureEnd, Bugs::new(beryl_session::bugs::dir())),
    )
}

/// The feed of a phone's app that plays a live game: the session thread keeps its replays and
/// its ledger of games in `data_dir`, as the web app keeps them in its data folder on a phone,
/// where the frame log is written too, and sends its views to `inbox`. With `stand_in`, that
/// agent answers the person's decisions in a fresh game. The log names `display_hz`, as
/// `phone_feed`'s does.
pub(crate) fn phone_live_feed(
    data_dir: PathBuf,
    stand_in: Option<String>,
    display_hz: f32,
    inbox: Inbox,
) -> Box<dyn Feed> {
    let dirs = beryl_shell::Dirs::new(
        &data_dir,
        data_dir.join("logs"),
        beryl_shell::Profile::current(),
    );
    let mut bugs = Bugs::new(dirs.bugs.clone());
    bugs.attach(inbox.clone());
    let commands = session::spawn(
        beryl_host::Class::Default,
        {
            let inbox = inbox.clone();
            move || {
                let live = Live::new(Some(dirs), inbox);
                match stand_in {
                    Some(name) => live.with_stand_in(&name),
                    None => live,
                }
            }
        },
        inbox,
    );
    phone_app(
        Source::Live(commands),
        None,
        data_dir,
        display_hz,
        (Finish::AtSuspend, bugs),
    )
}

/// A phone's app feed of `source`, writing its frame log in `log_dir` against `display_hz`,
/// finishing it as `finish` says and writing bug reports with `bugs`.
fn phone_app(
    source: Source,
    fixture: Option<String>,
    log_dir: PathBuf,
    display_hz: f32,
    (finish, bugs): (Finish, Bugs),
) -> Box<dyn Feed> {
    Box::new(AppFeed {
        source: Some(source),
        log: Rc::new(RefCell::new(None)),
        log_dir,
        finish,
        ended: false,
        quit: false,
        run: Run {
            checkout: checkout(),
            pid: std::process::id(),
            started_ms: now_ms(),
            window: "phone".to_owned(),
            display_hz,
            adapter: String::new(),
            load: None,
            fixture,
            pause_at_ms: None,
            drive: None,
            load_threads: None,
            slow_gpu_ms: None,
            game: None,
        },
        searching: false,
        shown_first: false,
        audio: None,
        saver: None,
        bugs,
        clock: 0,
        tier: Rc::default(),
        #[cfg(all(feature = "tools", unix))]
        live: None,
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
    let run = run_of(options, pause_at);
    // Started before the host, from the main thread, and stopped when the run ends.
    let _load = options
        .load
        .map(|(threads, class)| crate::load::start(threads, class));
    #[cfg(feature = "tools")]
    let drive = options
        .drive
        .as_deref()
        .map(crate::options::drive_script)
        .transpose()?;
    let slow_gpu = options.slow_gpu.unwrap_or_default();
    let (settings, saver) = player_settings(options);
    let data = crate::data::embedded()?;
    #[cfg(feature = "tools")]
    let (data, reload) = crate::data::initial(data, options.watch);
    let (setup, tier) = crate::tier::setup(settings.as_ref(), options.probe, Arc::clone(&data));
    let audio = start_audio(options.window, &settings.clone().unwrap_or_default());
    let log = Rc::new(RefCell::new(None));
    let mut feed = AppFeed {
        source: None,
        log: Rc::clone(&log),
        log_dir: frames_dir(),
        finish: if options.agent.is_some() {
            Finish::GameOver
        } else {
            Finish::AtExit
        },
        ended: false,
        quit: false,
        run: run.clone(),
        searching: false,
        shown_first: false,
        audio: Some(audio),
        saver,
        bugs: Bugs::desktop(),
        clock: 0,
        tier,
        #[cfg(all(feature = "tools", unix))]
        live: None,
    };
    let mut client = Client::new(data, Config::LARGEST);
    if let Some(settings) = settings {
        client.handle(Input::Settings(settings));
    }
    // A tools build lists the recordings, so its start screen offers the testing screen.
    #[cfg(feature = "tools")]
    match beryl_inspect::recordings::listed() {
        Ok(names) => client.handle(Input::Recordings(names)),
        Err(error) => eprintln!("beryl3d: the testing screen has no recordings: {error}"),
    }
    let hosted = match options.window {
        Window::None => {
            let host = Windowless::new(WindowlessOptions {
                size: SIZE,
                scale: f32::from(options.scale),
                hz: WINDOWLESS_HZ,
                pause_at,
                end_after: options.run_for,
                slow_gpu,
            });
            #[cfg(feature = "tools")]
            let host = host.driven(drive);
            feed.attach(options, host.inbox())?;
            #[cfg(feature = "tools")]
            let _watch = crate::data::start_watch(reload, host.inbox())?;
            host.run(client, Box::new(feed), setup)
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
            feed.attach(options, host.inbox())?;
            #[cfg(feature = "tools")]
            let _watch = crate::data::start_watch(reload, host.inbox())?;
            host.run(client, Box::new(feed), setup)
        }
    };
    let log = log.borrow_mut().take();
    finish_run(log, hosted)
}

/// Finishes the frame log after the host exits, keeping the host's error if it failed.
fn finish_run(log: Option<Logged>, hosted: Result<(), String>) -> Result<Finished, String> {
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

/// The session thread, or with `--fixture` the recording. With `--agent`, the session starts a
/// fresh game that the agent plays for the viewer, and keeps no replay or ledger, so that
/// neither app has an unfinished game of it to resume. With `--replay`, it resumes that file's
/// game and likewise keeps none.
// Only the recording can fail to start, and only a build with `tools` has one.
#[cfg_attr(not(feature = "tools"), expect(clippy::unnecessary_wraps))]
fn source(options: &Options, inbox: Inbox) -> Result<Source, String> {
    #[cfg(feature = "tools")]
    if let Some(name) = &options.fixture {
        return Fixture::new(name, Side::You).map(|fixture| Source::Fixture(Box::new(fixture)));
    }
    #[cfg(not(feature = "tools"))]
    let _ = options;
    let resume = options.replay.clone();
    // A resumed replay, like a game an agent plays, keeps no replay or ledger.
    let (data, played_by) = match &options.agent {
        Some(agent) => (
            None,
            Some((agent.clone(), options.opponent.clone(), options.seed)),
        ),
        None if resume.is_some() => (None, None),
        None => (data_dirs(), None),
    };
    let offer = options.offer.clone();
    Ok(Source::Live(session::spawn(
        options.search_class,
        {
            let inbox = inbox.clone();
            move || {
                let mut live = Live::new(data, inbox).offering(&offer);
                if let Some(path) = resume {
                    live = live.resuming(path);
                }
                match played_by {
                    Some((agent, opponent, seed)) => live.played_by(&agent, &opponent, seed),
                    None => live,
                }
            }
        },
        inbox,
    )))
}

/// What the frame log says of a run `options` describe, stopped at `pause_at`.
fn run_of(options: &Options, pause_at: Option<u64>) -> Run {
    Run {
        checkout: checkout(),
        pid: std::process::id(),
        started_ms: now_ms(),
        window: options.window.word().to_owned(),
        display_hz: 0.0,
        adapter: String::new(),
        load: crate::load::averages(),
        fixture: options.fixture.clone(),
        pause_at_ms: pause_at.map(|at| at as f64 / 1e6),
        drive: options.drive.clone(),
        load_threads: options
            .load
            .map(|(threads, class)| format!("{threads}:{}", class.word())),
        slow_gpu_ms: options.slow_gpu.map(|slow| slow.as_secs_f64() * 1e3),
        game: options.agent.as_ref().map(|agent| {
            format!(
                "{agent} against {}, seed {}, search class {}",
                options.opponent,
                options.seed,
                options.search_class.word()
            )
        }),
    }
}

/// The audio thread, at the levels `settings` give. A run with no window plays no sound, so
/// that agents' and checks' runs are silent.
fn start_audio(window: Window, settings: &app_client::settings::Settings) -> Audio {
    let audio = Audio::start(if window == Window::None {
        Output::Silent
    } else {
        Output::Device
    });
    audio.levels(settings.music, settings.effects, settings.muted);
    audio
}

/// The player's saved settings and the writer of their changes, in a live game the viewer
/// plays, with `--tier` in place of the saved graphics setting. It goes in the settings the
/// client holds, so that the settings the client asks to save never carry the other value back
/// to the frame loop.
fn player_settings(options: &Options) -> (Option<app_client::settings::Settings>, Option<Saver>) {
    let (mut settings, saver) = saved_settings(options);
    if let Some(graphics) = options.graphics {
        settings.get_or_insert_default().graphics = graphics;
    }
    (settings, saver)
}

/// The player's saved settings and the writer of their changes, in a live game the viewer
/// plays. A replayed recording, a resumed replay file and a game `--agent` plays start from
/// the defaults and save nothing, so that a test's or an agent's run never changes the
/// player's settings.
fn saved_settings(options: &Options) -> (Option<app_client::settings::Settings>, Option<Saver>) {
    #[cfg(feature = "tools")]
    if options.fixture.is_some() {
        return (None, None);
    }
    if options.agent.is_some() || options.replay.is_some() {
        return (None, None);
    }
    let Some(dir) =
        beryl_shell::Dirs::desktop(beryl_shell::Profile::current()).map(|dirs| dirs.data)
    else {
        return (None, None);
    };
    let settings = settings::load(&dir).unwrap_or_else(|error| {
        eprintln!("beryl3d: the saved settings could not be read, so the defaults apply: {error}");
        None
    });
    (settings, Some(Saver::start(dir)))
}

/// The web app's folders for the profile `BERYL_PROFILE` names, so that the two apps share
/// replays, the games ledger and bug reports, and a game saved by either resumes in the
/// other. Prints the data folder.
fn data_dirs() -> Option<beryl_shell::Dirs> {
    let dirs = beryl_shell::Dirs::desktop(beryl_shell::Profile::current());
    match &dirs {
        Some(dirs) => eprintln!("beryl3d: data in {}", dirs.data.display()),
        None => eprintln!("beryl3d: no home folder, so no game is saved"),
    }
    dirs
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

/// Opens the live socket for this process, and says where it is. A socket that cannot be made
/// leaves the app running without one.
#[cfg(all(feature = "tools", unix))]
fn start_live(inbox: Inbox) -> Option<crate::live::Live> {
    let pid = std::process::id();
    let dir = crate::live::socket_dir();
    match crate::live::Live::start(&dir, pid, out_dir(), inbox) {
        Ok(live) => {
            println!(
                "{LIVE_SOCKET} {}",
                crate::live::socket_path(&dir, pid).display()
            );
            Some(live)
        }
        Err(error) => {
            eprintln!("beryl3d: {error}");
            None
        }
    }
}
