//! The second half of the migration that stores ticket IDs in capitals with a leading `T`
//! (`q44s.7` became `TQ44S.7`). `capital_ids.sql` rewrites the ID columns; this rewrites the
//! ticket names written inside ticket text and comments (`Tq44s.7`, the way the commands
//! printed an ID before), and the parents recorded in the changelog.

use std::collections::HashMap;

use rusqlite::Transaction;
use serde_json::Value;

use crate::Result;

/// Rewrites ticket names in free text and the changelog. Runs after `capital_ids.sql`, so the
/// `tickets` table already holds the new IDs; each one's old ID is its lowercase without the `T`.
pub fn rewrite_text(transaction: &Transaction<'_>) -> Result<()> {
    let renamed = renamed_ids(transaction)?;
    for column in ["title", "body", "acceptance", "design", "notes"] {
        rewrite_column(transaction, "tickets", "id", column, &renamed)?;
    }
    rewrite_comments(transaction, &renamed)?;
    rewrite_history(transaction, &renamed)?;
    Ok(())
}

/// Each ticket's old ID mapped to its new one.
fn renamed_ids(transaction: &Transaction<'_>) -> Result<HashMap<String, String>> {
    let mut statement = transaction.prepare("SELECT id FROM tickets")?;
    let ids = statement.query_map([], |row| row.get::<_, String>(0))?;
    let mut renamed = HashMap::new();
    for id in ids {
        let id = id?;
        if let Some(old) = id.strip_prefix('T') {
            renamed.insert(old.to_ascii_lowercase(), id.clone());
        }
    }
    Ok(renamed)
}

fn rewrite_column(
    transaction: &Transaction<'_>,
    table: &str,
    key: &str,
    column: &str,
    renamed: &HashMap<String, String>,
) -> Result<()> {
    let changed: Vec<(String, String)> = {
        let mut statement = transaction.prepare(&format!(
            "SELECT {key}, {column} FROM {table} WHERE instr({column}, 'T') > 0"
        ))?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut changed = Vec::new();
        for row in rows {
            let (id, text) = row?;
            let new = rewrite_mentions(&text, renamed);
            if new != text {
                changed.push((id, new));
            }
        }
        changed
    };
    let mut update = transaction.prepare(&format!(
        "UPDATE {table} SET {column} = ?2 WHERE {key} = ?1"
    ))?;
    for (id, text) in changed {
        update.execute([id, text])?;
    }
    Ok(())
}

fn rewrite_comments(
    transaction: &Transaction<'_>,
    renamed: &HashMap<String, String>,
) -> Result<()> {
    let changed: Vec<(String, i64, String)> = {
        let mut statement = transaction
            .prepare("SELECT ticket, n, text FROM comments WHERE instr(text, 'T') > 0")?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut changed = Vec::new();
        for row in rows {
            let (ticket, n, text) = row?;
            let new = rewrite_mentions(&text, renamed);
            if new != text {
                changed.push((ticket, n, new));
            }
        }
        changed
    };
    let mut update =
        transaction.prepare("UPDATE comments SET text = ?3 WHERE ticket = ?1 AND n = ?2")?;
    for (ticket, n, text) in changed {
        update.execute(rusqlite::params![ticket, n, text])?;
    }
    Ok(())
}

