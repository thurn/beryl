//! The options of every `just b3d` command that takes a recording (docs/beryl-3d.md, "The agent
//! interface"), read by one parser that the text commands and the picture program share.
//!
//! A recording is named as `just b3d list` prints it, or by a test's name,
//! `b3d:<recording>/<seat>/<width>x<height>[@<scale>]`, which also sets the seat, the window and
//! the scale. The options common to every such command are `--seat`, `--layout`, `--size`,
//! `--scale`, `--tier`, `--at`, `--do` (repeatable), `--json` and `--no-catch-up`, which plays
//! every script at real time however many updates wait, as the web interface does. A command adds options of its
//! own through [`Spec`], and reads them back with [`Options::value`] and [`Options::switch`].

use std::fmt::Write;

use app_client::driver::{Action, Moment};
use beryl_protocol::Seat;
use beryl_protocol::protocol::Side;
use kit_probe::agent::Layout;
use kit_ui::input::Size;

use crate::{recordings, stories};

/// Why a command line cannot be run. Its message says what was expected; the program exits
/// with status 2.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Usage(pub String);

impl std::fmt::Display for Usage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// The two visual tiers (docs/beryl-3d.md, "Rendering"). Text commands accept the option and
/// print the same tree in either, because the tiers differ only in shading; `png` and `film`
/// draw in the tier given.
pub use kit_ui::input::Tier;

/// A command's own options, beside the common ones.
#[derive(Clone, Copy, Debug, Default)]
pub struct Spec {
    /// Whether the command takes a recording as its first argument.
    pub recording: bool,
    /// Options that take a value, as `--within`.
    pub values: &'static [&'static str],
    /// Options that take none, as `--full`.
    pub switches: &'static [&'static str],
}

/// A command line, read.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// The command's name, as `tree`.
    pub command: String,
    /// The recording's name as `list` prints it, if the command takes one.
    pub recording: Option<String>,
    /// The arguments that are not options, after the recording.
    pub rest: Vec<String>,
    pub seat: Side,
    pub layout: Layout,
    /// The window in logical points: 1600 by 900 unless `--size` or a test's name says
    /// otherwise, or 393 by 759 on the phone layout.
    pub size: Size,
    pub scale: f32,
    pub tier: Tier,
    pub at: Moment,
    pub actions: Vec<Action>,
    /// Each action as it was written, for the printed command.
    action_lines: Vec<String>,
    pub json: bool,
    /// Whether scripts play faster while many updates wait; real time with `--no-catch-up`.
    pub pace: Pace,
    /// The command's own options, in the order given, each with its value if it takes one.
    pub own: Vec<(String, Option<String>)>,
    /// Which options were given explicitly, so that the printed command can leave out the
    /// rest.
    sized: bool,
    at_given: bool,
}

/// How fast a recording's scripts play while many updates wait to play.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    /// Faster, as the app catches up (`catch_up` in `data/beryl3d/space-constants.json`).
    CatchUp,
    /// At real time, as the web interface plays every script.
    RealTime,
}

/// The options every command that takes a recording accepts, each with its value's form.
const COMMON: [(&str, &str); 9] = [
    ("--seat", "you|opponent"),
    ("--layout", "desktop|phone"),
    ("--size", "<width>x<height>"),
    ("--scale", "<number>"),
    ("--tier", "baseline|enhanced"),
    ("--at", "<moment>"),
    ("--do", "<action>"),
    ("--json", ""),
    ("--no-catch-up", ""),
];

const DESKTOP: Size = Size {
    width: 1600.0,
    height: 900.0,
};
const PHONE: Size = Size {
    width: 393.0,
    height: 759.0,
};

