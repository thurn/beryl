//! How ticket IDs are spelled in output. The hub stores a bare ID such as `q44s.7`; the commands
//! print it with a leading `T` (`Tq44s.7`), and the hub accepts either spelling on input.
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

fn prefix(value: &mut Value) {
    if let Value::String(id) = value
        && !id.starts_with('T')
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
    fn brief_drops_long_text_but_keeps_the_selection() {
        let mut value = json!([{"id": "a", "title": "t", "body": "long", "comments": [1]}]);
        brief(&mut value);
        assert_eq!(value, json!([{"id": "a", "title": "t"}]));
    }
}
