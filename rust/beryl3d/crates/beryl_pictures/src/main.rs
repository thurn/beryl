//! The picture commands of `just b3d` (docs/beryl-3d.md, "The agent interface"): `png` draws one
//! moment of a recording with the real renderer, offscreen, `film` draws a span of it as a
//! grid of frames in one picture, and `compare` puts a moment beside the web interface's
//! screenshot of it. `sample` draws the conformance samples of `just b3d match samples`. The crate is this one program, with no library, and links no
//! engine, so that it rebuilds and relinks fast after an edit (docs/beryl-3d.md,
//! "Edit-to-output time"). Its tests run the program.
//!
//! The options are read by the parser the text commands use, and a wrong command line exits with
//! status 2 and a message that names what would work. Every file is written under
//! `$BERYL_CACHE/b3d/out/` with a name that includes the process id, so two runs never share one.

mod compare;
mod film;
mod font;
mod pixels;
mod sample;

use std::fmt::Write;
use std::io::Write as _;
use std::path::PathBuf;
use std::process::ExitCode;

use beryl_host::{Headless, Scans};
use beryl_inspect::cli::{self, Options, Ran, Spec, Usage, options};
use beryl_inspect::recordings;
use beryl_scene::agent::hit::is_within;
use beryl_scene::agent::{TreeOptions, tree};
use beryl_scene::input::Rect;

/// Why a command did not draw: its command line was wrong, or the machine could not.
pub(crate) enum Error {
    Usage(Usage),
    Failed(String),
}

impl From<Usage> for Error {
    fn from(usage: Usage) -> Self {
        Self::Usage(usage)
    }
}

/// One picture command.
struct Command {
    name: &'static str,
    summary: &'static str,
    examples: [(&'static str, &'static str); 2],
    spec: Spec,
    own: &'static [(&'static str, &'static str)],
    run: fn(&Options) -> Result<String, Error>,
}

