//! How ticket IDs are spelled in output. The hub stores an ID as it is printed, a `T` (an `E`
//! for a root created as an epic, an `M` for a meta-ticket, an `A` for a ticket Ignite filed on
//! its own) and then capitals (`TQ44S.7`,
//! `EQ44S.7`), and accepts any case, with or without the letter, on input. A hub from
//! before that stored a bare lowercase ID (`q44s.7`), so an ID without its `T` still gets one.
//! Aliases from earlier trackers and anything a person wrote are printed as stored.

use serde_json::Value;

/// Removes the long free-form fields, keeping the IDs and which tickets are selected.
pub fn brief(value: &mut Value) {
    match value {
        Value::Array(values) => values.iter_mut().for_each(brief),
        Value::Object(fields) => {
            for key in [
                "body",
                "acceptance",
                "design",
                "notes",
                "comments",
                "history",
                "payload",
                "waiters",
            ] {
                fields.remove(key);
            }
            for key in ["tickets", "results", "ticket"] {
                if let Some(value) = fields.get_mut(key) {
                    brief(value);
                }
            }
        }
        _ => {}
    }
}

/// Spells every ticket ID in a hub reply with its `T`.
pub fn display(value: &mut Value) {
    match value {
        Value::Array(rows) => rows.iter_mut().for_each(display),
        Value::Object(row) => {
            for key in ["id", "parent", "src", "dst"] {
                if let Some(value) = row.get_mut(key) {
                    prefix(value);
                }
            }
            if let Some(ticket) = row.get_mut("ticket") {
                if ticket.is_object() {
                    display(ticket);
                } else {
                    prefix(ticket);
                }
            }
            for key in ["children", "closed", "ready"] {
                if let Some(Value::Array(ids)) = row.get_mut(key) {
                    for id in ids {
                        if id.is_object() {
                            display(id);
                        } else {
                            prefix(id);
                        }
                    }
                }
            }
            for key in [
                "tickets",
                "edges",
                "blocked_by",
                "comments",
                "results",
                "history",
                "waits_on",
                "waited_on_by",
            ] {
                if let Some(nested) = row.get_mut(key) {
                    display(nested);
                }
            }
            if let Some(Value::Object(change)) = row.get_mut("change") {
                for key in ["id", "parent"] {
                    if let Some(Value::Array(values)) = change.get_mut(key) {
                        values.iter_mut().for_each(prefix);
                    }
                }
            }
        }
        _ => {}
    }
}

/// One ticket ID spelled with its letter: `T`, or the `E`, `M`, `A` or `B` of an epic, a
/// meta-ticket, a ticket Ignite filed on its own or a bug report.
pub fn spelled(id: &str) -> String {
    let mut value = Value::String(id.to_owned());
    prefix(&mut value);
    value.as_str().unwrap_or(id).to_owned()
}

/// Adds the `T` an ID from before IDs were stored in capitals lacks. An epic's ID may start
/// with a capital `E` (`EQ44S`), a meta-ticket's with a capital `M` (`MAB12`) and an automatic
/// ticket's with a capital `A` (`AR2D2`) and a bug report's with a capital `B` (`BX9K2`), which no such older ID does, since those were
/// lowercase.
fn prefix(value: &mut Value) {
    if let Value::String(id) = value
        && !id.starts_with(['T', 'E', 'M', 'A', 'B'])
    {
        id.insert(0, 'T');
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ids_get_a_t_and_user_text_is_left_alone() {
        let mut output = json!({
            "id": "q44s.7", "parent": "q44s", "children": ["q44s.7.1"],
            "edges": [{"src": "q44s.7", "dst": "tccc"}],
            "blocked_by": [{"id": "tccc", "rule": 2}],
            "comments": [{"ticket": "q44s.7", "text": "q44s"}],
            "history": [{"ticket": "q44s.7", "change": {"parent": [null, "q44s"]}}],
            "aliases": ["by-moved.7"], "metadata": {"id": "keep"},
        });
        display(&mut output);
        assert_eq!(
            output,
            json!({
                "id": "Tq44s.7", "parent": "Tq44s", "children": ["Tq44s.7.1"],
                "edges": [{"src": "Tq44s.7", "dst": "Ttccc"}],
                "blocked_by": [{"id": "Ttccc", "rule": 2}],
                "comments": [{"ticket": "Tq44s.7", "text": "q44s"}],
                "history": [{"ticket": "Tq44s.7", "change": {"parent": [null, "Tq44s"]}}],
                "aliases": ["by-moved.7"], "metadata": {"id": "keep"},
            })
        );
    }

    #[test]
    fn meta_ticket_ids_keep_their_m() {
        assert_eq!(spelled("MAB12.3"), "MAB12.3");
    }

    #[test]
    fn automatic_ticket_ids_keep_their_a() {
        assert_eq!(spelled("AR2D2"), "AR2D2");
        assert_eq!(spelled("ar2d2"), "Tar2d2");
    }

    #[test]
    fn brief_drops_long_text_but_keeps_the_selection() {
        let mut value = json!([{"id": "a", "title": "t", "body": "long", "comments": [1]}]);
        brief(&mut value);
        assert_eq!(value, json!([{"id": "a", "title": "t"}]));
    }
}