/// A changelog entry is `{field: [old, new]}`; the `parent` field's values are ticket IDs.
/// Entries imported from earlier trackers are arrays of events and are left as they are.
fn rewrite_history(transaction: &Transaction<'_>, renamed: &HashMap<String, String>) -> Result<()> {
    let changed: Vec<(String, i64, String)> = {
        let mut statement = transaction.prepare(
            "SELECT ticket, seq, change FROM history WHERE instr(change, '\"parent\"') > 0",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        let mut changed = Vec::new();
        for row in rows {
            let (ticket, seq, change) = row?;
            let Ok(Value::Object(mut fields)) = serde_json::from_str::<Value>(&change) else {
                continue;
            };
            let Some(Value::Array(values)) = fields.get_mut("parent") else {
                continue;
            };
            let mut touched = false;
            for value in values.iter_mut() {
                if let Value::String(id) = value
                    && let Some(new) = renamed.get(id.as_str())
                {
                    *id = new.clone();
                    touched = true;
                }
            }
            if touched {
                changed.push((ticket, seq, Value::Object(fields).to_string()));
            }
        }
        changed
    };
    let mut update =
        transaction.prepare("UPDATE history SET change = ?3 WHERE ticket = ?1 AND seq = ?2")?;
    for (ticket, seq, change) in changed {
        update.execute(rusqlite::params![ticket, seq, change])?;
    }
    Ok(())
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Rewrites each `T<old ID>` in `text` that starts and ends at a word boundary and names a
/// ticket to that ticket's new ID. The longest name that is a ticket wins: `Tq44s.7.2` becomes
/// `TQ44S.7.2` when that ticket exists, and otherwise `TQ44S.7` followed by `.2`.
fn rewrite_mentions(text: &str, renamed: &HashMap<String, String>) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if chars[index] == 'T'
            && (index == 0 || !word(chars[index - 1]))
            && let Some((end, new)) = mention_at(&chars, index + 1, renamed)
        {
            out.push_str(new);
            index = end;
            continue;
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

/// The end of the old ID that starts at `start`, and the ID it is renamed to.
fn mention_at<'a>(
    chars: &[char],
    start: usize,
    renamed: &'a HashMap<String, String>,
) -> Option<(usize, &'a str)> {
    let run = chars[start..]
        .iter()
        .take_while(|&&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
        .count();
    let mut end = start + run;
    // A name ends at a word boundary: what follows it is not a letter, digit or underscore.
    if chars.get(end).is_some_and(|&c| word(c)) {
        return None;
    }
    while end > start {
        let name: String = chars[start..end].iter().collect();
        if let Some(new) = renamed.get(&name) {
            return Some((end, new));
        }
        // Drop the last dotted part (or a trailing dot) and try again.
        end = start + name.rfind('.')?;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn renamed() -> HashMap<String, String> {
        ["q44s", "q44s.7", "ci-0017128a18"]
            .into_iter()
            .map(|old| (old.to_owned(), format!("T{}", old.to_ascii_uppercase())))
            .collect()
    }

    #[test]
    fn displayed_names_of_tickets_are_rewritten() {
        let text = "See Tq44s.7, then Tq44s. Also (Tci-0017128a18) and `Tq44s.7`.";
        assert_eq!(
            rewrite_mentions(text, &renamed()),
            "See TQ44S.7, then TQ44S. Also (TCI-0017128A18) and `TQ44S.7`."
        );
    }

    #[test]
    fn the_longest_ticket_wins_and_unknown_children_fall_back_to_the_parent() {
        assert_eq!(rewrite_mentions("Tq44s.7.9", &renamed()), "TQ44S.7.9");
        assert_eq!(rewrite_mentions("Tq44s.8", &renamed()), "TQ44S.8");
    }

    #[test]
    fn words_and_other_names_are_left_alone() {
        for text in ["The q44s", "Tq44sx", "xTq44s", "Tq44s_a", "Tzzzz", "Tq44S"] {
            assert_eq!(rewrite_mentions(text, &renamed()), text);
        }
    }

    #[test]
    fn the_migration_rewrites_ids_text_and_the_changelog() {
        use crate::hub::MIGRATIONS;
        let mut conn = rusqlite::Connection::open_in_memory().unwrap();
        let before = MIGRATIONS
            .iter()
            .position(|migration| migration.version == 12)
            .unwrap();
        crate::migrate::apply(&mut conn, &MIGRATIONS[..before]).unwrap();
        conn.execute_batch(
            "INSERT INTO tickets (id, parent, title, body, created_at, created_by, updated_at,
                                  version)
               VALUES ('q44s', NULL, 'Epic', '', 't', 'me', 't', 1),
                      ('q44s.7', 'q44s', 'Child', 'Blocks Tq44s and Tq44s.7.', 't', 'me', 't', 1);
             INSERT INTO edges (src, dst) VALUES ('q44s.7', 'q44s');
             INSERT INTO comments (ticket, n, author, at, text)
               VALUES ('q44s.7', 1, 'me', 't', 'Moved under Tq44s');
             INSERT INTO history (ticket, seq, actor, at, change)
               VALUES ('q44s.7', 1, 'me', 't', '{\"parent\":[null,\"q44s\"]}');
             INSERT INTO aliases (alias, id) VALUES ('by-old', 'q44s.7');
             INSERT INTO meta (key, value) VALUES ('create:k', '{\"at\":\"t\",\"id\":\"q44s\"}');
             INSERT INTO records (machine, ticket, verb, class, queued_s, wall_s, cpu_s,
                                  peak_mb, ignite, at)
               VALUES ('m', 'q44s.7', 'land', 'c', 0, 0, 0, 0, 'x', 't'),
                      ('m', 'adhoc20261118055450891', 'land', 'c', 0, 0, 0, 0, 'x', 't');",
        )
        .unwrap();
        crate::migrate::apply(&mut conn, MIGRATIONS).unwrap();
        let strings = |sql: &str| -> Vec<String> {
            let mut statement = conn.prepare(sql).unwrap();
            statement
                .query_map([], |row| row.get::<_, String>(0))
                .unwrap()
                .map(|row| row.unwrap())
                .collect()
        };
        assert_eq!(
            strings("SELECT id FROM tickets ORDER BY id"),
            ["TQ44S", "TQ44S.7"]
        );
        assert_eq!(
            strings("SELECT parent FROM tickets WHERE parent IS NOT NULL"),
            ["TQ44S"]
        );
        assert_eq!(
            strings("SELECT body FROM tickets WHERE id = 'TQ44S.7'"),
            ["Blocks TQ44S and TQ44S.7."]
        );
        assert_eq!(
            strings("SELECT src || '>' || dst FROM edges"),
            ["TQ44S.7>TQ44S"]
        );
        assert_eq!(strings("SELECT text FROM comments"), ["Moved under TQ44S"]);
        assert_eq!(
            strings("SELECT change FROM history"),
            ["{\"parent\":[null,\"TQ44S\"]}"]
        );
        assert_eq!(strings("SELECT id FROM aliases"), ["TQ44S.7"]);
        assert_eq!(
            strings("SELECT value ->> '$.id' FROM meta WHERE key = 'create:k'"),
            ["TQ44S"]
        );
        assert_eq!(
            strings("SELECT ticket FROM records ORDER BY id"),
            ["TQ44S.7", "adhoc20261118055450891"]
        );
    }
}