const COMMANDS: [Command; 5] = [
    Command {
        name: "png",
        summary: "draws one moment of a recording to a PNG file",
        examples: [
            (
                "just b3d png verbs-zones --at 1500ms --tree",
                "the frame 1.5 s in, and its tree",
            ),
            (
                "just b3d png verbs-zones --at 1500ms --crop 'card \"Island\" #8'",
                "only the box around one element",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &["--crop", "--only"],
            switches: &["--tree"],
        },
        own: &[
            ("--crop <locator>", "only the box around that element"),
            (
                "--only <locator>",
                "draw that element and what is inside it alone, on black",
            ),
            ("--tree", "also print the tree of the same frame"),
        ],
        run: png,
    },
    Command {
        name: "film",
        summary: "draws up to 24 frames of a recording as a grid in one PNG file",
        examples: [
            (
                "just b3d film verbs-zones --from 0ms --to 3s --frames 12",
                "twelve frames over the first three seconds",
            ),
            (
                "just b3d film verbs-zones --from 800ms --to 1500ms --every 100ms --crop 'card #8'",
                "one card, followed every 100 ms",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &[
                "--from",
                "--to",
                "--frames",
                "--every",
                "--rate",
                "--frame-width",
                "--crop",
            ],
            switches: &[],
        },
        own: &[
            ("--from <moment>", "the first frame (default start)"),
            ("--to <moment>", "the last frame (default end)"),
            ("--frames <n>", "how many frames, 1 to 24 (default 12)"),
            (
                "--every <duration>",
                "one frame each duration, instead of --frames",
            ),
            (
                "--rate <frames a second>",
                "frames at that rate across the span, instead of --frames",
            ),
            (
                "--frame-width <px>",
                "each frame scaled down to that width (default: a picture at most 2000 px wide)",
            ),
            (
                "--crop <locator>",
                "follow that element, one box around it in each frame",
            ),
        ],
        run: film::film,
    },
    Command {
        name: "compare",
        summary: "puts the web interface's screenshot of a moment beside Beryl 3D's frame of it",
        examples: [
            (
                "just b3d compare opening-play --at prompt:1",
                "the play-or-draw prompt in both interfaces",
            ),
            (
                "just b3d compare pay-default --seat opponent --at view:2",
                "a moment from the opponent's seat",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &["--web"],
            switches: &[],
        },
        own: &[(
            "--web <png>",
            "the web interface's screenshot to put on the left (`just b3d compare` takes it)",
        )],
        run: compare::compare,
    },
    Command {
        name: "pixels",
        summary: "lists, worst first, the elements answerable for the pixels a match snapshot's \
                  two pictures differ in",
        examples: [
            (
                "just b3d match pixels opening-play --at prompt:1",
                "takes the snapshot and pairs it, then runs this",
            ),
            (
                "b3d-render pixels opening-play --at prompt:1 --scale 2 --dir <snapshot>",
                "on a snapshot directory already paired",
            ),
        ],
        spec: Spec {
            recording: true,
            values: &["--dir", "--only", "--web-only", "--box"],
            switches: &[],
        },
        own: &[
            (
                "--dir <directory>",
                "the `just b3d match` snapshot directory, with web.png, b3d.png, their records \
                 and joined.json",
            ),
            (
                "--only <locator>",
                "the snapshot shows that element alone (`just b3d match snapshot --only`)",
            ),
            (
                "--box <x>,<y>,<width>,<height>",
                "also count, in pixels.json's `inside`, the differing pixels inside this box, \
                 in points",
            ),
            (
                "--web-only <index>",
                "with --only, the index of its web pair in web.json",
            ),
        ],
        run: pixels::pixels,
    },
    Command {
        name: "sample",
        summary: "draws the conformance samples of a job and scores each against the web's picture",
        examples: [
            (
                "just b3d match samples",
                "every sample, shot in WebKit and drawn here",
            ),
            (
                "just b3d match samples shadow",
                "the samples whose names start with shadow",
            ),
        ],
        spec: Spec {
            recording: false,
            values: &["--job"],
            switches: &[],
        },
        own: &[(
            "--job <json>",
            "the samples to draw and the web's pictures of them (`just b3d match samples` writes it)",
        )],
        run: sample::sample,
    },
];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let ran = run(&args);
    // A closed pipe (`| head`) is not an error of the command's.
    let _ = std::io::stdout().write_all(ran.stdout.as_bytes());
    let _ = std::io::stderr().write_all(ran.stderr.as_bytes());
    ExitCode::from(ran.code)
}

/// Runs the command line `args`, the arguments after the program's name.
fn run(args: &[String]) -> Ran {
    // A missing recordings pack exits with its own status, so that `just b3d` builds it.
    let usage = |error: &Usage| Ran {
        stdout: String::new(),
        stderr: format!("{error}\n"),
        code: recordings::status(&error.0, 2),
    };
    let Some(first) = args.first() else {
        return Ran {
            stdout: listing(),
            ..Ran::default()
        };
    };
    if first == "--help" || first == "-h" {
        return Ran {
            stdout: listing(),
            ..Ran::default()
        };
    }
    let Some(command) = COMMANDS.iter().find(|c| c.name == first) else {
        let names: Vec<&str> = COMMANDS.iter().map(|c| c.name).collect();
        return usage(&Usage(format!(
            "there is no command '{first}'; the picture commands are {}. `just b3d` lists them.",
            names.join(", ")
        )));
    };
    let rest = &args[1..];
    if rest.iter().any(|arg| arg == "--help" || arg == "-h") {
        return Ran {
            stdout: help(command),
            ..Ran::default()
        };
    }
    let result = Options::parse(command.name, rest, command.spec)
        .map_err(Error::Usage)
        .and_then(|options| (command.run)(&options));
    match result {
        Ok(stdout) => Ran {
            stdout,
            ..Ran::default()
        },
        Err(Error::Usage(error)) => usage(&error),
        Err(Error::Failed(message)) => Ran {
            stdout: String::new(),
            code: recordings::status(&message, 1),
            stderr: format!("{message}\n"),
        },
    }
}

fn listing() -> String {
    let mut out = String::from("b3d-render: Beryl 3D's picture commands\n");
    for command in &COMMANDS {
        let _ = writeln!(out, "  {:<7}  {}", command.name, command.summary);
    }
    out.push_str("`b3d-render <command> --help` gives a command's options and examples.\n");
    out
}

fn help(command: &Command) -> String {
    let mut out = format!(
        "just b3d {} <recording>: {}\n\n\
         A recording is named as `just b3d list` prints it, or by a test's name,\n\
         b3d:<recording>/<seat>/<width>x<height>.\n\noptions:\n",
        command.name, command.summary
    );
    out.push_str(&options::common_help());
    for (flag, what) in command.own {
        let _ = writeln!(out, "  {flag}  {what}");
    }
    out.push_str(
        "\nA moment is a duration (850ms, 1.25s) or a name `info` prints (start, end, view:2,\n\
         prompt:1, step:2.0.1), with an optional offset (view:2+250ms). A locator is the\n\
         start of a tree line.\n\nexamples:\n",
    );
    let width = command
        .examples
        .iter()
        .map(|(l, _)| l.len())
        .max()
        .unwrap_or(0);
    for (line, what) in command.examples {
        let _ = writeln!(out, "  {line:<width$}  {what}");
    }
    out
}

/// The recording `options` names, played in the window, seat and tier they give, with a
/// renderer made with full limits, which draws the Enhanced tier where the GPU allows it.
pub(crate) fn open(options: &Options) -> Result<Headless, Error> {
    let page = cli::open(options)?.with_tier(options.tier);
    Headless::new(page, options.size.width, options.size.height, &Scans::app())
        .map_err(|why| Error::Failed(format!("no GPU to draw with: {why}")))
}

/// The directory pictures are written to, `$BERYL_CACHE/b3d/out`, which is made if needed.
pub(crate) fn out_dir() -> Result<PathBuf, Error> {
    let cache = std::env::var_os("BERYL_CACHE")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| PathBuf::from(home).join("Library/Caches/beryl"))
        })
        .ok_or_else(|| Error::Failed("neither BERYL_CACHE nor HOME is set".to_owned()))?;
    let dir = cache.join("b3d/out");
    std::fs::create_dir_all(&dir)
        .map_err(|why| Error::Failed(format!("{}: {why}", dir.display())))?;
    Ok(dir)
}