impl Options {
    /// Reads `args`, the arguments after the command's name.
    ///
    /// # Errors
    ///
    /// When an option is unknown or its value cannot be read, or the recording is missing or
    /// named twice. Each message names what would have been accepted.
    pub fn parse(command: &str, args: &[String], spec: Spec) -> Result<Self, Usage> {
        let mut options = Self {
            command: command.to_owned(),
            recording: None,
            rest: Vec::new(),
            seat: Side::You,
            layout: Layout::Desktop,
            size: DESKTOP,
            scale: 1.0,
            tier: Tier::Baseline,
            at: Moment::Named {
                name: app_client::driver::Name::Start,
                offset: app_client::driver::Offset::default(),
            },
            actions: Vec::new(),
            action_lines: Vec::new(),
            json: false,
            pace: Pace::CatchUp,
            own: Vec::new(),
            sized: false,
            at_given: false,
        };
        let mut positional = Vec::new();
        let mut at = 0;
        let mut layout_given = false;
        while at < args.len() {
            let arg = &args[at];
            at += 1;
            if !arg.starts_with("--") {
                positional.push(arg.clone());
                continue;
            }
            // `--at=1500ms` is read as `--at 1500ms`.
            let (flag, inline) = match arg.split_once('=') {
                Some((flag, value)) => (flag, Some(value.to_owned())),
                None => (arg.as_str(), None),
            };
            let takes_value = COMMON
                .iter()
                .any(|&(name, form)| name == flag && !form.is_empty())
                || spec.values.contains(&flag);
            let is_switch = COMMON
                .iter()
                .any(|&(name, form)| name == flag && form.is_empty())
                || spec.switches.contains(&flag);
            if !takes_value && !is_switch {
                return Err(unknown_option(command, flag, spec));
            }
            let value = if takes_value {
                if let Some(value) = inline {
                    value
                } else {
                    let Some(value) = args.get(at) else {
                        return Err(Usage(format!("{flag} needs a value: {}", form(flag))));
                    };
                    at += 1;
                    value.clone()
                }
            } else {
                String::new()
            };
            options.set(flag, value, takes_value)?;
            layout_given |= flag == "--layout";
        }
        let mut positional = positional.into_iter();
        if spec.recording {
            let Some(name) = positional.next() else {
                if spec.switches.contains(&"--all") && options.switch("--all") {
                    return Ok(options);
                }
                return Err(Usage(format!(
                    "{command} needs a recording's name; `just b3d list` lists them"
                )));
            };
            let scene = options.test_name(&name)?;
            if scene.starts_with(stories::PREFIX) && !name.starts_with("b3d:") && !layout_given {
                let header = stories::open(&scene).map_err(Usage)?.header;
                if !options.sized {
                    options.size = header.window;
                }
                options.layout = header.layout;
            }
            options.recording = Some(scene);
        }
        options.rest = positional.collect();
        Ok(options)
    }

    /// The value of the command's own option `flag`, the last if it was given several times.
    #[must_use]
    pub fn value(&self, flag: &str) -> Option<&str> {
        self.own
            .iter()
            .rev()
            .find(|(name, _)| name == flag)
            .and_then(|(_, value)| value.as_deref())
    }

    /// Whether the command's own switch `flag` was given.
    #[must_use]
    pub fn switch(&self, flag: &str) -> bool {
        self.own.iter().any(|(name, _)| name == flag)
    }

    /// The recording's name, for a command that takes one.
    #[must_use]
    pub fn name(&self) -> &str {
        self.recording.as_deref().unwrap_or_default()
    }

