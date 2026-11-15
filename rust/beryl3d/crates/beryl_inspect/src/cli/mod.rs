//! The text commands of `just b3d` (docs/beryl-3d.md, "The agent interface"): `list`, `info`,
//! `tree`, `diff`, `check` and `test`. The `b3d` program runs [`main`]; `just b3d` builds it and
//! passes its arguments through.
//!
//! Every printout begins with the command that prints it again, every name it prints can be
//! pasted into the next command, and a wrong command line exits with status 2 and a message that
//! names what would work.

mod check;
pub mod line;
mod listing;
pub mod options;
mod test;
mod view;

use std::fmt::Write;

use beryl_scene::driver::{Moment, Page};

pub use options::{Options, Spec, Tier, Usage};
pub use view::position_line;

use crate::{pages, recordings};

/// What a command printed and the status it exits with: 0 when it ran, 1 when a check found a
/// fault, 2 when the command line was wrong, and 3 when the recordings pack is missing
/// ([`recordings::MISSING`]), which `just b3d` builds before running the command again.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Ran {
    pub stdout: String,
    pub stderr: String,
    pub code: u8,
}

/// One text command.
struct Command {
    name: &'static str,
    /// What `--help` and the listing say it does.
    summary: &'static str,
    /// Two command lines, each with what it does.
    examples: [(&'static str, &'static str); 2],
    spec: Spec,
    /// Its own options, each with what it does, for `--help`.
    own: &'static [(&'static str, &'static str)],
    run: fn(&Options) -> Result<Printed, Usage>,
}

/// What a command printed, and whether it found a fault.
pub(crate) struct Printed {
    text: String,
    fault: bool,
}

impl Printed {
    fn text(text: String) -> Self {
        Self { text, fault: false }
    }
}

