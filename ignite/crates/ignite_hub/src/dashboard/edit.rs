//! The controls that edit a ticket from its page, shown only on the private listener to a
//! Tailscale user (see `crate::auth`).
//!
//! The server renders the hooks and the script (`static/edit.js`) builds the editors:
//!
//! - an editable value is a `.c-editable` element with a `data-field`, a `data-kind` that picks
//!   its editor, the rendered value in `.c-edit-view` and the raw value in a
//!   `<template class="c-edit-source">`;
//! - a pencil button (`data-edit="<field>"`) beside a heading or value opens that value's editor;
//! - buttons with `data-action` (close, reopen) change the status;
//! - one hidden `#c-edit` element names the ticket, its version and the batch address.
//!
//! An edit is an ordinary `POST /batch` of the hub's API, so it is recorded, relayed to a
//! session working the ticket and seen by dispatch like any other ticket write.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use ignite_core::fleet::FleetSettings;
use ignite_core::protocol::tickets::Status;

use super::components::{self as c, Tone, esc};

/// A 14-pixel pencil, drawn with the text color.
const PENCIL: &str = "<svg viewBox=\"0 0 16 16\" aria-hidden=\"true\" \
     fill=\"none\" stroke=\"currentColor\" stroke-width=\"1.5\" stroke-linecap=\"round\" \
     stroke-linejoin=\"round\"><path d=\"M11.5 2.5l2 2L6 12l-3 1 1-3z\"/><path d=\"M10 4l2 2\"/></svg>";

/// The element that tells the script which ticket the page edits: its ID, the version an edit
/// expects, and where the batch goes.
pub fn context(project: &str, ticket: &str, version: u64) -> String {
    format!(
        "<div id=\"c-edit\" hidden data-ticket=\"{}\" data-version=\"{version}\" data-rendered-version=\"{version}\" data-api=\"/{}/api/batch\"></div>",
        esc(ticket),
        esc(project)
    )
}

/// The pencil that opens the editor of `field`; `label` names the value for a screen reader.
pub fn pencil(field: &str, label: &str) -> String {
    format!(
        "<button type=\"button\" class=\"c-edit-button\" data-edit=\"{}\" aria-label=\"Edit {}\" title=\"Edit\">{PENCIL}</button>",
        esc(field),
        esc(&label.to_lowercase())
    )
}

/// A value the script can edit: `view` is its HTML, `raw` its text as the editor starts with,
/// and `kind` one of `line`, `markdown`, `status`, `labels`, `count`, `choice` or `allocation`. `block`
/// values (text sections) are a `div`, others a `span`.
pub fn unit(field: &str, kind: &str, view: &str, raw: &str, block: bool) -> String {
    let tag = if block { "div" } else { "span" };
    format!(
        "<{tag} class=\"c-editable\" data-field=\"{}\" data-kind=\"{}\"><{tag} class=\"c-edit-view\">{view}</{tag}><template class=\"c-edit-source\">{}</template></{tag}>",
        esc(field),
        esc(kind),
        esc(raw)
    )
}

/// A rail value with its pencil after it.
pub fn property(field: &str, label: &str, kind: &str, view: &str, raw: &str) -> String {
    unit(field, kind, view, raw, false) + &pencil(field, label)
}

/// The numeric allocation editor, shared by tickets, epics and fleet settings.
pub(super) fn allocation(
    field: &str,
    label: &str,
    local: Option<u32>,
    inherited: Option<u32>,
    editable: bool,
) -> String {
    let fallback = inherited.map_or_else(
        || "Inherited (inactive)".to_owned(),
        |n| format!("Inherited ({n})"),
    );
    let view = match local {
        Some(0) => super::components::muted("0 · Parked"),
        Some(n) => n.to_string(),
        None => super::components::muted(&fallback),
    };
    if !editable {
        return view;
    }
    allocation_property(field, label, local, &fallback, &view)
}

/// A category heading shows the effective number, including the inherited value.
pub(super) fn allocation_chip(
    field: &str,
    label: &str,
    local: Option<u32>,
    inherited: u32,
    editable: bool,
) -> String {
    let value = local.unwrap_or(inherited);
    let hint = match (local, value) {
        (None, 0) => "Allocation inherited · Parked",
        (None, _) => "Allocation inherited",
        (Some(0), _) => "Allocation · Parked",
        _ => "Allocation",
    };
    let tone = if local.is_none() || value == 0 {
        Tone::Muted
    } else {
        Tone::Neutral
    };
    let view = c::chip(&value.to_string(), tone, Some(hint));
    if !editable {
        return view;
    }
    allocation_property(
        field,
        label,
        local,
        &format!("Inherited ({inherited})"),
        &view,
    )
}

