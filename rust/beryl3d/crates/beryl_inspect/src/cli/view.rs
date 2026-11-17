//! `tree` and `diff`: the elements on screen at one moment, and what changed between two.

use std::collections::BTreeMap;
use std::fmt::Write;

use beryl_scene::agent::fault::rect_text;
use beryl_scene::agent::hit::bounds;
use beryl_scene::agent::tree::{line as tree_line, place_word};
use beryl_scene::agent::{TreeOptions, tree as tree_text};
use beryl_scene::driver::{Duration, Moment, Name, Offset, Page};
use beryl_scene::frame::{Frame, Role};
use serde_json::{Value, json};

use super::line::{Element, Line};
use super::options::{Options, Usage};
use super::{Printed, act, milliseconds, open, seek_and_act, time_of};

/// How many changes `diff` prints before it cuts the rest, unless `--full`.
const MOST_CHANGES: usize = 60;

/// The line `tree` prints under the command: where in the recording the page is and how many
/// elements are on screen, as `view 6 of 36, t=1500ms, animating, 48 elements`.
pub fn position_line(page: &mut Page) -> String {
    let (view, views, animating) = page.position();
    let now = page.now();
    let elements = page.frame().resolved.len();
    let state = if animating { "animating" } else { "at rest" };
    format!(
        "view {view} of {views}, t={}ms, {state}, {elements} elements",
        milliseconds(now)
    )
}

pub(super) fn tree(options: &Options) -> Result<Printed, Usage> {
    let role = match options.value("--role") {
        None => None,
        Some(word) => Some(Role::from_word(word).ok_or_else(|| {
            let words: Vec<&str> = Role::ALL.iter().map(|role| role.word()).collect();
            Usage(format!(
                "--role {word}: not a role; the roles are {}",
                words.join(", ")
            ))
        })?),
    };
    let tree_options = TreeOptions {
        within: options.value("--within").map(str::to_owned),
        role,
        full: options.switch("--full"),
    };
    let mut page = open(options)?;
    seek_and_act(&mut page, options, &options.at)?;
    if options.switch("--records") {
        return Ok(records(&mut page, options));
    }
    let (view, views, animating) = page.position();
    let now = page.now();
    let frame = page.frame();
    let elements = frame.resolved.len();
    let body =
        tree_text(frame, &tree_options).map_err(|error| Usage(format!("--within: {error}")))?;
    let header = position_line(&mut page);
    if options.json {
        let lines: Vec<Value> = body.lines().map(line_value).collect();
        let value = json!({
            "command": options.command_line(),
            "view": view,
            "views": views,
            "t_ms": milliseconds(now),
            "animating": animating,
            "elements": elements,
            "lines": lines,
        });
        return Ok(Printed::text(format!("{value}\n")));
    }
    Ok(Printed::text(format!(
        "{}\n{header}\n{body}",
        options.command_line()
    )))
}

/// `tree --records`: the moment's position and every element as a record, as one JSON object.
/// `view_given_ms` is when the client was given the latest view, from which the web side counts
/// how far into that view's animation the moment lies.
fn records(page: &mut Page, options: &Options) -> Printed {
    page.set_looking(true);
    let (view, views, animating) = page.position();
    let now = page.now();
    let given = page.view_given_at();
    let frame = page.frame();
    let value = json!({
        "command": options.command_line(),
        "view": view,
        "views": views,
        "t_ms": milliseconds(now),
        "view_given_ms": given.map(milliseconds),
        "animating": animating,
        "scale": options.scale,
        "elements": super::records::records(frame),
    });
    Printed::text(format!("{value}\n"))
}

/// A tree line as JSON: its text, and its fields as the grammar reads them.
fn line_value(text: &str) -> Value {
    match Line::parse(text) {
        Ok(Line::Group { depth, word }) => json!({ "depth": depth, "group": word }),
        Ok(Line::Cut {
            depth,
            count,
            place,
        }) => json!({ "depth": depth, "more": count, "place": place }),
        Ok(Line::Element { depth, element }) => element_value(depth, text.trim_start(), &element),
        Err(why) => json!({ "text": text, "error": why }),
    }
}

