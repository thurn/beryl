//! The Fleet page: machine settings, recent work, trees and the settings change log.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;

use ignite_core::fleet::{FleetChange, FleetSettings, MachineSetting, MachineState};
use ignite_core::time::Timestamp;
use ignite_store::hub::{fleet, rows::Machine};

use crate::routes::fleet::view_in_snapshot;

use super::attention::{MACHINE_SILENT_SECONDS, age, frame, seconds_since};
use super::components::{self as c, PageOptions, Tone, esc};
use super::data::ReadError;
use super::fleet_providers;
use super::references::{display_id, ticket_url};

/// Commands newer than this count toward a machine's work by class.
const RECENT_SECONDS: f64 = 86_400.0;

fn state_label(state: MachineState) -> &'static str {
    match state {
        MachineState::Active => "Active",
        MachineState::ChecksOnly => "Checks only",
        MachineState::Off => "Off",
    }
}

fn state_description(state: MachineState) -> &'static str {
    match state {
        MachineState::Active => "Takes tickets and verifies tips.",
        MachineState::ChecksOnly => {
            "Verifies tips. Takes a ticket only if it holds master back and no active machine can take it."
        }
        MachineState::Off => {
            "Sends heartbeats and updates itself. Takes no tickets, tip checks or idle work."
        }
    }
}

fn machine_settings(machine: &Machine, setting: MachineSetting, editable: bool) -> String {
    let MachineSetting { state, disk_cap_gb } = setting;
    let description = state_description(state);
    let (state_control, disk_cap) = if editable {
        let name = esc(&machine.name);
        let group = format!("fleet-state-{}", esc(&machine.id));
        let help = format!("{group}-help");
        let choices: String = [MachineState::Active, MachineState::ChecksOnly, MachineState::Off]
            .into_iter()
            .map(|choice| {
                let checked = if choice == state { " checked" } else { "" };
                format!(
                    r#"<label><input type="radio" autocomplete="off" name="{group}" data-fleet-field="state" value="{choice}"{checked}>{}</label>"#,
                    state_label(choice)
                )
            })
            .collect();
        let value = disk_cap_gb.map_or_else(String::new, |gb| gb.to_string());
        let max = u32::MAX;
        (
            format!(
                r#"<div class="c-segmented" role="radiogroup" aria-label="{name} state" aria-describedby="{help}">{choices}</div><span class="c-muted" id="{help}">{description}</span>"#
            ),
            format!(
                r#"<div class="c-edit-form"><input type="number" autocomplete="off" data-fleet-field="disk_cap_gb" aria-label="{name} disk cap in GB" min="0" max="{max}" step="1" value="{value}" placeholder="Automatic"></div>"#
            ),
        )
    } else {
        (
            format!("{}{}", state_label(state), c::muted(description)),
            disk_cap_gb.map_or_else(|| "Automatic".to_owned(), |gb| gb.to_string()),
        )
    };
    let items = [
        ("State".to_owned(), state_control),
        ("Disk cap (GB)".to_owned(), disk_cap),
    ];
    format!(
        r#"<div class="c-fleet-settings" data-fleet-machine="{}">{}</div>"#,
        esc(&machine.name),
        c::meta(&items)
    )
}

fn change_log(changes: &[FleetChange]) -> String {
    let body = if changes.is_empty() {
        c::empty("No fleet settings have changed.")
    } else {
        c::timeline(changes.iter().map(|change| {
            let actor = change.actor.split('@').next().unwrap_or_default();
            c::timeline_entry(
                &esc(&change.summary),
                &c::time(Some(&change.at.to_string()), ""),
                &esc(actor),
                false,
            )
        }))
    };
    c::section("Recent changes", &body, None, None, 2)
}

fn editor(project: &str, settings: &FleetSettings, body: &str) -> Result<String, ReadError> {
    let settings = serde_json::to_string(settings).map_err(|error| ReadError(error.to_string()))?;
    Ok(format!(
        r#"<div id="c-fleet" data-api="/{}/api/fleet" data-settings="{}"><p class="c-edit-error" data-fleet-status role="status" tabindex="-1" hidden></p>{body}</div>"#,
        esc(project),
        esc(&settings)
    ))
}