fn allocation_property(
    field: &str,
    label: &str,
    local: Option<u32>,
    fallback: &str,
    view: &str,
) -> String {
    let raw = local.map(|n| n.to_string()).unwrap_or_default();
    let view = format!(
        "{view}<template class=\"c-edit-inherited\">{}</template>",
        esc(fallback)
    );
    property(field, label, "allocation", &view, &raw)
}

/// The settings and API address for every fleet allocation editor on the page.
pub(super) fn fleet_allocation_context(
    settings: &FleetSettings,
    project: &str,
    body: &str,
) -> String {
    let settings = serde_json::to_string(settings).unwrap_or_default();
    format!(
        r#"<div id="c-allocation-fleet" data-api="/{}/api/fleet" data-settings="{}">{body}</div>"#,
        esc(project),
        esc(&settings),
    )
}

/// A dropdown whose choices are labels. An empty value removes the ticket's local pin.
pub(super) fn choice_property(
    field: &str,
    label: &str,
    view: &str,
    raw: &str,
    choices: &[(String, String)],
    labels: &[&str],
) -> String {
    let choices = serde_json::json!(choices);
    let labels = serde_json::json!(labels);
    let view = format!(
        "{view}<template class=\"c-edit-choices\">{}</template><template class=\"c-edit-pin-labels\">{}</template>",
        esc(&choices.to_string()),
        esc(&labels.to_string())
    );
    property(field, label, "choice", &view, raw)
}

/// A Markdown text section whose heading carries a pencil, built like
/// `components::section`. An `empty` section starts hidden until its "add" button shows it.
pub fn section(heading: &str, anchor: &str, field: &str, view: &str, raw: &str) -> String {
    let empty = raw.trim().is_empty();
    format!(
        "<section class=\"c-section\" id=\"{}\"{}><h2 class=\"c-section-title\">{}{}</h2>{}</section>",
        esc(anchor),
        if empty { " hidden data-edit-empty" } else { "" },
        esc(heading),
        pencil(field, heading),
        unit(field, "markdown", view, raw, true)
    )
}

/// One button per empty text section, which shows the section with its editor open.
pub fn adders(missing: &[(&str, &str)]) -> String {
    if missing.is_empty() {
        return String::new();
    }
    let buttons: String = missing
        .iter()
        .map(|(field, heading)| {
            format!(
                "<button type=\"button\" class=\"c-edit-add\" data-add=\"{}\">+ {}</button>",
                esc(field),
                esc(heading)
            )
        })
        .collect();
    format!("<div class=\"c-edit-adders\">{buttons}</div>")
}

/// The status actions at the top of the rail.
pub fn actions(status: Status) -> String {
    let (action, text) = if status == Status::Closed {
        ("reopen", "Reopen")
    } else {
        ("close", "Close")
    };
    format!(
        "<div class=\"c-edit-actions\"><button type=\"button\" class=\"c-edit-action\" data-action=\"{action}\">{text}</button></div>"
    )
}

/// The box under the comments that adds one.
pub fn composer() -> String {
    "<form class=\"c-edit-composer\" data-composer><textarea name=\"text\" rows=\"3\" placeholder=\"Add a comment\" aria-label=\"Comment\"></textarea><div class=\"c-edit-buttons\"><button type=\"submit\" class=\"c-edit-action c-primary\">Comment</button></div></form>".to_owned()
}

/// The raw value of a status as the editor's options name it.
pub fn status_value(status: Status) -> &'static str {
    match status {
        Status::Open => "open",
        Status::InProgress => "in_progress",
        Status::Blocked => "blocked",
        Status::Deferred => "deferred",
        Status::Closed => "closed",
        Status::PendingApproval => "pending_approval",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_unit_keeps_its_raw_text_escaped_in_a_template() {
        let html = unit("body", "markdown", "<p>x</p>", "a < b & \"c\"", true);
        assert!(
            html.contains(
                "<template class=\"c-edit-source\">a &lt; b &amp; &quot;c&quot;</template>"
            )
        );
        assert!(
            html.starts_with(
                "<div class=\"c-editable\" data-field=\"body\" data-kind=\"markdown\">"
            )
        );
    }

    #[test]
    fn the_actions_follow_the_status() {
        assert!(actions(Status::PendingApproval).contains("data-action=\"close\""));
        assert!(actions(Status::Open).contains("data-action=\"close\""));
        assert!(!actions(Status::Open).contains("release"));
        assert!(actions(Status::Closed).contains("data-action=\"reopen\""));
    }

    #[test]
    fn an_empty_section_starts_hidden() {
        assert!(section("Design", "design", "design", "", " ").contains(" hidden data-edit-empty"));
        assert!(
            !section("Design", "design", "design", "<p>x</p>", "x").contains("data-edit-empty")
        );
    }
}
