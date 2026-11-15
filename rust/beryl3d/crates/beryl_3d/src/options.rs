//! The app's command line (docs/beryl-3d.md, "Driving the running app"). `just b3d app` passes
//! it on, choosing the window for an agent's run and for a person's.

use std::time::Duration;

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
    /// Where the clock stops: a moment as `just b3d` writes one (`1500ms`, `prompt:1`).
    pub pause_at: Option<String>,
    /// How long the run lasts before the app ends it.
    pub run_for: Option<Duration>,
    pub window: Window,
}

/// Whether this build has the options only a debug build has (`--fixture` and `--pause-at`).
pub const TOOLS: bool = cfg!(feature = "tools");

impl Options {
    /// Reads `arguments`, the command line after the program's name. With `tools` false, as in
    /// a release build, `--fixture` and `--pause-at` are not options.
    ///
    /// # Errors
    ///
    /// When an argument is not an option, or an option's value is missing or malformed.
    pub fn parse(arguments: &[String], tools: bool) -> Result<Self, String> {
        let mut options = Self {
            fixture: None,
            pause_at: None,
            run_for: None,
            window: Window::Fullscreen,
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
                "--pause-at" if tools => options.pause_at = Some(value()?),
                "--for" => options.run_for = Some(duration(&value()?)?),
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
        Ok(options)
    }
}

/// The options this build takes, one line each.
#[must_use]
pub fn usage(tools: bool) -> String {
    let mut lines = vec![
        "usage: beryl3d [options]",
        "  --window none|background|foreground|fullscreen   what to open (fullscreen)",
        "  --for <duration>                                 end the run after this long",
    ];
    if tools {
        lines.push("  --fixture <recording>                            replay a recording");
        lines.push("  --pause-at <moment>                              stop the clock there");
    }
    lines.join("\n")
}

/// A duration written as `just b3d` writes one: a number and `ms` or `s` (`850ms`, `1.25s`).
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
