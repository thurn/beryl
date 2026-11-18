//! A ticket's changelog as a timeline: each stored change in words, with runs of identical
//! entries merged and runs of bookkeeping changes folded into one line.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::Value;

use super::common::{person, priority_chip, status_dot};
use super::components::{self as c, Tone, esc};
use super::data::{HistoryRow, TicketRow, parse_status};

/// A JSON object whose members keep the order they were written in, which `serde_json`'s map
/// does not, and which decides the order of the words in an entry.
pub struct Ordered(pub Vec<(String, Value)>);

impl<'de> Deserialize<'de> for Ordered {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Members;
        impl<'de> Visitor<'de> for Members {
            type Value = Ordered;
            fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut access: A) -> Result<Ordered, A::Error> {
                let mut members = Vec::new();
                while let Some(entry) = access.next_entry::<String, Value>()? {
                    members.push(entry);
                }
                Ok(Ordered(members))
            }
        }
        deserializer.deserialize_map(Members)
    }
}

struct Entry {
    actor: String,
    at: String,
    what: String,
    detail: Option<String>,
    quiet: bool,
}

fn field_words(field: &str) -> Option<&'static str> {
    Some(match field {
        "body" | "description" => "edited the description",
        "acceptance" | "acceptance_criteria" => "edited the acceptance criteria",
        "design" => "edited the design",
        "notes" => "edited the notes",
        "type" | "issue_type" => "changed the type",
        "parent" => "changed the parent",
        "defer_until" => "changed the deferral",
        "last" => "changed when this ticket runs",
        _ => return None,
    })
}

const BOOKKEEPING: [&str; 5] = [
    "metadata",
    "updated_at",
    "version",
    "next_child",
    "closed_at",
];

/// JavaScript's `String(value)`: strings as they are, other values as JSON.
fn text(value: &Value) -> String {
    value
        .as_str()
        .map_or_else(|| value.to_string(), str::to_owned)
}

fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null | Value::Bool(false)) => false,
        Some(Value::String(text)) => !text.is_empty(),
        Some(Value::Number(number)) => number.as_f64().is_some_and(|number| number != 0.0),
        Some(_) => true,
    }
}

fn status(value: &Value) -> String {
    match value.as_str() {
        Some(name)
            if [
                "open",
                "in_progress",
                "blocked",
                "deferred",
                "closed",
                "pending_approval",
            ]
            .contains(&name) =>
        {
            status_dot(parse_status(name), true)
        }
        _ => esc(&text(value)),
    }
}

fn number(value: &Value) -> i64 {
    value
        .as_i64()
        .or_else(|| value.as_str().and_then(|text| text.trim().parse().ok()))
        .unwrap_or(0)
}

fn get<'a>(delta: &'a [(String, Value)], key: &str) -> Option<&'a Value> {
    delta
        .iter()
        .find(|(name, _)| name == key)
        .map(|(_, value)| value)
}

fn label_chip(label: &str) -> String {
    c::chip(label, Tone::Muted, None)
}

/// Describes one stored change. `kind` is set for an imported event, whose fields hold plain
/// values; a change this system wrote holds `[old, new]` pairs.
fn describe(actor: &str, at: &str, changed: &[(String, Value)], kind: Option<&str>) -> Entry {
    let unwrapped: Vec<(String, Value)>;
    let delta: &[(String, Value)] = if kind.is_some() {
        changed
    } else {
        unwrapped = changed
            .iter()
            .map(|(field, value)| {
                let value = match value {
                    Value::Array(pair) if pair.len() == 2 => pair[1].clone(),
                    other => other.clone(),
                };
                (field.clone(), value)
            })
            .collect();
        &unwrapped
    };
    let entry = |what: String, detail: Option<String>, quiet: bool| Entry {
        actor: actor.to_owned(),
        at: at.to_owned(),
        what,
        detail,
        quiet,
    };
    if get(delta, "id").is_some() {
        return entry("created this".into(), None, false);
    }
    let assignee = get(delta, "assignee");
    let closing = get(delta, "status").and_then(Value::as_str) == Some("closed");
    let claiming = kind == Some("claimed")
        || get(delta, "status").and_then(Value::as_str) == Some("in_progress");
    let action = if kind == Some("claimed") && !truthy(assignee) {
        "claimed this".to_owned()
    } else if closing {
        "closed this".to_owned()
    } else if claiming && truthy(assignee) {
        format!("claimed this for {}", person(assignee.map(text).as_deref()))
    } else {
        String::new()
    };
    let mut parts: Vec<String> = if action.is_empty() {
        Vec::new()
    } else {
        vec![action.clone()]
    };
    for (field, value) in delta {
        if BOOKKEEPING.contains(&field.as_str()) {
            continue;
        }
        match field.as_str() {
            "status" => {
                if action.is_empty() {
                    parts.push(format!("changed status to {}", status(value)));
                }
            }
            "priority" => parts.push(format!("set priority to {}", priority_chip(number(value)))),
            "assignee" => {
                if action.is_empty() {
                    parts.push(if truthy(Some(value)) {
                        format!("assigned {}", person(Some(&text(value))))
                    } else {
                        "unassigned it".to_owned()
                    });
                }
            }
            "title" => parts.push(format!("renamed it to “{}”", esc(&text(value)))),
            "labels" if value.is_array() => {
                let now: Vec<&str> = value
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                let before: Vec<&str> = get(changed, "labels")
                    .and_then(Value::as_array)
                    .and_then(|pair| pair.first())
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .collect();
                for added in now.iter().filter(|label| !before.contains(label)) {
                    parts.push(format!("added label {}", label_chip(added)));
                }
                for removed in before.iter().filter(|label| !now.contains(label)) {
                    parts.push(format!("removed label {}", label_chip(removed)));
                }
            }
            "close_reason" => {}
            other => parts.push(field_words(other).map_or_else(
                || format!("changed {}", esc(&other.replace('_', " "))),
                str::to_owned,
            )),
        }
    }
    let detail = get(delta, "close_reason")
        .and_then(Value::as_str)
        .filter(|detail| !detail.is_empty())
        .map(str::to_owned);
    let quiet = parts.is_empty() && detail.is_none();
    entry(parts.join(", "), detail, quiet)
}

