//! `list` and `info`: the names an agent pastes into the other commands, a recording's and a
//! moment's.

use std::fmt::Write;

use app_client::driver::{Duration, Moment, Name, Offset, Page, StepMoment};
use game_probe::agent::steps::card_of;
use kit_frame::frame::Role;
use kit_probe::agent::tree::locator_of;
use serde_json::{Value, json};

use super::options::{self, Options, Usage, quote, seat_word, side_of};
use super::{Printed, milliseconds, open, open_recording};
use crate::{scene, stories};

/// How many moments `info` prints before it cuts the rest, unless `--full`, so that with its
/// header and its closing lines it prints at most 100.
const MOST_MOMENTS: usize = 95;
/// How many recordings `list` prints before it cuts the rest, unless `--full`.
const MOST_RECORDINGS: usize = 95;

pub(super) fn list(options: &Options) -> Result<Printed, Usage> {
    let prefix = match options.rest.as_slice() {
        [] => "",
        [prefix] => prefix.as_str(),
        _ => {
            return Err(Usage(format!(
                "list takes at most one prefix, not {}",
                options.rest.join(" ")
            )));
        }
    };
    let listed = scene::list(prefix).map_err(Usage)?;
    if listed.is_empty() {
        let names = scene::names(prefix).map_err(Usage)?;
        let mut unique: Vec<&str> = names.iter().map(|(n, _)| n.as_str()).collect();
        unique.dedup();
        return Err(Usage(format!(
            "no recording's name starts with '{prefix}'. The nearest names are:\n{}",
            options::nearest(prefix, &unique)
                .iter()
                .map(|n| format!("  {n}"))
                .collect::<Vec<_>>()
                .join("\n")
        )));
    }
    if options.json {
        let rows: Vec<Value> = listed
            .iter()
            .map(|r| {
                json!({
                    "name": r.name,
                    "seat": seat_word(side_of(r.seat)),
                    "length_ms": r.length_ms,
                    "views": r.views,
                    "failure": r.failure,
                })
            })
            .collect();
        let value = json!({ "command": options.command_line(), "recordings": rows });
        return Ok(Printed::text(format!("{value}\n")));
    }
    let mut out = format!("{}\n", options.command_line());
    let includes_story = listed
        .iter()
        .any(|row| row.name.starts_with(stories::PREFIX));
    let shown = if options.switch("--full") {
        listed.len()
    } else {
        listed.len().min(MOST_RECORDINGS)
    };
    let width = listed[..shown]
        .iter()
        .map(|r| r.name.len())
        .max()
        .unwrap_or(0);
    for row in &listed[..shown] {
        let seat = format!("--seat {}", seat_word(side_of(row.seat)));
        let length = match (&row.failure, row.length_ms) {
            (Some(why), _) if row.name.starts_with(stories::PREFIX) => why.clone(),
            (None, Some(ms)) if row.name.starts_with(stories::PREFIX) => {
                format!("{ms}ms, story")
            }
            (Some(why), _) => format!("not recorded: {why}"),
            (None, Some(ms)) => format!("{ms}ms, {} views", row.views),
            (None, None) => format!("does not play, {} views", row.views),
        };
        let _ = writeln!(out, "{:<width$}  {seat:<15}  {length}", row.name);
    }
    if shown < listed.len() {
        let _ = writeln!(
            out,
            "... {} more (just b3d list {}--full, or a longer prefix)",
            listed.len() - shown,
            if prefix.is_empty() {
                String::new()
            } else {
                format!("{} ", quote(prefix))
            }
        );
    }
    let _ = writeln!(
        out,
        "{} {}. `just b3d info <name>` lists one's moments.",
        listed.len(),
        if includes_story {
            "scenes"
        } else {
            "recordings"
        }
    );
    Ok(Printed::text(out))
}

/// One moment as `info` lists it.
struct Listed {
    moment: String,
    at: u64,
    what: String,
    /// For `--json`.
    value: Value,
}

