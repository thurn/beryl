//! The app's command line (docs/beryl-3d.md, "Driving the running app"). `just client app` passes
//! it on, choosing the window for an agent's run and for a person's.

use std::path::PathBuf;
use std::time::Duration;

use app_client::settings::Graphics;
use beryl_host::Class;

use crate::tier::Verdict;

/// What the app opens.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Window {
    /// No window: the windowless host draws into an offscreen target on a timer.
    None,
    /// A window that never takes focus and opens behind the others.
    Background,
    /// An ordinary window.
    Foreground,
    /// The whole display: what a person gets (decision C15).
    Fullscreen,
}

impl Window {
    /// The word the command line and the frame log use for it.
    #[must_use]
    pub fn word(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Background => "background",
            Self::Foreground => "foreground",
            Self::Fullscreen => "fullscreen",
        }
    }

    fn from_word(word: &str) -> Option<Self> {
        [
            Self::None,
            Self::Background,
            Self::Foreground,
            Self::Fullscreen,
        ]
        .into_iter()
        .find(|window| window.word() == word)
    }
}

/// The app's options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Options {
    /// The recording to replay instead of a game.
    pub fixture: Option<String>,
    /// A replay file whose game to resume at its latest decision instead of the newest game,
    /// such as a bug report's `replay.json` (`--replay`). The game saves nothing.
    pub replay: Option<PathBuf>,
    /// Where the clock stops: a moment as `just client` writes one (`1500ms`, `prompt:1`).
    pub pause_at: Option<String>,
    /// How long the run lasts before the app ends it.
    pub run_for: Option<Duration>,
    pub window: Window,
    /// Physical pixels to a logical point in a run with no window (`--scale`): 2 on macOS, the
    /// scale of a Retina display, so that an agent's run draws text as the desktop app does
    /// there, and 1 elsewhere. A window takes its display's scale instead.
    pub scale: u8,
    /// The drive script to play: a name in `client/drive`, or a path.
    pub drive: Option<String>,
    /// Use the checkout tuning and reload it when it changes (`--watch`).
    pub watch: bool,
    /// Busy threads to start, and their class (`--load`).
    pub load: Option<(usize, Class)>,
    /// How long each finished frame is held back before it is shown (`--slow-gpu`).
    pub slow_gpu: Option<Duration>,
    /// With `fixture`, write that recording to this file as JSON and exit, for a phone's app to
    /// replay.
    pub write_fixture: Option<String>,
    /// A fresh game in which this agent answers for the viewer, so that a run with no person
    /// plays a whole game and the AI searches (`--agent`).
    pub agent: Option<String>,
    /// With `agent`, the agent in the other seat.
    pub opponent: String,
    /// With `agent`, the game's seed.
    pub seed: u64,
    /// The scheduling class the session thread, and through it the AI's search threads, run
    /// in (`--search-class`): default, as in the web app, unless a run compares another.
    pub search_class: Class,
    /// Agents the start screen offers after the session's own list, by name (`--offer`, given
    /// once for each): a retired agent such as `expert`, which `just client stand-in` plays.
    pub offer: Vec<String>,
    /// How the tier is chosen for this run, in place of the saved setting (`--tier`).
    pub graphics: Option<Graphics>,
    /// The measurement's verdict at startup, in place of the one it measures (`--probe`).
    pub probe: Option<Verdict>,
}

/// The agent across the table in a game `--agent` starts: the searching agent the prototype
/// measured, with the budget the web app gives it.
pub const OPPONENT: &str = "mcts-hidden";
/// The seed of a game `--agent` starts, unless `--seed` gives another: the prototype's.
pub const SEED: u64 = 20_261_114;

/// The scale of a run with no window unless `--scale` gives another.
pub const WINDOWLESS_SCALE: u8 = if cfg!(target_os = "macos") { 2 } else { 1 };

/// Whether this build has the options only a debug build has (`--fixture`, `--replay`,
/// `--pause-at`, `--drive`, `--watch`, `--load`, `--slow-gpu`, `--agent`, `--opponent`, `--seed`,
/// `--search-class`, `--offer`, `--tier` and `--probe`).
pub const TOOLS: bool = cfg!(feature = "tools");