    /// The seat the recording is played for.
    #[must_use]
    pub fn seat_word(&self) -> &'static str {
        seat_word(self.seat)
    }

    /// The command line that prints this output again: every option that sets where and what
    /// is shown, the seat and window always, so that it can be pasted into another command.
    #[must_use]
    pub fn command_line(&self) -> String {
        let mut out = format!("just b3d {}", self.command);
        if let Some(name) = &self.recording {
            let _ = write!(out, " {}", quote(name));
            let _ = write!(out, " --seat {}", self.seat_word());
            if self.layout == Layout::Phone {
                out.push_str(" --layout phone");
            } else if name.starts_with(stories::PREFIX) {
                out.push_str(" --layout desktop");
            }
            let _ = write!(out, " --size {}", size_text(self.size));
            if (self.scale - 1.0).abs() > f32::EPSILON {
                let _ = write!(out, " --scale {}", self.scale);
            }
            if self.tier == Tier::Enhanced {
                out.push_str(" --tier enhanced");
            }
            if self.at_given {
                let _ = write!(out, " --at {}", self.at);
            }
            if self.pace == Pace::RealTime {
                out.push_str(" --no-catch-up");
            }
        }
        for arg in &self.rest {
            let _ = write!(out, " {}", quote(arg));
        }
        for (flag, value) in &self.own {
            out.push(' ');
            out.push_str(flag);
            if let Some(value) = value {
                let _ = write!(out, " {}", quote(value));
            }
        }
        for line in &self.action_lines {
            let _ = write!(out, " --do {}", quote(line));
        }
        if self.json {
            out.push_str(" --json");
        }
        out
    }

    fn set(&mut self, flag: &str, value: String, takes_value: bool) -> Result<(), Usage> {
        let bad = |why: &str| Usage(format!("{flag} {value}: {why}; expected {}", form(flag)));
        match flag {
            "--seat" => self.seat = parse_seat(&value).ok_or_else(|| bad("not a seat"))?,
            "--layout" => {
                self.layout = match value.as_str() {
                    "desktop" => Layout::Desktop,
                    "phone" => Layout::Phone,
                    _ => return Err(bad("not a layout")),
                };
                if !self.sized {
                    self.size = match self.layout {
                        Layout::Desktop => DESKTOP,
                        Layout::Phone => PHONE,
                    };
                }
            }
            "--size" => {
                self.size = parse_size(&value).ok_or_else(|| bad("not a size"))?;
                self.sized = true;
            }
            "--scale" => {
                self.scale = value
                    .parse::<f32>()
                    .ok()
                    .filter(|scale| scale.is_finite() && *scale > 0.0)
                    .ok_or_else(|| bad("not a positive number"))?;
            }
            "--tier" => {
                self.tier = match value.as_str() {
                    "baseline" => Tier::Baseline,
                    "enhanced" => Tier::Enhanced,
                    _ => return Err(bad("not a tier")),
                };
            }
            "--at" => {
                self.at = value
                    .parse()
                    .map_err(|error| Usage(format!("--at {value}: {error}")))?;
                self.at_given = true;
            }
            "--do" => {
                let action = value
                    .parse()
                    .map_err(|error| Usage(format!("--do {value}: {error}")))?;
                self.actions.push(action);
                self.action_lines.push(value);
            }
            "--json" => self.json = true,
            "--no-catch-up" => self.pace = Pace::RealTime,
            _ => self
                .own
                .push((flag.to_owned(), takes_value.then_some(value))),
        }
        Ok(())
    }

    /// The recording's name from `name`, which is either a name or a test's name, setting the
    /// seat, the window and the layout from a test's name.
    fn test_name(&mut self, name: &str) -> Result<String, Usage> {
        let Some(test) = name.strip_prefix("b3d:") else {
            return Ok(name.to_owned());
        };
        // A card spec moment's name has a slash of its own, so the seat and window are read from
        // the end.
        let mut parts = test.rsplitn(3, '/');
        let (Some(size), Some(seat), Some(recording)) = (parts.next(), parts.next(), parts.next())
        else {
            return Err(Usage(format!(
                "'{name}' is not a test's name: write b3d:<recording>/<seat>/<width>x<height>, as \
                 in b3d:verbs-zones/you/1600x900"
            )));
        };
        self.seat = parse_seat(seat).ok_or_else(|| {
            Usage(format!(
                "'{name}': '{seat}' is not a seat; expected you or opponent"
            ))
        })?;
        let (size, scale) = match size.split_once('@') {
            Some((size, scale)) => (size, Some(scale)),
            None => (size, None),
        };
        self.size = parse_size(size).ok_or_else(|| {
            Usage(format!(
                "'{name}': '{size}' is not a size; expected <width>x<height>, as in 1600x900"
            ))
        })?;
        if let Some(scale) = scale {
            self.scale = scale
                .parse()
                .ok()
                .filter(|scale: &f32| scale.is_finite() && *scale > 0.0)
                .ok_or_else(|| {
                    Usage(format!(
                        "'{name}': '{scale}' is not a scale; expected a positive number, as in \
                         1600x900@2"
                    ))
                })?;
        }
        self.sized = true;
        // The layout and its safe area follow the window, as they do in `just b3d test`.
        let data = crate::data::checkout_ref().map_err(Usage)?;
        self.layout = Layout::for_size(&data.profiles, self.size);
        Ok(recording.to_owned())
    }
}