fn describe_imported(event: &Value) -> Entry {
    let string = |key: &str| {
        event
            .get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned()
    };
    let actor = string("actor");
    let at = string("created_at");
    let kind = event
        .get("event_type")
        .and_then(Value::as_str)
        .unwrap_or("event")
        .to_owned();
    let mut detail = string("comment");
    let mut fields: Vec<(String, Value)> = Vec::new();
    if let Some(source) = event.get("new_value").and_then(Value::as_str) {
        if source.starts_with('{') {
            match serde_json::from_str::<Ordered>(source) {
                Ok(object) => fields = object.0,
                // Older stores sometimes recorded prose instead of a JSON object.
                Err(_) if detail.is_empty() => source.clone_into(&mut detail),
                Err(_) => {}
            }
        } else if detail.is_empty() {
            source.clone_into(&mut detail);
        }
    }
    let detail_opt = (!detail.is_empty()).then(|| detail.clone());
    let plain = |what: String, detail: Option<String>| Entry {
        actor: actor.clone(),
        at: at.clone(),
        what,
        detail,
        quiet: false,
    };
    match kind.as_str() {
        "created" => plain("created this".into(), None),
        "started" => plain("started work".into(), None),
        "closed" => plain("closed this".into(), detail_opt),
        "reopened" => plain("reopened this".into(), None),
        "label_added" | "label_removed" => {
            let action = if kind == "label_added" {
                "added"
            } else {
                "removed"
            };
            let label = detail
                .split_once(':')
                .map_or(detail.as_str(), |(_, rest)| rest)
                .trim();
            plain(format!("{action} label {}", label_chip(label)), None)
        }
        "claimed" | "updated" => describe(&actor, &at, &fields, Some(&kind)),
        _ => plain(esc(&kind.replace('_', " ")), detail_opt),
    }
}

fn entries_of(row: &HistoryRow) -> Vec<Entry> {
    if let Ok(Value::Array(events)) = serde_json::from_str::<Value>(&row.change) {
        let mut entries: Vec<Entry> = events.iter().map(describe_imported).collect();
        entries.sort_by(|left, right| right.at.cmp(&left.at));
        return entries;
    }
    let object = serde_json::from_str::<Ordered>(&row.change)
        .map(|object| object.0)
        .unwrap_or_default();
    vec![describe(&row.actor, &row.at, &object, None)]
}

/// The timeline of a ticket's history. `link` makes ticket names in a detail into links, and
/// `started` is when an imported ticket began, for a ticket with no recorded history.
pub fn timeline(
    ticket: &TicketRow,
    history: &[HistoryRow],
    link: &dyn Fn(&str) -> String,
    started: &str,
) -> String {
    let mut entries: Vec<Entry> = history.iter().flat_map(entries_of).collect();
    let recorded = !entries.is_empty();
    if !recorded {
        let plain = |actor: &str, at: &str, what: &str, detail: Option<String>| Entry {
            actor: actor.to_owned(),
            at: at.to_owned(),
            what: what.to_owned(),
            detail,
            quiet: false,
        };
        if let Some(closed_at) = &ticket.closed_at {
            let reason = ticket
                .close_reason
                .clone()
                .filter(|reason| !reason.is_empty());
            entries.push(plain("", closed_at, "closed this", reason));
        }
        if !started.is_empty() {
            entries.push(plain("", started, "started work", None));
        }
        entries.push(plain(
            &ticket.created_by,
            &ticket.created_at,
            "created this",
            None,
        ));
    }
    let mut lines: Vec<String> = Vec::new();
    let mut index = 0;
    while index < entries.len() {
        let event = &entries[index];
        let mut end = index + 1;
        while let Some(next) = entries.get(end) {
            let breaks = if event.quiet {
                !next.quiet
            } else {
                next.quiet
                    || event.detail.is_some()
                    || next.detail.is_some()
                    || next.actor != event.actor
                    || next.what != event.what
            };
            if breaks {
                break;
            }
            end += 1;
        }
        let count = end - index;
        let times = if count > 1 {
            format!(" <span class=\"c-muted\">×{count}</span>")
        } else {
            String::new()
        };
        let detail = event.detail.as_deref().map_or(String::new(), |detail| {
            format!(
                "<span class=\"c-timeline-detail\">{}</span>",
                link(&esc(detail))
            )
        });
        let body = if event.quiet {
            format!(
                "{count} bookkeeping {} by Ignite",
                if count == 1 { "update" } else { "updates" }
            )
        } else {
            format!("{}{times}{detail}", event.what)
        };
        let who = if event.quiet || event.actor.is_empty() {
            String::new()
        } else {
            person(Some(&event.actor))
        };
        lines.push(c::timeline_entry(
            &body,
            &c::time(Some(&event.at), ""),
            &who,
            event.quiet,
        ));
        index = end;
    }
    let note = if recorded {
        String::new()
    } else {
        c::empty("No events were recorded. These dates come from the ticket.")
    };
    c::timeline(lines) + &note
}