impl Options {
    /// Reads `arguments`, the command line after the program's name. With `tools` false, as in
    /// a release build, the options of `TOOLS` are not options.
    ///
    /// # Errors
    ///
    /// When an argument is not an option, or an option's value is missing or malformed.
    pub fn parse(arguments: &[String], tools: bool) -> Result<Self, String> {
        let mut options = Self {
            fixture: None,
            replay: None,
            pause_at: None,
            run_for: None,
            window: Window::Fullscreen,
            scale: WINDOWLESS_SCALE,
            drive: None,
            watch: false,
            load: None,
            slow_gpu: None,
            write_fixture: None,
            agent: None,
            opponent: OPPONENT.to_owned(),
            seed: SEED,
            search_class: Class::Default,
            offer: Vec::new(),
            graphics: None,
            probe: None,
        };
        let mut rest = arguments.iter();
        while let Some(option) = rest.next() {
            let mut value = || {
                rest.next()
                    .cloned()
                    .ok_or_else(|| format!("{option} needs a value"))
            };
            match option.as_str() {
                "--fixture" if tools => options.fixture = Some(value()?),
                "--replay" if tools => {
                    let path = PathBuf::from(value()?);
                    if !path.is_file() {
                        return Err(format!("--replay {}: no such file", path.display()));
                    }
                    options.replay = Some(path);
                }
                "--pause-at" if tools => options.pause_at = Some(value()?),
                "--drive" if tools => options.drive = Some(value()?),
                "--watch" if tools => options.watch = true,
                "--load" if tools => options.load = Some(crate::load::parse(&value()?)?),
                "--slow-gpu" if tools => options.slow_gpu = Some(duration(&value()?)?),
                "--write-fixture" if tools => options.write_fixture = Some(value()?),
                "--agent" if tools => options.agent = Some(value()?),
                "--opponent" if tools => options.opponent = value()?,
                "--seed" if tools => {
                    let text = value()?;
                    options.seed = text
                        .parse()
                        .map_err(|_| format!("--seed {text}: a seed is a whole number"))?;
                }
                "--search-class" if tools => {
                    let word = value()?;
                    options.search_class = Class::from_word(&word).ok_or_else(|| {
                        format!("--search-class {word}: a class is utility, default, initiated or interactive")
                    })?;
                }
                "--offer" if tools => {
                    let name = value()?;
                    if beryl_search::agent(&name).is_none() {
                        return Err(format!("--offer {name}: no agent has that name"));
                    }
                    options.offer.push(name);
                }
                "--tier" if tools => {
                    let word = value()?;
                    options.graphics = Some(crate::tier::graphics(&word).ok_or_else(|| {
                        format!("--tier {word}: a tier is automatic, baseline or enhanced")
                    })?);
                }
                "--probe" if tools => {
                    options.probe = Some(match value()?.as_str() {
                        "pass" => Verdict::Pass,
                        "fail" => Verdict::Fail,
                        word => return Err(format!("--probe {word}: a verdict is pass or fail")),
                    });
                }
                "--for" => {
                    let text = value()?;
                    options.run_for = Some(match text.parse::<u64>() {
                        Ok(ms) => Duration::from_millis(ms),
                        Err(_) => duration(&text)?,
                    });
                }
                "--scale" => options.scale = scale(&value()?)?,
                "--window" => {
                    let word = value()?;
                    options.window = Window::from_word(&word).ok_or_else(|| {
                        format!(
                            "--window {word}: a window is none, background, foreground or \
                             fullscreen"
                        )
                    })?;
                }
                _ => return Err(format!("{option} is not an option.\n{}", usage(tools))),
            }
        }
        if options.replay.is_some() && (options.fixture.is_some() || options.agent.is_some()) {
            return Err("--replay resumes a game, so it takes no --fixture or --agent".to_owned());
        }
        Ok(options)
    }
}

/// The options this build takes, one line each.
#[must_use]
pub fn usage(tools: bool) -> String {
    let mut lines = vec![
        "usage: beryl [options]",
        "  --window none|background|foreground|fullscreen   what to open (fullscreen)",
        "  --for <duration>                                 end after this long (bare milliseconds or a unit)",
        "  --scale <1-4>                                    pixels per point with no window",
    ];
    if tools {
        lines.push("  --fixture <recording>                            replay a recording");
        lines.push(
            "  --replay <replay.json>                           resume that file's game, saving none",
        );
        lines.push("  --watch                                          reload tuning files when they change");
        lines.push("  --pause-at <moment>                              stop the clock there");
        lines.push(
            "  --drive <script>                                 play a script of actions, then end",
        );
        lines.push(
            "  --load <threads>[:<class>]                       busy threads (class default)",
        );
        lines.push(
            "  --slow-gpu <duration>                            hold each frame back this long",
        );
        lines.push(
            "  --write-fixture <path>                           write --fixture's recording, exit",
        );
        lines.push(
            "  --agent <name>                                   a new game this agent plays for you",
        );
        lines.push(
            "  --opponent <name>                                --agent's opponent (mcts-hidden)",
        );
        lines.push("  --seed <number>                                  --agent's game's seed");
        lines.push(
            "  --search-class <class>                           the AI's threads' class (default)",
        );
        lines.push(
            "  --offer <agent>                                  add an agent to the start screen",
        );
        lines.push(
            "  --tier automatic|baseline|enhanced               how the tier is chosen (setting)",
        );
        lines.push(
            "  --probe pass|fail                                the startup measurement's verdict",
        );
    }
    lines.join("\n")
}

/// The drive script `name` names: a file in `client/drive` named `<name>.drive`, or a
/// path.
#[cfg(feature = "tools")]
pub(crate) fn drive_script(name: &str) -> Result<beryl_host::Drive, String> {
    let scripts = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../drive");
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

/// A scale of a run with no window: a whole number of physical pixels to a point, 1 to 4.
fn scale(text: &str) -> Result<u8, String> {
    text.parse()
        .ok()
        .filter(|scale| (1..=4).contains(scale))
        .ok_or_else(|| format!("--scale {text}: a scale is a whole number from 1 to 4"))
}

/// A duration written as `just client` writes one: a number and `ms` or `s` (`850ms`, `1.25s`).
fn duration(text: &str) -> Result<Duration, String> {
    let (number, scale) = if let Some(number) = text.strip_suffix("ms") {
        (number, 1e-3)
    } else if let Some(number) = text.strip_suffix('s') {
        (number, 1.0)
    } else {
        return Err(format!(
            "'{text}' is not a duration: write a number and ms or s, as in 850ms or 1.25s"
        ));
    };
    number
        .parse::<f64>()
        .ok()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
        .map(|seconds| Duration::from_secs_f64(seconds * scale))
        .ok_or_else(|| format!("'{text}' is not a duration: '{number}' is not a number"))
}
