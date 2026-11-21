//! The text each ticket verb prints. `--json` prints the hub's reply instead, so agents that
//! parse it see the shapes `tk` printed.

use std::fmt::Write as _;

use serde_json::Value;

fn text<'a>(value: &'a Value, key: &str) -> &'a str {
    value[key].as_str().unwrap_or("")
}

/// `Tq44s.7  P1  in_progress  Title  [label, label]`.
fn line(ticket: &Value) -> String {
    let labels: Vec<&str> = ticket["labels"]
        .as_array()
        .map(|labels| labels.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    let suffix = if labels.is_empty() {
        String::new()
    } else {
        format!("  [{}]", labels.join(", "))
    };
    format!(
        "{}  P{}  {}  {}{suffix}",
        text(ticket, "id"),
        ticket["priority"],
        text(ticket, "status"),
        text(ticket, "title"),
    )
}

/// One line per ticket, or `no tickets`.
pub fn list(tickets: &Value) -> String {
    match tickets.as_array() {
        Some(rows) if !rows.is_empty() => rows.iter().map(line).collect::<Vec<_>>().join("\n"),
        _ => "no tickets".to_owned(),
    }
}

/// The tickets that hold `ticket` back, other than itself. The hub lists a ticket that is not
/// open, or is deferred, as its own blocker, but its status line already says so, and
/// "blocked by" naming the ticket itself reads as a dependency loop.
fn other_blockers(ticket: &Value) -> Vec<&str> {
    let own = text(ticket, "id");
    ticket["blocked_by"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|blocker| text(blocker, "id"))
        .filter(|id| *id != own)
        .collect()
}

/// An alias as a person would write it. Aliases are stored in lowercase; one without a dash is
/// an ID the ticket had before it moved, shown in capitals as IDs are. One with a dash comes
/// from an earlier tracker and is shown as stored.
fn shown_alias(alias: &str) -> String {
    if alias.contains('-') {
        alias.to_owned()
    } else {
        alias.to_ascii_uppercase()
    }
}

/// A ticket with its text, edges and, when asked for, comments and history.
pub fn detail(ticket: &Value) -> String {
    let mut out = vec![line(ticket)];
    for (label, key) in [
        ("parent", "parent"),
        ("assignee", "assignee"),
        ("type", "type"),
    ] {
        if let Some(value) = ticket[key].as_str() {
            out.push(format!("{label}: {value}"));
        }
    }
    let aliases: Vec<String> = ticket["aliases"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(shown_alias)
        .collect();
    if !aliases.is_empty() {
        out.push(format!("formerly: {}", aliases.join(", ")));
    }
    if let Some(hours) = ticket["every_hours"].as_u64() {
        out.push(format!(
            "every: {hours}h (a meta-ticket; each copy is filed under it)"
        ));
    }
    if let Some(amount) = ticket["saving"]["amount"].as_f64() {
        out.push(format!(
            "saving: {amount:.1} {} a week ({})",
            text(&ticket["saving"], "unit"),
            text(&ticket["saving"], "rule")
        ));
    }
    if let Some(reason) = ticket["close_reason"].as_str() {
        out.push(format!("closed: {reason}"));
    }
    if ticket["ready"] == true {
        out.push("ready".to_owned());
    }
    if let Some(count) = ticket["releases"]["count"].as_u64() {
        out.push(format!(
            "releases: {count} counted since the ticket last changed"
        ));
    }
    if let Some(reason) = ticket["held_back"].as_str() {
        out.push(format!("held back: {reason}"));
    }
    let attachments: Vec<ignite_core::protocol::attachments::Attachment> =
        serde_json::from_value(ticket["attachments"].clone()).unwrap_or_default();
    if !attachments.is_empty() {
        out.push("attachments:".to_owned());
        out.extend(super::attach::lines(text(ticket, "id"), &attachments));
    }
    let blockers = other_blockers(ticket);
    if !blockers.is_empty() {
        out.push(format!("blocked by: {}", blockers.join(", ")));
    }
    out.extend(edges(ticket));
    for key in ["body", "acceptance", "design", "notes"] {
        let value = text(ticket, key);
        if !value.is_empty() {
            out.push(format!("\n{key}:\n{value}"));
        }
    }
    if let Some(comments) = ticket["comments"].as_array() {
        for comment in comments {
            out.push(format!(
                "\ncomment {} by {} at {}:\n{}",
                comment["n"],
                text(comment, "author"),
                text(comment, "at"),
                text(comment, "text")
            ));
        }
    }
    if let Some(history) = ticket["history"].as_array() {
        for change in history {
            out.push(format!(
                "history {} {} {}: {}",
                change["seq"],
                text(change, "at"),
                text(change, "actor"),
                change["change"]
            ));
        }
    }
    if let Some(next) = ticket["next"].as_str() {
        out.push(format!("next     {next}"));
    }
    out.join("\n")
}

/// `waits on: Ta (blocks), Tb (related)` and `waited on by: Tc (blocks)`, from the ticket's
/// edges in both directions. `ticket dep list` prints each one's status and title as well.
fn edges(ticket: &Value) -> Vec<String> {
    let own = text(ticket, "id");
    let mut waits_on = vec![];
    let mut waited_on_by = vec![];
    for edge in ticket["edges"].as_array().into_iter().flatten() {
        let (src, dst, kind) = (text(edge, "src"), text(edge, "dst"), text(edge, "kind"));
        if src == own {
            waits_on.push(format!("{dst} ({kind})"));
        } else {
            waited_on_by.push(format!("{src} ({kind})"));
        }
    }
    [("waits on", waits_on), ("waited on by", waited_on_by)]
        .into_iter()
        .filter(|(_, ids)| !ids.is_empty())
        .map(|(label, ids)| format!("{label}: {}", ids.join(", ")))
        .collect()
}

/// `ticket dep list`: the ticket, then one line per ticket it waits on or that waits on it.
pub fn dependencies(value: &Value) -> String {
    let mut out = vec![format!(
        "{}  {}  {}",
        text(value, "id"),
        text(value, "status"),
        text(value, "title")
    )];
    for (label, key) in [
        ("waits on    ", "waits_on"),
        ("waited on by", "waited_on_by"),
    ] {
        for other in value[key].as_array().into_iter().flatten() {
            out.push(format!(
                "{label}  {}  {}  {}  ({})",
                text(other, "id"),
                text(other, "status"),
                text(other, "title"),
                text(other, "kind")
            ));
        }
    }
    if out.len() == 1 {
        out.push("no dependencies".to_owned());
    }
    out.join("\n")
}

/// `ticket children --open`: one line per child, then its assignee, blockers and last comment
/// indented under it.
pub fn open_children(children: &Value) -> String {
    let rows = children.as_array().map(Vec::as_slice).unwrap_or_default();
    if rows.is_empty() {
        return "no open children".to_owned();
    }
    let mut out = vec![];
    for child in rows {
        out.push(line(child));
        if let Some(assignee) = child["assignee"].as_str() {
            out.push(format!("    assignee: {assignee}"));
        }
        let blockers = other_blockers(child);
        if !blockers.is_empty() {
            out.push(format!("    blocked by: {}", blockers.join(", ")));
        }
        if let Some(comment) = child["last_comment"].as_str() {
            out.push(format!("    last comment: {comment}"));
        }
    }
    out.join("\n")
}

/// What a write did: the tickets it touched, any ancestors a close closed with them, and any
/// ticket a close filed to verify an epic's acceptance instead of closing the epic.
pub fn written(verb: &str, reply: &Value) -> String {
    let ids: Vec<&str> = reply["results"]
        .as_array()
        .map(|rows| rows.iter().map(|row| text(row, "id")).collect())
        .unwrap_or_default();
    let mut out = format!("{verb} {}", ids.join(", "));
    let listed = |key: &str| -> Vec<&str> {
        reply["results"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|row| row[key].as_array().into_iter().flatten())
            .filter_map(Value::as_str)
            .collect()
    };
    let closed = listed("closed");
    if !closed.is_empty() {
        let _ = write!(out, "\nalso closed: {}", closed.join(", "));
    }
    let filed = listed("filed");
    if !filed.is_empty() {
        let _ = write!(
            out,
            "\nfiled to verify the epic's acceptance: {}",
            filed.join(", ")
        );
    }
    for added in reply["filer_dependencies"].as_array().into_iter().flatten() {
        let _ = write!(
            out,
            "\ndepends on {}, which you hold unlanded; add --independent to file without that",
            text(added, "on")
        );
    }
    out
}

pub fn graph(graph: &Value) -> String {
    let count = |key: &str| graph[key].as_array().map_or(0, Vec::len);
    format!(
        "{} tickets, {} edges, feed position {}",
        count("tickets"),
        count("edges"),
        graph["seq"]
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_held_back_ticket_says_how_often_it_was_released_and_why_it_waits() {
        let ticket = json!({"id": "Ta", "priority": 2, "status": "open", "title": "One",
            "releases": {"count": 2, "sessions": ["local_a", "local_b"]},
            "held_back": "released 2 times by 2 sessions (local_a, local_b), so ..."});
        let shown = detail(&ticket);
        assert!(shown.contains("\nreleases: 2 counted since the ticket last changed\n"));
        assert!(shown.contains("\nheld back: released 2 times by 2 sessions"));
    }

    #[test]
    fn a_list_prints_one_line_per_ticket() {
        let tickets = json!([
            {"id": "Ta", "priority": 1, "status": "open", "title": "One", "labels": ["x", "y"]},
            {"id": "Tb", "priority": 2, "status": "closed", "title": "Two"},
        ]);
        assert_eq!(
            list(&tickets),
            "Ta  P1  open  One  [x, y]\nTb  P2  closed  Two"
        );
        assert_eq!(list(&json!([])), "no tickets");
    }

    #[test]
    fn a_closed_ticket_is_not_shown_as_blocked_by_itself() {
        let ticket = json!({"id": "Ta", "priority": 0, "status": "closed", "title": "Done",
                            "blocked_by": [{"id": "Ta", "rule": 1}]});
        assert!(
            !detail(&ticket).contains("blocked by"),
            "{}",
            detail(&ticket)
        );
        let waiting = json!({"id": "Tb", "priority": 0, "status": "in_progress", "title": "W",
                             "blocked_by": [{"id": "Tb", "rule": 1}, {"id": "Tc", "rule": 2}]});
        assert!(
            detail(&waiting).lines().any(|l| l == "blocked by: Tc"),
            "{}",
            detail(&waiting)
        );
        let children = json!([waiting]);
        assert!(open_children(&children).contains("    blocked by: Tc"));
    }

    #[test]
    fn a_moved_ticket_lists_its_former_ids_in_capitals() {
        let ticket = json!({"id": "EME7A.30", "priority": 0, "status": "closed", "title": "Font",
                            "parent": "EME7A", "aliases": ["by-old", "tq44s.113"]});
        assert!(
            detail(&ticket)
                .lines()
                .any(|l| l == "formerly: by-old, TQ44S.113"),
            "{}",
            detail(&ticket)
        );
    }

    #[test]
    fn a_ticket_with_an_instruction_ends_with_its_next_line() {
        let ticket = json!({"id": "Ta", "priority": 1, "status": "pending_approval",
                            "title": "Look", "body": "Why is it slow?",
                            "next": "pending approval: run `ignite start Ta` before investigating it"});
        assert_eq!(
            detail(&ticket).lines().last(),
            Some("next     pending approval: run `ignite start Ta` before investigating it")
        );
    }

    #[test]
    fn a_write_names_the_tickets_and_the_ancestors_it_closed() {
        let reply = json!({"seq": 4, "results": [{"id": "Ta.1", "closed": ["Ta"]}]});
        assert_eq!(written("closed", &reply), "closed Ta.1\nalso closed: Ta");
    }

    #[test]
    fn a_write_names_the_ticket_filed_to_verify_an_epic() {
        let reply = json!({"seq": 4, "results": [{"id": "Ta.1", "filed": ["Ta.2"]}]});
        assert_eq!(
            written("closed", &reply),
            "closed Ta.1\nfiled to verify the epic's acceptance: Ta.2"
        );
    }
}