/// A file name for a picture of `options`' recording: the recording, seat, window and `what`
/// (the moment or span), and the process id.
pub(crate) fn file_name(options: &Options, what: &str) -> String {
    let text = format!(
        "{}-{}-{}-{what}-{}",
        options.name(),
        options.seat_word(),
        options::size_text(options.size),
        std::process::id()
    );
    let clean: String = text
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
                c
            } else {
                '_'
            }
        })
        .collect();
    format!("{clean}.png")
}

/// A box of pixels, `x` and `y` from the top left.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Region {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// How far around an element a crop reaches, in logical points.
const MARGIN: f32 = 16.0;

/// The box around `rect`, in logical points, in a picture of `size` pixels at `scale`.
pub(crate) fn around(rect: Rect, scale: f32, size: (u32, u32)) -> Region {
    let left = ((rect.x - MARGIN) * scale).floor().max(0.0) as u32;
    let top = ((rect.y - MARGIN) * scale).floor().max(0.0) as u32;
    let right = (((rect.x + rect.width + MARGIN) * scale).ceil() as u32).min(size.0);
    let bottom = (((rect.y + rect.height + MARGIN) * scale).ceil() as u32).min(size.1);
    Region {
        x: left.min(size.0.saturating_sub(1)),
        y: top.min(size.1.saturating_sub(1)),
        width: right.saturating_sub(left).max(1),
        height: bottom.saturating_sub(top).max(1),
    }
}

/// The pixels of `region` in `pixels`, a picture `stride` pixels wide.
pub(crate) fn cut(pixels: &[u8], stride: u32, region: Region) -> Vec<u8> {
    let mut out = Vec::with_capacity((region.width * region.height * 4) as usize);
    for row in region.y..region.y + region.height {
        let start = ((row * stride + region.x) * 4) as usize;
        out.extend_from_slice(&pixels[start..start + (region.width * 4) as usize]);
    }
    out
}

/// The element `locator` names in the present frame, as a box around it.
pub(crate) fn crop_of(head: &mut Headless, locator: &str, scale: f32) -> Result<Region, Usage> {
    let size = head.size();
    let element = head
        .page()
        .try_find(locator)
        .map_err(|error| Usage(format!("--crop: {error}")))?;
    Ok(around(element.rect(), scale, size))
}

fn png(options: &Options) -> Result<String, Error> {
    let mut head = open(options)?;
    let only = options.value("--only");
    if only.is_some() {
        // Only a looking frame knows which element each of its records belongs to.
        head.page().set_looking(true);
    }
    cli::seek_and_act(head.page(), options, &options.at)?;
    let region = match options.value("--crop") {
        Some(locator) => Some(crop_of(&mut head, locator, options.scale)?),
        None => None,
    };
    let (width, height) = head.size();
    let pixels = match only {
        Some(locator) => {
            let index = (head.page().try_find(locator))
                .map_err(|error| Usage(format!("--only: {error}")))?
                .index;
            head.pixels_keeping(|frame, at| at == index || is_within(frame, at, index))
        }
        None => head.pixels(),
    };
    let (pixels, width, height) = match region {
        Some(region) => (cut(&pixels, width, region), region.width, region.height),
        None => (pixels, width, height),
    };
    let moment = options.at.to_string();
    let path = out_dir()?.join(file_name(options, &moment));
    beryl_render::write_rgba_png(&path, &pixels, width, height).map_err(Error::Failed)?;
    let mut out = format!(
        "{}\nwrote {} ({width}x{height} px)\n",
        options.command_line(),
        path.display()
    );
    if options.switch("--tree") {
        let header = cli::position_line(head.page());
        let body = tree(head.frame(), &TreeOptions::default())
            .map_err(|error| Usage(error.to_string()))?;
        let _ = write!(out, "{header}\n{body}");
    }
    Ok(out)
}
