//! Pieces every page of the ticket views shares: status dots, priority chips, the way a
//! session or person is named, progress bars and the page icons.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use ignite_core::protocol::tickets::Status;

use super::components::{self as c, Icons, Tone};
use super::data::{Card, Progress};
use super::references::display_id;

/// Where the site icons are served from, relative to the hub's root.
pub const SITE_ICONS: Icons = Icons {
    svg: "/favicon.svg",
    png: "/favicon-32.png",
    apple: "/apple-touch-icon.png",
};

pub const PRIORITY_NAMES: [&str; 5] = ["Critical", "High", "Medium", "Low", "Backlog"];

fn status_display(status: Status) -> (Tone, &'static str) {
    match status {
        Status::InProgress => (Tone::Blue, "In progress"),
        Status::Blocked => (Tone::Amber, "Blocked"),
        Status::Open => (Tone::Neutral, "Open"),
        Status::Deferred => (Tone::Muted, "Deferred"),
        Status::PendingApproval => (Tone::Muted, "Pending approval"),
        Status::Closed => (Tone::Green, "Closed"),
    }
}

/// A status as a colored dot, with its name beside it or, without `label`, as a tooltip.
pub fn status_dot(status: Status, label: bool) -> String {
    let (tone, name) = status_display(status);
    if label {
        c::dot(tone, name, None)
    } else {
        c::dot(tone, "", Some(name))
    }
}

/// `P0` to `P4`, red for critical, orange for high and neutral for medium.
pub fn priority_chip(priority: i64) -> String {
    let tone = match priority {
        0 => Tone::Red,
        1 => Tone::Orange,
        2 => Tone::Neutral,
        _ => Tone::Muted,
    };
    let name = usize::try_from(priority)
        .ok()
        .and_then(|index| PRIORITY_NAMES.get(index).copied());
    c::chip(&format!("P{priority}"), tone, name)
}

fn is_hex(text: &str) -> bool {
    text.chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c))
}

/// A session, vizier or person as the page names them: short form with the full name as a
/// tooltip.
pub fn person(name: Option<&str>) -> String {
    let Some(name) = name.filter(|name| !name.is_empty()) else {
        return c::muted("Nobody");
    };
    let short = if let Some(rest) = name.strip_prefix("local_")
        && rest.len() >= 6
        && rest.is_char_boundary(6)
        && is_hex(&rest[..6])
        && rest[6..]
            .chars()
            .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c) || c == '-')
    {
        Some(format!("session {}", &rest[..6]))
    } else if let Some(rest) = name.strip_prefix("vizier:")
        && let Some((host, _)) = rest.split_once(':')
        && !host.is_empty()
    {
        Some(format!("vizier on {host}"))
    } else {
        uuid_head(name)
    };
    short.map_or_else(|| c::esc(name), |short| c::tip(&c::esc(&short), name))
}

/// The first group of a lowercase UUID.
fn uuid_head(name: &str) -> Option<String> {
    let groups: Vec<&str> = name.split('-').collect();
    let sizes = [8, 4, 4, 4, 12];
    (groups.len() == 5
        && groups
            .iter()
            .zip(sizes)
            .all(|(group, size)| group.len() == size && is_hex(group)))
    .then(|| groups[0].to_owned())
}

/// A progress bar of a ticket's descendants, if it has any.
pub fn children_progress(count: Option<&Progress>) -> String {
    let Some(count) = count else {
        return String::new();
    };
    let todo = count.total.saturating_sub(count.closed + count.working);
    c::progress(
        count.closed,
        count.total,
        Some(&format!(
            "{} closed, {} in progress, {todo} to do",
            count.closed, count.working
        )),
    )
}

/// The name in the header and page titles. The dashboard is the hub's root, so no project name
/// appears in its addresses to derive one from.
pub fn brand() -> String {
    "Ignite".to_owned()
}

/// A card's ID and priority.
pub fn ticket_heading(ticket: &Card) -> String {
    format!(
        "<div class=\"c-card-top\">{}{}</div>",
        c::mono(&display_id(&ticket.id)),
        priority_chip(i64::from(ticket.priority))
    )
}
