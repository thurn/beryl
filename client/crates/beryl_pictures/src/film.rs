//! `film`: a span of a recording as a grid of frames in one picture.

use std::fmt::Write;

use crate::scene::SceneHost;
use beryl_inspect::cli::{self, Options, Usage};
use game_probe::driver::{Duration, Moment, Name, Offset};
use kit_frame::frame::StateBits;
use kit_probe::agent::tree::locator_of;

use super::{Error, Region, crop_of, cut, file_name, font, open, out_dir};

/// The most frames a film holds.
const MOST_FRAMES: u64 = 24;
/// The frames of a film when neither `--frames` nor `--every` says.
const DEFAULT_FRAMES: u64 = 12;
/// The widest a film's picture is, in pixels.
const MOST_WIDTH: u32 = 2000;
/// How many elements a frame's line names as moving before it counts the rest.
const MOST_NAMED: usize = 3;

fn moment(options: &Options, flag: &str, name: Name) -> Result<Moment, Usage> {
    match options.value(flag) {
        None => Ok(Moment::Named {
            name,
            offset: Offset::default(),
        }),
        Some(text) => text
            .parse()
            .map_err(|error| Usage(format!("{flag} {text}: {error}"))),
    }
}

/// The options that each say how many frames a film holds; a command line gives at most one.
const COUNTS: [&str; 3] = ["--frames", "--every", "--rate"];

/// The times of the frames, in nanoseconds since the recording's start.
fn times(options: &Options, from: u64, to: u64) -> Result<Vec<u64>, Usage> {
    let span = to - from;
    let given: Vec<&str> = COUNTS
        .into_iter()
        .filter(|flag| options.value(flag).is_some())
        .collect();
    if let [first, second, ..] = given[..] {
        return Err(Usage(format!(
            "{first} and {second} both say how many frames; give one"
        )));
    }
    if let Some(text) = options.value("--rate") {
        return rate_times(text, from, span);
    }
    // How many frames, and how far apart when `--every` says.
    let (count, every) =
        match (options.value("--frames"), options.value("--every")) {
            (Some(text), _) => {
                let count = text.parse::<u64>().ok().filter(|n| *n >= 1).ok_or_else(|| {
                Usage(format!(
                    "--frames {text}: not a count; write a whole number from 1 to {MOST_FRAMES}"
                ))
            })?;
                (count, None)
            }
            (None, Some(text)) => {
                let every: Duration = text
                    .parse()
                    .map_err(|error| Usage(format!("--every {text}: {error}")))?;
                if every.0 == 0 {
                    return Err(Usage(
                        "--every 0ms: a frame each 0 ms never ends; write a longer duration"
                            .to_owned(),
                    ));
                }
                (span / every.0 + 1, Some(every.0))
            }
            (None, None) => (DEFAULT_FRAMES, None),
        };
    if count > MOST_FRAMES {
        return Err(Usage(format!(
            "that span would make {count} frames, and a film holds at most {MOST_FRAMES}; \
             write --frames {MOST_FRAMES} or fewer, or --every {}ms or longer",
            cli::milliseconds(span / (MOST_FRAMES - 1)).max(1)
        )));
    }
    let step = every.unwrap_or(if count > 1 { span / (count - 1) } else { 0 });
    Ok((0..count).map(|i| from + step * i).collect())
}

/// The times of `--rate <text>` frames a second over `span` nanoseconds from `from`: the
/// span's length times the rate, rounded, and at least one, as `just motion --film` cuts
/// Arena's frames of a verb. The first frame is at `from`, so the last is one frame's time
/// before the span's end.
fn rate_times(text: &str, from: u64, span: u64) -> Result<Vec<u64>, Usage> {
    let rate = text
        .parse::<f64>()
        .ok()
        .filter(|rate| rate.is_finite() && *rate > 0.0)
        .ok_or_else(|| {
            Usage(format!(
                "--rate {text}: not a rate; write frames a second, as 15"
            ))
        })?;
    let seconds = span as f64 / 1e9;
    let count = ((seconds * rate).round() as u64).max(1);
    if count > MOST_FRAMES {
        // The fastest rate that fits, rounded down to a hundredth so that it still fits.
        let lower = (MOST_FRAMES as f64 / seconds * 100.0).floor() / 100.0;
        return Err(Usage(format!(
            "that span at --rate {text} would make {count} frames, and a film holds at most \
             {MOST_FRAMES}; write --rate {lower} or lower, or a shorter span"
        )));
    }
    let period = 1e9 / rate;
    Ok((0..count)
        .map(|i| from + (i as f64 * period).round() as u64)
        .collect())
}

