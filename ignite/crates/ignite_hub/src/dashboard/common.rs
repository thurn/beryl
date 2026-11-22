//! Pieces every page of the ticket views shares: status dots, allocation chips, the way a
//! session or person is named, progress bars and the page icons.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use std::cell::RefCell;
use std::collections::HashMap;

use ignite_core::protocol::tickets::Status;
use ignite_core::work_estimate::{EstimateDefault, WorkRemaining};

use super::components::{self as c, Icons, Tone};
use super::data::Card;
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
        Status::Open => (Tone::Orange, "Open"),
        Status::Deferred => (Tone::Muted, "Deferred"),
        Status::PendingApproval => (Tone::Muted, "Parked"),
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

thread_local! {
    /// The titles of the sessions the page being rendered may name, by session id. Set for the
    /// length of one request by [`with_session_titles`], so that [`person`], which dozens of
    /// fragments call, need not be passed them.
    static SESSION_TITLES: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Runs `render` with `titles` as the session titles that [`person`] shows.
pub(crate) fn with_session_titles<T>(
    titles: HashMap<String, String>,
    render: impl FnOnce() -> T,
) -> T {
    SESSION_TITLES.with(|cell| *cell.borrow_mut() = titles);
    let rendered = render();
    SESSION_TITLES.with(|cell| cell.borrow_mut().clear());
    rendered
}

/// The title the session `name` last reported, if the page knows one.
pub(crate) fn session_title(name: &str) -> Option<String> {
    SESSION_TITLES.with(|cell| cell.borrow().get(name).cloned())
}

/// A session, vizier or person as the page names them: a session's title when it reported one,
/// otherwise a short form, with the full name as a tooltip.
pub fn person(name: Option<&str>) -> String {
    named(name, name.and_then(session_title))
}

/// The session, vizier or person working on `ticket`, as [`person`] names them, except that a
/// title naming another ticket in brackets is not shown. A reused worker claims its next ticket
/// while its title still reads `⚒️ [<previous ticket>] ...`, and the daemon reports the new title
/// only at its next heartbeat; until then the short form is shown instead of the wrong ticket.
pub fn worker(name: Option<&str>, ticket: &str) -> String {
    let title = name
        .and_then(session_title)
        .filter(|title| !names_another_ticket(title, ticket));
    named(name, title)
}

/// Whether `title` has a bracketed ticket ID, such as `[TW4YS]`, and none of them is `ticket`.
fn names_another_ticket(title: &str, ticket: &str) -> bool {
    let mut named = title.split('[').skip(1).filter_map(|rest| {
        let (id, _) = rest.split_once(']')?;
        let looks_like_id = !id.is_empty()
            && id.chars().all(|c| c.is_ascii_alphanumeric() || c == '.')
            && id.chars().any(|c| c.is_ascii_alphabetic());
        looks_like_id.then_some(id)
    });
    let mut any = false;
    for id in named.by_ref() {
        if id.eq_ignore_ascii_case(ticket) {
            return false;
        }
        any = true;
    }
    any
}

/// A Claude desktop session (`local_<uuid>`) links to the app, which opens that session; see
/// [`session_link`]. Nothing else links, and neither does [`worker_unlinked`], whose caller is
/// already inside a link.
fn named(name: Option<&str>, title: Option<String>) -> String {
    let shown = named_unlinked(name, title);
    match name {
        Some(name) if is_desktop_session(name) => session_link(name, &shown),
        _ => shown,
    }
}

/// [`worker`] without the link to the desktop app, for a name shown inside another link, such
/// as a ticket card, where a nested link is not valid HTML.
pub fn worker_unlinked(name: Option<&str>, ticket: &str) -> String {
    let title = name
        .and_then(session_title)
        .filter(|title| !names_another_ticket(title, ticket));
    named_unlinked(name, title)
}

/// Whether `name` is a Claude desktop session's ID, `local_` and a UUID.
pub(crate) fn is_desktop_session(name: &str) -> bool {
    name.strip_prefix("local_").is_some_and(|rest| {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
    })
}

/// `html` as a link that opens the desktop session `session` in the Claude app.
pub(crate) fn session_link(session: &str, html: &str) -> String {
    format!(
        "<a class=\"c-session-link\" href=\"claude://claude.ai/epitaxy/{}\">{html}</a>",
        c::esc(session)
    )
}

fn named_unlinked(name: Option<&str>, title: Option<String>) -> String {
    let Some(name) = name.filter(|name| !name.is_empty()) else {
        return c::muted("Nobody");
    };
    if let Some(title) = title {
        return c::tip(&c::esc(&title), name);
    }
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

/// The percentage of estimated work that is closed, with a breakdown on hover or focus.
pub fn work_progress(forecast: Option<&WorkRemaining>, default: EstimateDefault) -> String {
    forecast.map_or_else(String::new, |work| {
        let percent = work.percent_complete(default);
        c::estimate_progress(
            percent,
            percent < 100 && work.uses_default(),
            &work.breakdown(default),
        )
    })
}

/// The name in the header and page titles. The dashboard is the hub's root, so no project name
/// appears in its addresses to derive one from.
pub fn brand() -> String {
    "Ignite".to_owned()
}

/// A local allocation when a card has one.
pub fn allocation_chip(allocation: Option<u32>) -> String {
    match allocation {
        None => String::new(),
        Some(0) => c::chip("Parked", Tone::Muted, Some("Allocation 0")),
        Some(n) => c::chip(&n.to_string(), Tone::Neutral, Some("Allocation")),
    }
}

/// A card's ID and allocation.
pub fn ticket_heading(ticket: &Card) -> String {
    format!(
        "<div class=\"c-card-top\">{}{}</div>",
        c::mono(&display_id(&ticket.id)),
        allocation_chip(
            ticket
                .allocation
                .filter(|_| ticket.status != Status::PendingApproval)
        ) + &super::pins::chips(&ticket.pins)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A ticket's lifecycle reads as gray (pending approval), orange (approved), blue (in
    /// progress) and green (done).
    #[test]
    fn status_dots_follow_the_ticket_lifecycle_colors() {
        for (status, tone) in [
            (Status::PendingApproval, "c-tone-muted"),
            (Status::Open, "c-tone-orange"),
            (Status::InProgress, "c-tone-blue"),
            (Status::Closed, "c-tone-green"),
        ] {
            assert!(status_dot(status, true).contains(tone), "{status:?}");
        }
    }
}