pub(super) fn info(options: &Options) -> Result<Printed, Usage> {
    if stories::is_story(options.name()) && !stories::is_recording_story(options.name()) {
        return story_info(options);
    }
    let mut page = open_recording(options)?;
    let moments = page.moments();
    let steps = page.steps();
    let views = moments
        .iter()
        .filter(|(name, _)| matches!(name, Name::View(_)))
        .count();
    let prompts: Vec<(Name, u64)> = moments
        .iter()
        .filter(|(name, _)| matches!(name, Name::Prompt(_)))
        .map(|&(name, at)| (name, at.0))
        .collect();
    let end = moments.last().map_or(0, |(_, at)| at.0);

    let mut listed = Vec::new();
    for &(name, at) in &moments {
        // Prompts and steps are listed below, with their buttons and their spans.
        if matches!(name, Name::Prompt(_) | Name::Step(..)) {
            continue;
        }
        listed.push(Listed {
            moment: name.to_string(),
            at: at.0,
            what: String::new(),
            value: json!({ "moment": name.to_string(), "t_ms": milliseconds(at.0) }),
        });
    }
    listed.extend(step_rows(&mut page, &steps)?);
    listed.extend(prompt_rows(&mut page, &prompts)?);
    // In time order; at one time, the order above: a view, its steps, then its prompt.
    listed.sort_by_key(|row| row.at);

    let summary = format!(
        "{}: {views} views, {} prompts, {} steps, ends at {}ms",
        options.name(),
        prompts.len(),
        steps.len(),
        milliseconds(end)
    );
    if options.json {
        let rows: Vec<Value> = listed.into_iter().map(|row| row.value).collect();
        // The interface tests' check times (`Page::check_times`), rounded up to whole
        // milliseconds so that `--at <n>ms` lands on or just after each one.
        let checks: Vec<u64> = (page.check_times().into_iter())
            .map(|at| at.div_ceil(1_000_000))
            .collect();
        let value = json!({
            "command": options.command_line(),
            "recording": options.name(),
            "views": views,
            "prompts": prompts.len(),
            "steps": steps.len(),
            "end_ms": milliseconds(end),
            "moments": rows,
            "check_times_ms": checks,
        });
        return Ok(Printed::text(format!("{value}\n")));
    }
    let mut out = format!("{}\n{summary}\n", options.command_line());
    let shown = if options.switch("--full") {
        listed.len()
    } else {
        listed.len().min(MOST_MOMENTS)
    };
    let width = listed[..shown]
        .iter()
        .map(|row| row.moment.len())
        .max()
        .unwrap_or(0);
    for row in &listed[..shown] {
        let at = format!("{}ms", milliseconds(row.at));
        let line = format!("{:<width$}  {at:>8}  {}", row.moment, row.what);
        let _ = writeln!(out, "{}", line.trim_end());
    }
    if shown < listed.len() {
        let _ = writeln!(
            out,
            "... {} more moments ({} --full)",
            listed.len() - shown,
            options.command_line()
        );
    }
    let _ = writeln!(
        out,
        "Each line starts with what --at takes, as in: just b3d tree {} --at {}",
        quote(options.name()),
        listed[..shown]
            .last()
            .map_or("end", |row| row.moment.as_str())
    );
    Ok(Printed::text(out))
}

/// Each script step's moment with its verb, the card it moves and its span.
fn step_rows(page: &mut Page, steps: &[StepMoment]) -> Result<Vec<Listed>, Usage> {
    let mut listed = Vec::new();
    for step in steps {
        let verb = format!("{:?}", step.verb).to_lowercase();
        // What the step moves, as the tree names it halfway through, so a reader can find the
        // step that moves a given card.
        let subject = match step.subject {
            Some(slot) => {
                page.seek(&Moment::At(Duration(step.middle)))
                    .map_err(|error| Usage(error.to_string()))?;
                let frame = page.frame();
                card_of(frame, slot).map(|index| locator_of(frame, index))
            }
            None => None,
        };
        listed.push(Listed {
            moment: step.name.to_string(),
            at: step.middle,
            what: format!(
                "{verb}{} from {}ms to {}ms",
                subject.as_ref().map_or(String::new(), |s| format!(" {s}")),
                milliseconds(step.from),
                milliseconds(step.to)
            ),
            value: json!({
                "moment": step.name.to_string(),
                "t_ms": milliseconds(step.middle),
                "verb": verb,
                "subject": subject,
                "from_ms": milliseconds(step.from),
                "to_ms": milliseconds(step.to),
            }),
        });
    }
    Ok(listed)
}

/// Each prompt's moment with the buttons on screen when it is open and still.
fn prompt_rows(page: &mut Page, prompts: &[(Name, u64)]) -> Result<Vec<Listed>, Usage> {
    let mut listed = Vec::new();
    for &(name, at) in prompts {
        page.seek(&Moment::Named {
            name,
            offset: Offset::default(),
        })
        .map_err(|error| Usage(error.to_string()))?;
        let frame = page.frame();
        let buttons: Vec<String> = (0..frame.resolved.len() as u32)
            .filter(|&index| frame.resolved[index as usize].role == Role::Button)
            .map(|index| locator_of(frame, index))
            .collect();
        listed.push(Listed {
            moment: name.to_string(),
            at,
            what: if buttons.is_empty() {
                "no buttons".to_owned()
            } else {
                buttons.join(", ")
            },
            value: json!({
                "moment": name.to_string(),
                "t_ms": milliseconds(at),
                "buttons": buttons,
            }),
        });
    }
    Ok(listed)
}

fn story_info(options: &Options) -> Result<Printed, Usage> {
    let mut page = open(options)?;
    let moments = page.moments();
    if options.json {
        let rows: Vec<Value> = moments
            .iter()
            .map(|(name, at)| json!({ "moment": name.to_string(), "t_ms": milliseconds(at.0) }))
            .collect();
        let checks: Vec<u64> = page.check_times().into_iter().map(milliseconds).collect();
        let end = moments.last().map_or(0, |(_, at)| milliseconds(at.0));
        return Ok(Printed::text(format!(
            "{}\n",
            json!({
                "command": options.command_line(), "recording": options.name(),
                "views": 0, "prompts": 0, "steps": 0, "end_ms": end,
                "moments": rows, "check_times_ms": checks,
            })
        )));
    }
    let mut out = format!("{}\n", options.command_line());
    for (name, at) in moments {
        let _ = writeln!(out, "{name}  {}ms", milliseconds(at.0));
    }
    Ok(Printed::text(out))
}
