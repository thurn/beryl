//! The hub requests the ticket verbs send, built from the parsed arguments. Nothing here talks
//! to the hub, so each builder is tested on its own.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::{Map, Value, json};

use super::http::{Failure, Result, usage};
use crate::args::TicketFieldArgs;

type Fields = Map<String, Value>;

/// The `fields` of a create or update, from the flags both share.
fn shared_fields(fields: &mut Fields, args: &TicketFieldArgs) -> Result<()> {
    put(fields, "type", args.ticket_type.clone());
    put(fields, "priority", args.priority);
    put(fields, "body", read_file(args.body_file.as_deref())?);
    put(fields, "acceptance", args.acceptance.clone());
    put(fields, "workers", args.allocation);
    put(fields, "max_workers", args.max_workers);
    put(fields, "cores", args.cores);
    put(fields, "every_hours", args.every);
    if let Some(owner) = &args.owner {
        // `default` is the old spelling, from when an unowned ticket followed a default owner.
        let value = if owner == "none" || owner == "default" {
            Value::Null
        } else {
            let parsed: ignite_core::owner::Owner = owner.parse().map_err(usage)?;
            json!(parsed.to_string())
        };
        fields.insert("owner".to_owned(), value);
    }
    Ok(())
}

fn put(fields: &mut Fields, name: &str, value: Option<impl Into<Value>>) {
    if let Some(value) = value {
        fields.insert(name.to_owned(), value.into());
    }
}

/// An empty string clears a field that may be absent.
fn nullable(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        json!(value)
    }
}

/// The text of a file, or of standard input for `-`.
fn read_file(path: Option<&Path>) -> Result<Option<String>> {
    let Some(path) = path else { return Ok(None) };
    let read = if path == Path::new("-") {
        std::io::read_to_string(std::io::stdin())
    } else {
        std::fs::read_to_string(path)
    };
    read.map(Some)
        .map_err(|error| usage(format!("{}: {error}", path.display())))
}

/// A dependency named on `create`: a ticket, or `kind:ticket` with `blocked-by`, `depends-on`,
/// `relates-to`, or an edge kind of the hub's own.
fn dependency(spec: &str) -> Result<Value> {
    let Some((kind, id)) = spec.split_once(':') else {
        return Ok(json!({"on": spec}));
    };
    let kind = match kind {
        "blocked-by" | "depends-on" => "blocks",
        "relates-to" => "related",
        "blocks" => {
            return Err(usage(
                "`blocks:` is not supported on create; add the edge with `dep add` afterwards",
            ));
        }
        other => other,
    };
    Ok(json!({"on": id, "kind": kind}))
}

/// The status of a ticket filed but not yet released for implementation.
/// `body` marked as filed by a program when `automatic` (`ignite ticket create --automatic`), so
/// that the hub makes the writer's machine the owner of the tickets it creates.
/// It is also marked `independent` when the filer says the tickets need none of its unlanded
/// work (`--independent`), so that the hub adds no dependency on it.
pub fn filed(mut body: Value, automatic: bool, independent: bool) -> Value {
    if automatic {
        body["automatic"] = Value::Bool(true);
    }
    if independent {
        body["independent"] = Value::Bool(true);
    }
    body
}

/// The status `--release` or `--pending` asks a new ticket to be filed with, if either.
pub fn filing_status(release: bool, pending: bool) -> Option<&'static str> {
    if release {
        Some(RELEASED)
    } else if pending {
        Some(PENDING)
    } else {
        None
    }
}

pub const PENDING: &str = "pending_approval";

/// The status a released ticket takes.
pub const RELEASED: &str = "open";

/// Files a ticket with `status` when one is given, such as `open` for `--release` or
/// `pending_approval` for `--pending`. Without one the hub decides: an epic, or a ticket under a
/// pending epic, waits for a person's release, and any other ticket is filed `open` (see
/// `add_holds` in `ignite_store`).
pub fn create(
    title: &str,
    fields: &TicketFieldArgs,
    labels: &[String],
    deps: &[String],
    status: Option<&str>,
) -> Result<Value> {
    let mut set = Fields::new();
    set.insert("title".into(), json!(title));
    if let Some(status) = status {
        set.insert("status".into(), json!(status));
    }
    shared_fields(&mut set, fields)?;
    if fields.every.is_some() && fields.ticket_type.is_none() {
        set.insert("type".into(), json!("meta"));
    }
    if !labels.is_empty() {
        set.insert("labels".into(), json!(labels));
    }
    let deps = deps
        .iter()
        .map(|spec| dependency(spec))
        .collect::<Result<Vec<_>>>()?;
    let mut op = json!({"op": "create", "fields": set, "deps": deps});
    if let Some(parent) = &fields.parent {
        op["parent"] = json!(parent);
    }
    Ok(op)
}