/// Checks that the recording `name` exists for `seat`.
///
/// # Errors
///
/// When it does not: the message names the seats it has, or the five nearest names.
pub fn known(name: &str, seat: Side) -> Result<(), Usage> {
    let names = recordings::names().map_err(Usage)?;
    let wanted = match seat {
        Side::You => Seat::P0,
        Side::Opponent => Seat::P1,
    };
    if names.iter().any(|(n, s)| n == name && *s == wanted) {
        return Ok(());
    }
    let seats: Vec<&str> = names
        .iter()
        .filter(|(n, _)| n == name)
        .map(|(_, s)| seat_word(side_of(*s)))
        .collect();
    if !seats.is_empty() {
        return Err(Usage(format!(
            "the recording '{name}' is not recorded for --seat {}, only for --seat {}",
            seat_word(seat),
            seats.join(" and --seat ")
        )));
    }
    let mut unique: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
    unique.dedup();
    Err(Usage(format!(
        "no recording is named '{name}'. The nearest names are:\n{}\n`just b3d list` lists \
         every recording.",
        nearest(name, &unique)
            .iter()
            .map(|n| format!("  {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    )))
}

/// The five of `names` nearest to `name`: those that start with it or contain it first, then by
/// edit distance to the whole name or to its start, whichever is nearer. The start counts so
/// that a misspelt prefix of a long name (`winds-of-abandn/exiles`) finds that name rather
/// than the short names a few edits away from it.
#[must_use]
pub fn nearest<'a>(name: &str, names: &[&'a str]) -> Vec<&'a str> {
    let length = name.chars().count();
    let mut sorted = names.to_vec();
    sorted.sort_by_key(|candidate| {
        let contains = candidate.contains(name) || name.contains(*candidate);
        let start: String = candidate.chars().take(length).collect();
        let distance =
            recordings::distance(candidate, name).min(recordings::distance(&start, name));
        (!contains, distance)
    });
    sorted.truncate(5);
    sorted
}

/// The seat a recording's messages are sent to, as the options name it.
#[must_use]
pub fn side_of(seat: Seat) -> Side {
    if seat == Seat::P0 {
        Side::You
    } else {
        Side::Opponent
    }
}

/// `you` or `opponent`.
#[must_use]
pub fn seat_word(seat: Side) -> &'static str {
    match seat {
        Side::You => "you",
        Side::Opponent => "opponent",
    }
}

/// A window's size as the options write it: `1600x900`.
#[must_use]
pub fn size_text(size: Size) -> String {
    format!("{}x{}", size.width, size.height)
}

/// `text` as one shell word: as it is when it needs no quoting, else in single quotes.
#[must_use]
pub fn quote(text: &str) -> String {
    let plain = !text.is_empty()
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-_./:,+=@%".contains(c));
    if plain {
        text.to_owned()
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

/// The help text's list of common options.
#[must_use]
pub fn common_help() -> String {
    let mut out = String::new();
    for (flag, form) in COMMON {
        let _ = writeln!(out, "  {}", format!("{flag} {form}").trim_end());
    }
    out
}

fn parse_seat(word: &str) -> Option<Side> {
    match word {
        "you" => Some(Side::You),
        "opponent" => Some(Side::Opponent),
        _ => None,
    }
}

fn parse_size(text: &str) -> Option<Size> {
    let (width, height) = text.split_once('x')?;
    let width: f32 = width.parse().ok()?;
    let height: f32 = height.parse().ok()?;
    (width >= 1.0 && height >= 1.0 && width.is_finite() && height.is_finite())
        .then_some(Size { width, height })
}

fn form(flag: &str) -> String {
    let form = COMMON
        .iter()
        .find(|(name, _)| *name == flag)
        .map_or("a value", |(_, form)| form);
    format!("{flag} {form}")
}

fn unknown_option(command: &str, flag: &str, spec: Spec) -> Usage {
    let mut known: Vec<&str> = COMMON.iter().map(|(name, _)| *name).collect();
    known.extend(spec.values);
    known.extend(spec.switches);
    let near = nearest(flag, &known);
    Usage(format!(
        "{command} has no option {flag}; the nearest are {}. `just b3d {command} --help` lists \
         them.",
        near.join(", ")
    ))
}