/// The width `--frame-width` asks of each frame, if it is given.
fn frame_width(options: &Options) -> Result<Option<u32>, Usage> {
    options
        .value("--frame-width")
        .map(|text| {
            text.parse::<u32>()
                .ok()
                .filter(|width| *width >= 1)
                .ok_or_else(|| {
                    Usage(format!(
                        "--frame-width {text}: not a width; write a whole number of pixels, as 760"
                    ))
                })
        })
        .transpose()
}

/// Plays the recording to `from` and acts, then runs the clock to each of `times` and calls
/// `visit` with the frame's index.
fn walk(
    head: &mut SceneHost,
    options: &Options,
    from: &Moment,
    times: &[u64],
    mut visit: impl FnMut(&mut SceneHost, u64, usize) -> Result<(), Error>,
) -> Result<(), Error> {
    cli::seek_and_act(head.page(), options, from)?;
    for (index, &time) in times.iter().enumerate() {
        let now = head.page().now();
        head.page().step(Duration(time.saturating_sub(now)));
        visit(head, time, index)?;
    }
    Ok(())
}

/// What is moving in the present frame, as a line's text.
fn moving(head: &mut SceneHost) -> String {
    let frame = head.frame();
    let names: Vec<String> = frame
        .resolved
        .iter()
        .enumerate()
        .filter(|(_, element)| element.state.has(StateBits::MOVING))
        .map(|(index, _)| locator_of(frame, index as u32))
        .collect();
    if names.is_empty() {
        return "nothing moving".to_owned();
    }
    let shown = names
        .iter()
        .take(MOST_NAMED)
        .cloned()
        .collect::<Vec<_>>()
        .join(", ");
    match names.len().saturating_sub(MOST_NAMED) {
        0 => format!("moving: {shown}"),
        rest => format!("moving: {shown} and {rest} more"),
    }
}

/// Shrinks `pixels`, `width` by `height`, to `to_width` by `to_height` by averaging the source
/// pixels each new pixel covers.
fn shrink(pixels: &[u8], width: u32, height: u32, to_width: u32, to_height: u32) -> Vec<u8> {
    if (width, height) == (to_width, to_height) {
        return pixels.to_vec();
    }
    let mut out = Vec::with_capacity((to_width * to_height * 4) as usize);
    for y in 0..to_height {
        let top = y * height / to_height;
        let bottom = ((y + 1) * height / to_height).max(top + 1).min(height);
        for x in 0..to_width {
            let left = x * width / to_width;
            let right = ((x + 1) * width / to_width).max(left + 1).min(width);
            let mut sum = [0u32; 4];
            for row in top..bottom {
                for column in left..right {
                    let at = ((row * width + column) * 4) as usize;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(pixels[at + channel]);
                    }
                }
            }
            let count = (bottom - top) * (right - left);
            out.extend(sum.iter().map(|total| (total / count) as u8));
        }
    }
    out
}