fn element_value(depth: usize, text: &str, element: &Element) -> Value {
    json!({
        "depth": depth,
        "line": text,
        "role": element.role,
        "name": element.name,
        "id": element.id,
        "place": element.place,
        "x": element.x,
        "y": element.y,
        "width": element.width,
        "height": element.height,
        "states": element.states,
    })
}

/// One element of a frame, as `diff` compares it.
struct Seen {
    /// Its tree line.
    line: String,
    /// Its place and box, as the tree prints them.
    place: String,
    rect: String,
    /// Its states, as the tree prints them.
    states: String,
}

fn seen(frame: &Frame) -> BTreeMap<(String, u32), Seen> {
    let mut out = BTreeMap::new();
    for (index, element) in frame.resolved.iter().enumerate() {
        let line = tree_line(frame, index as u32);
        let place = place_word(element.place);
        let rect = rect_text(bounds(element));
        let states = match line.find(&rect) {
            Some(at) => line[at + rect.len()..].trim().to_owned(),
            None => String::new(),
        };
        out.insert(
            (element.role.word().to_owned(), element.id.0),
            Seen {
                line,
                place,
                rect,
                states,
            },
        );
    }
    out
}

pub(super) fn diff(options: &Options) -> Result<Printed, Usage> {
    let moment = |flag: &str, name: Name| -> Result<Moment, Usage> {
        match options.value(flag) {
            None => Ok(Moment::Named {
                name,
                offset: Offset::default(),
            }),
            Some(text) => text
                .parse()
                .map_err(|error| Usage(format!("{flag} {text}: {error}"))),
        }
    };
    let from = moment("--from", Name::Start)?;
    let to = moment("--to", Name::End)?;
    let mut page = open(options)?;
    let from_time = time_of(&mut page, options, &from)?;
    let to_time = time_of(&mut page, options, &to)?;
    if to_time < from_time {
        return Err(Usage(format!(
            "--to {to} ({}ms) is before --from {from} ({}ms); give the earlier moment as --from",
            milliseconds(to_time),
            milliseconds(from_time)
        )));
    }
    page.seek(&Moment::At(Duration(from_time)))
        .map_err(|error| Usage(error.to_string()))?;
    let before = seen(page.frame());
    act(&mut page, options)?;
    // An action that runs the clock (a drag, a step) may already have passed `--to`.
    page.step(Duration(to_time.saturating_sub(page.now())));
    let after = seen(page.frame());

    let mut changes: Vec<(String, Value)> = Vec::new();
    for (key, now) in &after {
        let Some(was) = before.get(key) else {
            changes.push((
                format!("{}  (appeared)", now.line),
                json!({ "line": now.line, "change": "appeared" }),
            ));
            continue;
        };
        let mut what = Vec::new();
        if was.place != now.place || was.rect != now.rect {
            let place = if was.place.is_empty() {
                String::new()
            } else {
                format!("{} ", was.place)
            };
            what.push(format!("moved from {place}{}", was.rect));
        }
        if was.states != now.states {
            let states = if was.states.is_empty() {
                "no states"
            } else {
                was.states.as_str()
            };
            what.push(format!("states were: {states}"));
        }
        if !what.is_empty() {
            let what = what.join("; ");
            changes.push((
                format!("{}  ({what})", now.line),
                json!({ "line": now.line, "change": what }),
            ));
        }
    }
    for (key, was) in &before {
        if !after.contains_key(key) {
            changes.push((
                format!("{}  (left)", was.line),
                json!({ "line": was.line, "change": "left" }),
            ));
        }
    }

    if options.json {
        let rows: Vec<Value> = changes.into_iter().map(|(_, value)| value).collect();
        let value = json!({
            "command": options.command_line(),
            "from_ms": milliseconds(from_time),
            "to_ms": milliseconds(to_time),
            "changes": rows,
        });
        return Ok(Printed::text(format!("{value}\n")));
    }
    let mut out = format!("{}\n", options.command_line());
    let shown = if options.switch("--full") {
        changes.len()
    } else {
        changes.len().min(MOST_CHANGES)
    };
    for (line, _) in &changes[..shown] {
        let _ = writeln!(out, "{line}");
    }
    if shown < changes.len() {
        let _ = writeln!(
            out,
            "... {} more changes ({} --full)",
            changes.len() - shown,
            options.command_line()
        );
    }
    Ok(Printed::text(out))
}