/// One machine's section: its state, its work in the last day by class, and its trees.
fn machine_section(
    conn: &Connection,
    root: &str,
    now: Timestamp,
    machine: &Machine,
    setting: MachineSetting,
    editable: bool,
) -> Result<String, ReadError> {
    let Machine {
        id,
        name,
        os,
        cores,
        memory_mb,
        version,
        last_seen,
        ..
    } = machine;
    let since = now.plus_seconds(-RECENT_SECONDS).to_string();
    let silent = seconds_since(now, Some(last_seen)).is_none_or(|s| s > MACHINE_SILENT_SECONDS);
    let connection = if silent {
        c::dot(Tone::Red, "Not reporting", None)
    } else {
        c::dot(Tone::Green, "Reporting", None)
    };
    let mut by_class = conn.prepare(
        "SELECT class, COUNT(*), SUM(cpu_s) FROM records
             WHERE machine = ?1 AND at >= ?2 GROUP BY class ORDER BY class",
    )?;
    let classes = by_class
        .query_map([id, &since], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, f64>(2)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let spent = if classes.is_empty() {
        c::muted("No commands in the last day")
    } else {
        classes
            .iter()
            .map(|(class, count, cpu)| {
                format!("{} {count} ({:.0} core-minutes)", esc(class), cpu / 60.0)
            })
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut trees = conn.prepare(
        "SELECT tree, id, status, title FROM tickets
             WHERE machine = ?1 AND tree IS NOT NULL AND status != 'closed' ORDER BY tree",
    )?;
    let trees = trees
        .query_map([id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let tree_rows = if trees.is_empty() {
        c::empty("No tree holds a ticket.")
    } else {
        c::rows(trees.iter().map(|(tree, ticket, status, title)| {
            let label = c::link(
                &ticket_url(root, ticket),
                &esc(&display_id(ticket)),
                None,
                "",
            );
            c::row(
                &(c::mono(tree)
                    + &c::muted(" · ")
                    + &label
                    + &c::muted(&format!(" {}", esc(title)))),
                &esc(status),
                None,
            )
        }))
    };
    let items = vec![
        ("Connection".to_owned(), connection),
        ("System".to_owned(), esc(os)),
        (
            "Cores".to_owned(),
            format!("{cores}, {} GB memory", memory_mb / 1024),
        ),
        (
            "Last seen".to_owned(),
            esc(&age(now, Some(last_seen))) + " ago",
        ),
        ("Version".to_owned(), c::mono(version)),
        ("Last day, by class".to_owned(), spent),
    ];
    Ok(c::section(
        name,
        &(machine_settings(machine, setting, editable)
            + &c::meta(&items)
            + &c::subsection("Trees", &tree_rows, Some(trees.len()), 3)),
        None,
        None,
        2,
    ))
}

/// The page for the router.
pub(crate) fn options(
    conn: &Connection,
    root: &str,
    seq: &str,
    now: Timestamp,
    editor_project: Option<&str>,
) -> Result<PageOptions, ReadError> {
    let snapshot = conn.unchecked_transaction()?;
    let machines = fleet::machines(&snapshot)?;
    let view = view_in_snapshot(&snapshot, &machines, now)?;
    let settings = &view.settings;
    let editable = editor_project.is_some();
    let mut body = String::new();
    for machine in &machines {
        body += &machine_section(
            &snapshot,
            root,
            now,
            machine,
            settings.machine(&machine.name),
            editable,
        )?;
    }
    if machines.is_empty() {
        body = c::empty("No machine has registered.");
    }
    body = fleet_providers::section(&snapshot, &view, &machines, editable)? + &body;
    body += &change_log(&view.changes);
    snapshot.commit()?;
    if let Some(project) = editor_project {
        body = editor(project, settings, &body)?;
    }
    Ok(frame(root, seq, "fleet", "", &body))
}