/// What `update` was asked to change besides the shared fields.
pub struct Changes<'a> {
    pub title: Option<&'a str>,
    pub status: Option<&'a str>,
    pub assignee: Option<&'a str>,
    pub add_labels: &'a [String],
    pub remove_labels: &'a [String],
}

pub fn update(id: &str, fields: &TicketFieldArgs, changes: &Changes) -> Result<Value> {
    let mut set = Fields::new();
    put(&mut set, "title", changes.title);
    put(&mut set, "status", changes.status);
    put(&mut set, "assignee", changes.assignee.map(nullable));
    shared_fields(&mut set, fields)?;
    if let Some(parent) = &fields.parent {
        set.insert("parent".into(), nullable(parent));
    }
    let mut op = json!({"op": "update", "id": id});
    let mut any = !set.is_empty();
    op["set"] = Value::Object(set);
    for (name, values) in [
        ("add_labels", changes.add_labels),
        ("remove_labels", changes.remove_labels),
    ] {
        if !values.is_empty() {
            op[name] = json!(values);
            any = true;
        }
    }
    if any {
        Ok(op)
    } else {
        Err(usage(
            "nothing to update: give at least one field to change",
        ))
    }
}

/// How the reply to an update reads: a new parent gives the ticket a new ID, so the reply says
/// `moved <old> to <new>`.
pub fn update_verb(id: &str, fields: &TicketFieldArgs) -> String {
    if fields.parent.is_some() {
        format!("moved {id} to")
    } else {
        "updated".to_owned()
    }
}

/// An edge between two tickets. `relates-to` is the hub's `related`.
pub fn edge(op: &str, id: &str, on: &str, kind: Option<&str>) -> Value {
    let mut value = json!({"op": op, "id": id, "on": on});
    if let Some(kind) = kind {
        value["kind"] = json!(if kind == "relates-to" {
            "related"
        } else {
            kind
        });
    }
    value
}

/// A one-operation batch.
pub fn single(op: Value) -> Value {
    Value::Object(Map::from_iter([("ops".to_owned(), Value::Array(vec![op]))]))
}

/// A batch read from a file: an object with `ops` (and optionally `if`), or a bare array of
/// operations.
pub fn batch_file(path: &Path) -> Result<Value> {
    let text = read_file(Some(path))?.unwrap_or_default();
    let value: Value = serde_json::from_str(&text).map_err(|e| usage(format!("batch: {e}")))?;
    let body = if value.is_array() {
        json!({"ops": value})
    } else {
        value
    };
    if !body["ops"].is_array() {
        return Err(usage("a batch file needs an `ops` array"));
    }
    Ok(body)
}

/// Gives every create a `request_key` it lacks, so that repeating the batch after an unclear
/// failure returns the tickets already made instead of making them twice. Returns whether
/// repeating the whole batch is then safe.
pub fn prepare_batch(body: &mut Value, mut key: impl FnMut() -> Result<String>) -> Result<bool> {
    let guarded = body["if"].as_array().is_some_and(|conditions| {
        conditions
            .iter()
            .any(|c| c["seq"].is_u64() || c["version"].is_u64())
    });
    let operations = body["ops"]
        .as_array_mut()
        .ok_or_else(|| usage("batch requires an ops array"))?;
    let mut all_creates = !operations.is_empty();
    for operation in operations {
        if operation["op"] == "create" {
            if operation.get("request_key").is_none() {
                operation["request_key"] = json!(key()?);
            }
        } else {
            all_creates = false;
        }
    }
    Ok(guarded || all_creates)
}

