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
    if let Some(reason) = ticket["close_reason"].as_str() {
        out.push(format!("closed: {reason}"));
    }
    if ticket["ready"] == true {
        out.push("ready".to_owned());
    }
    if let Some(blockers) = ticket["blocked_by"].as_array().filter(|b| !b.is_empty()) {
        let ids: Vec<&str> = blockers.iter().map(|b| text(b, "id")).collect();
        out.push(format!("blocked by: {}", ids.join(", ")));
    }
    if let Some(edges) = ticket["edges"].as_array() {
        for edge in edges {
            out.push(format!(
                "edge: {} -> {} ({})",
                text(edge, "src"),
                text(edge, "dst"),
                text(edge, "kind")
            ));
        }
    }
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
    out.join("\n")
}

/// What a write did: the tickets it touched, and any ancestors a close closed with them.
pub fn written(verb: &str, reply: &Value) -> String {
    let ids: Vec<&str> = reply["results"]
        .as_array()
        .map(|rows| rows.iter().map(|row| text(row, "id")).collect())
        .unwrap_or_default();
    let mut out = format!("{verb} {}", ids.join(", "));
    let closed: Vec<&str> = reply["results"]
        .as_array()
        .into_iter()
        .flatten()
        .flat_map(|row| row["closed"].as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .collect();
    if !closed.is_empty() {
        let _ = write!(out, "\nalso closed: {}", closed.join(", "));
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
    fn a_write_names_the_tickets_and_the_ancestors_it_closed() {
        let reply = json!({"seq": 4, "results": [{"id": "Ta.1", "closed": ["Ta"]}]});
        assert_eq!(written("closed", &reply), "closed Ta.1\nalso closed: Ta");
    }
}