pub(super) fn film(options: &Options) -> Result<String, Error> {
    let from = moment(options, "--from", Name::Start)?;
    let to = moment(options, "--to", Name::End)?;
    let mut head = open(options)?;
    let from_time = cli::time_of(head.page(), options, &from)?;
    let to_time = cli::time_of(head.page(), options, &to)?;
    if to_time < from_time {
        return Err(Usage(format!(
            "--to {to} ({}ms) is before --from {from} ({}ms); give the earlier moment as --from",
            cli::milliseconds(to_time),
            cli::milliseconds(from_time)
        ))
        .into());
    }
    let times = times(options, from_time, to_time)?;
    let asked_width = frame_width(options)?;
    let size = head.size();

    // With `--crop`, a first pass follows the element to find the box that holds it in every
    // frame, so that all the frames of the film are the same size.
    let cell = match options.value("--crop") {
        None => None,
        Some(locator) => {
            let mut boxes: Vec<Region> = Vec::new();
            walk(&mut head, options, &from, &times, |head, time, _| {
                let region = crop_of(head, locator, options.scale).map_err(|error| {
                    Usage(format!("{error} (at {}ms)", cli::milliseconds(time)))
                })?;
                boxes.push(region);
                Ok(())
            })?;
            let width = boxes.iter().map(|b| b.width).max().unwrap_or(1);
            let height = boxes.iter().map(|b| b.height).max().unwrap_or(1);
            Some((width.min(size.0), height.min(size.1), boxes))
        }
    };
    let (cell_width, cell_height) = cell.as_ref().map_or(size, |(w, h, _)| (*w, *h));

    let count = times.len() as u32;
    let columns = f64::from(count).sqrt().ceil() as u32;
    let rows = count.div_ceil(columns);
    // `--frame-width` scales each frame down to that width, never up, as Arena's frames are
    // cut for `just motion --film`; without it, the whole picture is at most `MOST_WIDTH` wide.
    let shown_width = match asked_width {
        Some(width) => cell_width.min(width),
        None => cell_width.min(MOST_WIDTH / columns),
    }
    .max(1);
    let shown_height = (cell_height * shown_width / cell_width).max(1);
    let (picture_width, picture_height) = (columns * shown_width, rows * shown_height);
    let mut picture = vec![0u8; (picture_width * picture_height * 4) as usize];
    for opacity in picture.iter_mut().skip(3).step_by(4) {
        *opacity = 255;
    }

    let mut lines = String::new();
    walk(&mut head, options, &from, &times, |head, time, index| {
        let pixels = head.pixels();
        let cropped = match &cell {
            None => pixels,
            Some((width, height, boxes)) => {
                // The box keeps its centre on the element and the film's size.
                let found = boxes[index];
                let x = (found.x + found.width / 2).saturating_sub(width / 2);
                let y = (found.y + found.height / 2).saturating_sub(height / 2);
                let region = Region {
                    x: x.min(size.0 - width),
                    y: y.min(size.1 - height),
                    width: *width,
                    height: *height,
                };
                cut(&pixels, size.0, region)
            }
        };
        let small = shrink(&cropped, cell_width, cell_height, shown_width, shown_height);
        let (column, row) = (index as u32 % columns, index as u32 / columns);
        for line in 0..shown_height {
            let to =
                (((row * shown_height + line) * picture_width + column * shown_width) * 4) as usize;
            let source = (line * shown_width * 4) as usize;
            picture[to..to + (shown_width * 4) as usize]
                .copy_from_slice(&small[source..source + (shown_width * 4) as usize]);
        }
        let label = format!("{}ms", cli::milliseconds(time));
        font::label(
            &mut picture,
            picture_width,
            column * shown_width,
            row * shown_height,
            &label,
            2,
            2,
        );
        let _ = writeln!(lines, "  #{} {label}  {}", index + 1, moving(head));
        Ok(())
    })?;

    let what = format!("{from}-{to}-{count}").replace(' ', "");
    let path = out_dir()?.join(format!("film-{}", file_name(options, &what)));
    beryl_render::write_rgba_png(&path, &picture, picture_width, picture_height)
        .map_err(Error::Failed)?;
    Ok(format!(
        "{}\nwrote {} ({picture_width}x{picture_height} px, {count} frames of \
         {shown_width}x{shown_height} px in {columns} columns)\n{lines}",
        options.command_line(),
        path.display()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options(args: &[&str]) -> Options {
        let film = crate::COMMANDS
            .iter()
            .find(|c| c.name == "film")
            .expect("film");
        let args: Vec<String> = args.iter().map(|&a| a.to_owned()).collect();
        Options::parse("film", &args, film.spec).expect("the options parse")
    }

    #[test]
    fn a_rate_of_15_over_one_second_gives_15_frames_a_fifteenth_of_a_second_apart() {
        let found =
            times(&options(&["verbs-zones", "--rate", "15"]), 0, 1_000_000_000).expect("the times");
        assert_eq!(found.len(), 15);
        for pair in found.windows(2) {
            let apart = pair[1] - pair[0];
            assert!((66_666_666..=66_666_667).contains(&apart), "{found:?}");
        }
    }

    #[test]
    fn a_rate_and_a_frame_count_together_are_a_usage_error() {
        let given = options(&["verbs-zones", "--rate", "15", "--frames", "6"]);
        let Err(Usage(message)) = times(&given, 0, 1_000_000_000) else {
            panic!("accepted both");
        };
        assert!(
            message.contains("--frames") && message.contains("--rate"),
            "{message}"
        );
    }

    #[test]
    fn a_rate_too_fast_for_the_span_names_one_that_fits() {
        let given = options(&["verbs-zones", "--rate", "15"]);
        let Err(Usage(message)) = times(&given, 0, 2_000_000_000) else {
            panic!("accepted 30 frames");
        };
        assert!(
            message.contains("30 frames") && message.contains("--rate 12"),
            "{message}"
        );
    }
}