const COMMANDS: [Command; 6] = [
    Command {
        name: "list",
        summary: "every recording whose name starts with the prefix, with its length and views",
        examples: [
            ("just b3d list", "every recording"),
            (
                "just b3d list verbs",
                "the recordings whose names start with verbs",
            ),
        ],
        spec: Spec {
            recording: false,
            values: &[],
            switches: &["--full"],
        },
        own: &[("--full", "every recording, with none cut")],
        run: listing::list,
    },
    Command {
        name: "info",
        summary: "a recording's moments: views, script steps and prompts with their buttons",
        examples: [
            (
                "just b3d info verbs-zones",
                "every moment, from the first seat",
            ),
            (
                "just b3d info verbs-zones --seat opponent --json",
                "the second seat's moments as JSON",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &[],
            switches: &["--full"],
        },
        own: &[("--full", "every moment, with none cut")],
        run: listing::info,
    },
    Command {
        name: "tree",
        summary: "the elements on screen at one moment, as an indented tree",
        examples: [
            (
                "just b3d tree verbs-zones --at 1500ms",
                "the tree 1.5 s into the recording",
            ),
            (
                "just b3d tree verbs-zones --at view:2 --within hand.yours",
                "your hand once the second view is still",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &["--within", "--role"],
            switches: &["--full"],
        },
        own: &[
            (
                "--within <place|locator>",
                "only that place or element, whole",
            ),
            ("--role <role>", "only elements of that role"),
            ("--full", "every element, with no place cut short"),
        ],
        run: view::tree,
    },
    Command {
        name: "diff",
        summary: "the elements that appeared, left, moved or changed state between two moments",
        examples: [
            (
                "just b3d diff verbs-zones --from 800ms --to 1500ms",
                "what changed between two times",
            ),
            (
                "just b3d diff verbs-zones --from view:1 --to view:1 --do 'hover card #8'",
                "what hovering a card changes",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &["--from", "--to"],
            switches: &["--full"],
        },
        own: &[
            (
                "--from <moment>",
                "the first moment; --do runs after it (default start)",
            ),
            ("--to <moment>", "the second moment (default end)"),
            ("--full", "every change, with none cut"),
        ],
        run: view::diff,
    },
    Command {
        name: "check",
        summary: "every interface check on every tick of a recording, and the faults",
        examples: [
            (
                "just b3d check verbs-zones",
                "every tick, from the first seat at 1600x900",
            ),
            (
                "just b3d check verbs-zones --seat opponent --size 1440x900",
                "the second seat in a smaller window",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &[],
            switches: &[],
        },
        own: &[],
        run: check::check,
    },
    Command {
        name: "test",
        summary: "every interface test whose name starts with the prefix, and the faults",
        examples: [
            ("just b3d test", "every test, at its check times"),
            (
                "just b3d test b3d:verbs-zones --every-tick",
                "one recording's tests, checked on every tick",
            ),
        ],
        spec: Spec {
            recording: false,
            values: &[],
            switches: &[test::EVERY_TICK],
        },
        own: &[(
            "--every-tick",
            "check every tick instead of the check times, as the sweep does",
        )],
        run: test::test,
    },
];

/// Runs the command line `args`, the arguments after the program's name.
#[must_use]
pub fn main(args: &[String]) -> Ran {
    let Some(first) = args.first() else {
        return Ran {
            stdout: listing_text(),
            ..Ran::default()
        };
    };
    if first == "--help" || first == "-h" {
        return Ran {
            stdout: listing_text(),
            ..Ran::default()
        };
    }
    let Some(command) = COMMANDS.iter().find(|c| c.name == first) else {
        let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        let near = options::nearest(first, &names);
        return usage(&Usage(format!(
            "there is no command '{first}'; the nearest are {}. `just b3d` lists them.",
            near.join(", ")
        )));
    };
    let rest = &args[1..];
    if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ran {
            stdout: help(command),
            ..Ran::default()
        };
    }
    let printed = Options::parse(command.name, rest, command.spec)
        .and_then(|options| (command.run)(&options));
    match printed {
        Ok(printed) => Ran {
            stdout: printed.text,
            stderr: String::new(),
            code: u8::from(printed.fault),
        },
        Err(error) => usage(&error),
    }
}

fn usage(error: &Usage) -> Ran {
    Ran {
        stdout: String::new(),
        stderr: format!("{error}\n"),
        code: if error.0.contains(recordings::MISSING) {
            3
        } else {
            2
        },
    }
}

/// The listing of the text commands, one line each.
fn listing_text() -> String {
    let mut out = String::from("b3d: Beryl 3D's text commands\n");
    for command in &COMMANDS {
        let _ = writeln!(out, "  {:<6}  {}", command.name, command.summary);
    }
    out.push_str("`b3d <command> --help` gives a command's options and examples.\n");
    out
}

fn help(command: &Command) -> String {
    let mut out = format!(
        "just b3d {}{}: {}\n\n",
        command.name,
        if command.spec.recording {
            " <recording>"
        } else {
            " [prefix]"
        },
        command.summary
    );
    if command.spec.recording {
        out.push_str(
            "A recording is named as `just b3d list` prints it, or by a test's name,\n\
             b3d:<recording>/<seat>/<width>x<height>.\n\noptions:\n",
        );
        out.push_str(&options::common_help());
    } else if !command.own.is_empty() {
        out.push_str("options:\n");
    }
    for (flag, what) in command.own {
        let _ = writeln!(out, "  {flag}  {what}");
    }
    if command.spec.recording {
        out.push_str(
            "\nA moment is a duration (850ms, 1.25s) or a name `info` prints (start, end, \
             view:2, prompt:1,\nstep:2.0.1), with an optional offset (view:2+250ms). A locator \
             is the start of a tree line.\n",
        );
    }
    out.push_str("\nexamples:\n");
    let width = command.examples.iter().map(|(l, _)| l.len()).max();
    for (line, what) in command.examples {
        let _ = writeln!(out, "  {line:<w$}  {what}", w = width.unwrap_or(0));
    }
    out
}

/// The recording `options` names, in the window and seat they give, at its start.
pub fn open(options: &Options) -> Result<Page, Usage> {
    options::known(options.name(), options.seat)?;
    pages::page(options.name(), options.seat, options.layout, options.size).map_err(Usage)
}

/// The time of `moment` in `page`'s recording, in nanoseconds since its start.
///
/// # Errors
///
/// When the recording has no such moment, or the moment lies past the recording's end; the
/// message names the moments it has.
pub fn time_of(page: &mut Page, options: &Options, moment: &Moment) -> Result<u64, Usage> {
    let info = format!("`just b3d info {}` lists its moments", options.name());
    let time = page
        .time_of(moment)
        .map_err(|error| Usage(format!("{error}\n{info}")))?;
    let end = page.moments().last().map_or(0, |(_, at)| at.0);
    if time > end {
        return Err(Usage(format!(
            "{moment} is past the end of {}, which ends at {}ms. Write a time up to {}ms, or a \
             moment's name such as end or view:1; {info}.",
            options.name(),
            milliseconds(end),
            milliseconds(end),
        )));
    }
    Ok(time)
}

/// Seeks `page` to `moment` and does each of `options`' actions in order.
///
/// # Errors
///
/// When the moment is wrong (see [`time_of`]), or an action's locator names no single
/// uncovered element.
pub fn seek_and_act(page: &mut Page, options: &Options, moment: &Moment) -> Result<(), Usage> {
    let time = time_of(page, options, moment)?;
    page.seek(&Moment::At(beryl_scene::driver::Duration(time)))
        .map_err(|error| Usage(error.to_string()))?;
    act(page, options)
}

/// Does each of `options`' actions in order.
pub fn act(page: &mut Page, options: &Options) -> Result<(), Usage> {
    for action in &options.actions {
        page.try_act(action)
            .map_err(|error| Usage(format!("--do: {error}")))?;
    }
    Ok(())
}

/// Nanoseconds as whole milliseconds, rounded down, so that every time printed can be given
/// back to `--at`: a recording that ends at 9240.6 ms ends at 9240ms, and 9241ms is past it.
pub fn milliseconds(nanoseconds: u64) -> u64 {
    nanoseconds / 1_000_000
}