/// A random 128-bit key as hex.
pub fn random_key() -> Result<String> {
    let mut bytes = [0_u8; 16];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| Failure::new(ignite_core::protocol::client::Exit::Usage, error))?;
    Ok(bytes.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_fields() -> TicketFieldArgs {
        TicketFieldArgs {
            priority: None,
            ticket_type: None,
            parent: None,
            body_file: None,
            acceptance: None,
            allocation: None,
            max_workers: None,
            cores: None,
            owner: None,
            every: None,
        }
    }

    #[test]
    fn every_files_a_meta_ticket_and_update_changes_its_schedule() {
        let scheduled = TicketFieldArgs {
            every: Some(6),
            ..no_fields()
        };
        let op = create("Sweep", &scheduled, &[], &[], None).unwrap();
        assert_eq!(op["fields"]["type"], "meta");
        assert_eq!(op["fields"]["every_hours"], 6);
        let changed = update("MAB12", &scheduled, &no_changes()).unwrap();
        assert_eq!(changed["set"], json!({"every_hours": 6}));
    }

    #[test]
    fn an_owner_is_set_checked_or_cleared() {
        let owned = |owner: &str| TicketFieldArgs {
            owner: Some(owner.to_owned()),
            ..no_fields()
        };
        let set = create("x", &owned("Cygnus/Codex"), &[], &[], None).unwrap();
        assert_eq!(set["fields"]["owner"], "cygnus/codex");
        assert!(create("x", &owned("a b"), &[], &[], None).is_err());
        let cleared = update("t1", &owned("default"), &no_changes()).unwrap();
        assert_eq!(cleared["set"]["owner"], Value::Null);
    }

    fn no_changes() -> Changes<'static> {
        Changes {
            title: None,
            status: None,
            assignee: None,
            add_labels: &[],
            remove_labels: &[],
        }
    }

    #[test]
    fn create_carries_the_new_fields_labels_and_dependencies() {
        let fields = TicketFieldArgs {
            priority: Some(1),
            parent: Some("Tig9a".into()),
            allocation: Some(3),
            max_workers: Some(5),
            cores: Some(2.5),
            ..no_fields()
        };
        let op = create(
            "Split",
            &fields,
            &["tier:opus".into()],
            &["a".into(), "blocked-by:b".into(), "relates-to:c".into()],
            None,
        )
        .unwrap();
        assert_eq!(
            op,
            json!({
                "op": "create", "parent": "Tig9a",
                "fields": {"title": "Split", "priority": 1, "workers": 3, "max_workers": 5,
                           "cores": 2.5, "labels": ["tier:opus"]},
                "deps": [{"on": "a"}, {"on": "b", "kind": "blocks"},
                         {"on": "c", "kind": "related"}],
            })
        );
        assert!(create("x", &no_fields(), &[], &["blocks:z".into()], None).is_err());
        let released = create("x", &no_fields(), &[], &[], Some(RELEASED)).unwrap();
        assert_eq!(released["fields"]["status"], "open");
    }

    #[test]
    fn update_clears_fields_with_an_empty_value_and_refuses_an_empty_change() {
        let changes = Changes {
            title: None,
            status: Some("blocked"),
            assignee: Some(""),
            add_labels: &["x".into()],
            remove_labels: &[],
        };
        let fields = TicketFieldArgs {
            parent: Some(String::new()),
            ..no_fields()
        };
        assert_eq!(
            update("t1", &fields, &changes).unwrap(),
            json!({"op": "update", "id": "t1", "add_labels": ["x"],
                   "set": {"status": "blocked", "assignee": null, "parent": null}})
        );
        let nothing = Changes {
            title: None,
            status: None,
            assignee: None,
            add_labels: &[],
            remove_labels: &[],
        };
        assert!(update("t1", &no_fields(), &nothing).is_err());
    }

    #[test]
    fn a_batch_of_creates_is_given_keys_and_is_safe_to_repeat() {
        let mut body = json!({"ops": [
            {"op": "create", "fields": {"title": "Epic"}},
            {"op": "create", "fields": {"title": "Kept"}, "request_key": "mine"},
        ]});
        let mut n = 0;
        let safe = prepare_batch(&mut body, || {
            n += 1;
            Ok(format!("k{n}"))
        })
        .unwrap();
        assert!(safe);
        assert_eq!(body["ops"][0]["request_key"], "k1");
        assert_eq!(body["ops"][1]["request_key"], "mine");
    }

    #[test]
    fn a_batch_with_a_comment_is_repeated_only_under_a_version_guard() {
        let ops = json!([{"op": "comment", "id": "a", "text": "hi"}]);
        let mut open = json!({"ops": ops});
        assert!(!prepare_batch(&mut open, random_key).unwrap());
        let mut guarded = json!({"if": [{"id": "a", "version": 3}], "ops": ops});
        assert!(prepare_batch(&mut guarded, random_key).unwrap());
        assert!(prepare_batch(&mut json!({}), random_key).is_err());
    }
}
