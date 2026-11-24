//! Explains one node selected by the same locator grammar as the tree.

use std::fmt::Write;

use kit_probe::agent::locate;
use kit_probe::agent::tree::line;
use serde_json::Value;

use super::{Options, Printed, Usage, open, seek_and_act};

pub(super) fn run(options: &Options) -> Result<Printed, Usage> {
    let [locator] = options.rest.as_slice() else {
        return Err(Usage(
            "give one quoted locator, as in 'button \"Auto Pay\"'".to_owned(),
        ));
    };
    let mut scene = open(options)?;
    scene.enable_explanation();
    seek_and_act(&mut scene, options, &options.at)?;
    let frame = scene.frame();
    let index = locate(frame, locator).map_err(|error| Usage(error.to_string()))?;
    let element = frame.resolved[index as usize];
    let line = line(frame, index);
    let mut explanation = scene.explain(&element);
    let command = options.command_line();
    if options.json {
        explanation["command"] = command.into();
        explanation["line"] = line.into();
        return Ok(Printed::text(format!("{explanation}\n")));
    }

    let mut out = format!("{command}\n{line}\n");
    let place = &explanation["place"];
    let _ = writeln!(out, "Place");
    write_fields(&mut out, place, &["attempts", "layout", "attempt_count"]);
    if let Some(layout) = place["layout"].as_object() {
        let _ = writeln!(out, "  Parent layout");
        for (name, value) in layout {
            let _ = writeln!(out, "    {name} = {}", text(value));
        }
    }
    if let Some(attempts) = place["attempts"].as_array() {
        for attempt in attempts {
            let slot = text(&attempt["slot"]);
            let bounds = &attempt["bounds"];
            let reason = if attempt["selected"] == true {
                "selected"
            } else {
                "rejected"
            };
            let _ = writeln!(out, "  {slot} {bounds} {reason}");
            if let Some(blockers) = attempt["blocked_by"].as_array() {
                for blocker in blockers {
                    let _ = writeln!(
                        out,
                        "    would cover {} #{} {}",
                        text(&blocker["group"]),
                        blocker["id"],
                        blocker["bounds"]
                    );
                }
                let count = attempt["blockers"].as_u64().unwrap_or_default();
                if count > blockers.len() as u64 {
                    let _ = writeln!(out, "    … {count} blockers total");
                }
            }
        }
        let count = place["attempt_count"].as_u64().unwrap_or_default();
        if count > attempts.len() as u64 {
            let _ = writeln!(out, "  … {count} attempts total");
        }
    }
    let _ = writeln!(out, "Look");
    if let Some(styles) = explanation["look"].as_array() {
        for style in styles {
            let source = &style["source"];
            let _ = writeln!(
                out,
                "  {} = {} ({}, {})",
                text(&style["property"]),
                text(&style["value"]),
                text(&source["key"]),
                text(&source["file"])
            );
            if let Some(other) = style["also_from"].as_array() {
                for source in other {
                    let _ = writeln!(
                        out,
                        "    also from {} ({})",
                        text(&source["key"]),
                        text(&source["file"])
                    );
                }
            }
        }
    }
    if let Some(motion) = explanation["motion"]
        .as_array()
        .filter(|motion| !motion.is_empty())
    {
        let _ = writeln!(out, "Motion");
        for cause in motion {
            let _ = writeln!(out, "  {}", text(cause));
        }
    }
    let _ = writeln!(out, "Owner");
    write_fields(&mut out, &explanation["owner"], &[]);
    Ok(Printed::text(out))
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn write_fields(out: &mut String, value: &Value, skip: &[&str]) {
    if let Some(object) = value.as_object() {
        for (name, value) in object {
            if !value.is_null() && !skip.contains(&name.as_str()) {
                let _ = writeln!(out, "  {name} = {}", text(value));
            }
        }
    }
}
