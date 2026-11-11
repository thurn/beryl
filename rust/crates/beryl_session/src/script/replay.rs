//! The replay check (plan K24): a script played over the table of the view before gives the
//! table of the view it came with, exactly. The session runs it in a debug build wherever it
//! builds a script, as the engine checks its invariants on every debug apply
//! (`beryl_engine::invariants`), so every test that plays a game holds every script to it.

use serde_json::Value;

use super::{Reset, Script, Table};
use crate::protocol::{PlayerView, StackItem};

/// `before` once `script` has played over it: `before` is the table of the view before, or
/// with a reset, the table of the view the script came with. An error names the step that
/// cannot be played.
pub fn replay(mut before: Table, script: &Script) -> Result<Table, String> {
    if let Some(reset) = &script.reset {
        number(&mut before, reset)?;
    }
    for step in script.beats.iter().flatten() {
        before
            .apply(&step.act)
            .map_err(|e| format!("`{step}`: {e}"))?;
    }
    Ok(before)
}

/// Gives the abilities on `table`'s stack the numbers a reset lists, bottom first.
fn number(table: &mut Table, reset: &Reset) -> Result<(), String> {
    let mut ids = reset.abilities.iter();
    for entry in &mut table.stack {
        if matches!(entry.item, StackItem::Ability { .. }) {
            entry.id = Some(*ids.next().ok_or("a reset numbers too few abilities")?);
        }
    }
    match ids.next() {
        None => Ok(()),
        Some(id) => Err(format!("a reset numbers a{id}, which is not on the stack")),
    }
}

/// What is wrong with `script`, which came with `view`, as a seat that showed `before` plays
/// it: the first step that cannot be played, or the first part of the table that ends up other
/// than the view's, with the script's text. None when the replay gives the view's table.
pub fn failure(before: &Table, script: &Script, view: &PlayerView) -> Option<String> {
    check(before.clone(), script, &Table::of(view))
}

/// [`failure`], from the tables themselves.
pub(super) fn check(before: Table, script: &Script, after: &Table) -> Option<String> {
    let start = if script.reset.is_some() {
        after.clone()
    } else {
        before
    };
    let wrong = match replay(start, script) {
        Err(e) => e,
        Ok(table) if table.shows(after) => return None,
        Ok(table) => difference(&table, after),
    };
    Some(format!("{wrong}\nin the script\n{script}"))
}

/// The first part of `replayed` that differs from `view`'s, by its path in the table.
fn difference(replayed: &Table, view: &Table) -> String {
    let value = |table| serde_json::to_value(table).expect("a table is plain data");
    first_difference(&value(replayed), &value(view), "table").map_or_else(
        || "the tables are the same".to_owned(),
        |(path, replayed, view)| format!("{path} is {replayed} replayed and {view} in the view"),
    )
}

/// The path and the two values of the first place `a` and `b` differ, depth first.
fn first_difference(a: &Value, b: &Value, path: &str) -> Option<(String, String, String)> {
    match (a, b) {
        _ if a == b => None,
        (Value::Object(a), Value::Object(b)) => a
            .keys()
            .chain(b.keys().filter(|k| !a.contains_key(*k)))
            .find_map(|key| {
                let absent = Value::Null;
                first_difference(
                    a.get(key).unwrap_or(&absent),
                    b.get(key).unwrap_or(&absent),
                    &format!("{path}.{key}"),
                )
            }),
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => a
            .iter()
            .zip(b)
            .enumerate()
            .find_map(|(i, (a, b))| first_difference(a, b, &format!("{path}[{i}]"))),
        _ => Some((path.to_owned(), a.to_string(), b.to_string())),
    }
}
