//! `list` and `info`: the names an agent pastes into the other commands, a recording's and a
//! moment's.

use std::fmt::Write;

use beryl_scene::driver::{Moment, Name, Offset, Page};
use beryl_scene::frame::Role;
use serde_json::{Value, json};

use super::options::{self, Options, Usage, quote, seat_word, side_of};
use super::{Printed, milliseconds, open};
use crate::recordings;

/// How many lines `info` prints before it cuts the rest, unless `--full`.
const MOST_MOMENTS: usize = 200;

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
    let listed = recordings::list(prefix).map_err(Usage)?;
    if listed.is_empty() {
        let names = recordings::names().map_err(Usage)?;
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
    let width = listed.iter().map(|r| r.name.len()).max().unwrap_or(0);
    for row in &listed {
        let seat = format!("--seat {}", seat_word(side_of(row.seat)));
        let length = match (&row.failure, row.length_ms) {
            (Some(why), _) => format!("not recorded: {why}"),
            (None, Some(ms)) => format!("{ms}ms, {} views", row.views),
            (None, None) => format!("does not play, {} views", row.views),
        };
        let _ = writeln!(out, "{:<width$}  {seat:<15}  {length}", row.name);
    }
    let _ = writeln!(
        out,
        "{} recordings. `just b3d info <name>` lists one's moments.",
        listed.len()
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
    let mut page = open(options)?;
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
    for step in &steps {
        let verb = format!("{:?}", step.verb).to_lowercase();
        listed.push(Listed {
            moment: step.name.to_string(),
            at: step.middle,
            what: format!(
                "{verb} from {}ms to {}ms",
                milliseconds(step.from),
                milliseconds(step.to)
            ),
            value: json!({
                "moment": step.name.to_string(),
                "t_ms": milliseconds(step.middle),
                "verb": verb,
                "from_ms": milliseconds(step.from),
                "to_ms": milliseconds(step.to),
            }),
        });
    }
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
        let value = json!({
            "command": options.command_line(),
            "recording": options.name(),
            "views": views,
            "prompts": prompts.len(),
            "steps": steps.len(),
            "end_ms": milliseconds(end),
            "moments": rows,
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
        listed.last().map_or("end", |row| row.moment.as_str())
    );
    Ok(Printed::text(out))
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
            .map(|index| beryl_scene::agent::tree::locator_of(frame, index))
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
