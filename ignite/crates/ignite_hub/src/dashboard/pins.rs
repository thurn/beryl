//! Machine and provider pins on ticket cards and in ticket properties.

use ignite_core::pins::{Pins, from_labels};
use ignite_store::Result;
use ignite_store::hub::{fleet, fleet_settings, pins};
use rusqlite::Connection;

use super::components::{self as c, Tone, esc};
use super::edit;

pub(super) fn chips(pins: &Pins) -> String {
    pins.machine
        .iter()
        .chain(pins.provider.iter())
        .map(|name| c::chip(name, Tone::Neutral, None))
        .collect()
}

pub(super) fn properties(
    conn: &Connection,
    id: &str,
    labels: &[String],
    editable: bool,
) -> Result<[(String, String); 2]> {
    let own = from_labels(labels);
    let effective = pins::of(conn, id)?;
    let (machines, providers) = if editable {
        let machines = fleet::machines(conn)?
            .into_iter()
            .map(|machine| machine.name)
            .collect();
        let providers = fleet_settings::settings(conn)?
            .providers
            .into_keys()
            .collect();
        (Some(machines), Some(providers))
    } else {
        (None, None)
    };
    Ok([
        property(
            "machine",
            "Machine",
            labels,
            own.machine.as_deref(),
            effective.machine.as_deref(),
            machines,
        ),
        property(
            "provider",
            "Provider",
            labels,
            own.provider.as_deref(),
            effective.provider.as_deref(),
            providers,
        ),
    ])
}

fn property(
    field: &str,
    label: &str,
    labels: &[String],
    own: Option<&str>,
    effective: Option<&str>,
    choices: Option<Vec<String>>,
) -> (String, String) {
    let mut view = effective.map_or_else(|| c::muted("None"), esc);
    if effective.is_some() && own.is_none() {
        view.push_str(" <span class=\"c-muted\">inherited</span>");
    }
    if let Some(mut names) = choices {
        if let Some(own) = own {
            // A saved pin can outlive a fleet entry. Keep the current value selectable.
            names.retain(|name| !name.eq_ignore_ascii_case(own));
            names.push(own.to_owned());
        }
        names.sort_unstable();
        names.dedup();
        let prefix = format!("{field}:");
        let existing: Vec<&str> = labels
            .iter()
            .filter(|name| name.starts_with(&prefix))
            .map(String::as_str)
            .collect();
        let raw = own
            .map(|name| format!("{prefix}{name}"))
            .unwrap_or_default();
        let mut options = vec![(String::new(), "None".to_owned())];
        options.extend(
            names
                .into_iter()
                .map(|name| (format!("{prefix}{name}"), name)),
        );
        view = edit::choice_property(field, label, &view, &raw, &options, &existing);
    }
    (label.to_owned(), view)
}
