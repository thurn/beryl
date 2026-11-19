//! Ticket owners on the dashboard (see `ignite_core::owner`): the Owner row of a ticket's
//! properties, with its dropdown on an editable page, and the owner chip on a ticket's card.
//! The default owner is picked in the header's settings menu, which `static/owners.js` fills
//! from `GET /api/owners`.

use rusqlite::{Connection, OptionalExtension};

use ignite_core::owner::Owner;
use ignite_store::hub::owners;

use super::components::{self as c, Tone, esc};
use super::data::ReadError;

/// Who works a ticket, as its page shows it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OwnerView {
    /// The ticket's own owner, as stored.
    pub own: Option<String>,
    /// The owner in force, with its app resolved: its own, an ancestor's or the default.
    pub effective: Option<Owner>,
    /// The ancestor whose owner it follows, when it follows one.
    pub inherited_from: Option<String>,
    /// The default owner, which a ticket with no owner of its own or an ancestor's follows.
    pub default: Option<Owner>,
    /// Every owner a person may pick.
    pub choices: Vec<Owner>,
}

/// An owner as a person reads it: `jarvis · Claude`.
pub fn label(owner: &Owner) -> String {
    match &owner.app {
        Some(app) => {
            let mut chars = app.chars();
            let app: String = chars
                .next()
                .map(|first| first.to_uppercase().chain(chars).collect())
                .unwrap_or_default();
            format!("{} · {app}", owner.machine)
        }
        None => owner.machine.clone(),
    }
}

/// The chip a card shows for a ticket with an owner of its own; none for one that follows its
/// ancestors or the default.
pub fn chip(owner: Option<&str>) -> String {
    owner
        .and_then(|text| text.parse::<Owner>().ok())
        .map_or_else(String::new, |owner| {
            c::chip(&label(&owner), Tone::Muted, None)
        })
}

/// A ticket's parent and its own owner.
type ParentAndOwner = (Option<String>, Option<String>);

/// The owner view of ticket `id`, walking its ancestors for an inherited owner.
pub fn view(conn: &Connection, id: &str) -> Result<OwnerView, ReadError> {
    let routing = owners::routing(conn)?;
    let default = owners::default_owner(conn, &routing)?;
    let row = |id: &str| -> Result<Option<ParentAndOwner>, ReadError> {
        Ok(conn
            .query_row(
                "SELECT parent, owner FROM tickets WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    };
    let (mut parent, own) = row(id)?.unwrap_or_default();
    let mut effective = own.as_deref().and_then(|text| text.parse::<Owner>().ok());
    let mut inherited_from = None;
    let mut steps = 0;
    while effective.is_none()
        && steps < 32
        && let Some(ancestor) = parent.take()
    {
        let (next, owner) = row(&ancestor)?.unwrap_or_default();
        if let Some(owner) = owner.and_then(|text| text.parse::<Owner>().ok()) {
            effective = Some(owner);
            inherited_from = Some(ancestor);
        }
        parent = next;
        steps += 1;
    }
    let resolve = |owner: Owner| owner.resolved(&routing);
    Ok(OwnerView {
        own,
        effective: effective.or_else(|| default.clone()).map(resolve),
        inherited_from,
        default: default.map(resolve),
        choices: routing.choices(),
    })
}

/// The Owner property's value: the owner in force and where it comes from.
pub fn property_view(view: &OwnerView, link: impl Fn(&str) -> String) -> String {
    let Some(effective) = &view.effective else {
        return c::muted("Any machine");
    };
    let source = if view.own.is_some() {
        String::new()
    } else if let Some(ancestor) = &view.inherited_from {
        format!(" <span class=\"c-muted\">from {}</span>", link(ancestor))
    } else {
        format!(" {}", c::muted("default"))
    };
    format!("{}{source}", esc(&label(effective)))
}

/// The choices of the Owner dropdown as `value=label` lines, the first being "Default", which
/// clears the ticket's own owner. `edit.js` builds the dropdown from them.
pub fn choices(view: &OwnerView) -> String {
    let default = view.default.as_ref().map_or_else(
        || "Default".to_owned(),
        |d| format!("Default ({})", label(d)),
    );
    std::iter::once(format!("={default}"))
        .chain(view.choices.iter().map(|o| format!("{o}={}", label(o))))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_owner_reads_as_machine_and_app() {
        assert_eq!(label(&"jarvis/codex".parse().unwrap()), "jarvis · Codex");
        assert_eq!(label(&"cygnus".parse().unwrap()), "cygnus");
        assert!(chip(None).is_empty());
        assert!(chip(Some("multivac/claude")).contains("multivac · Claude"));
    }
}
