//! Routing for the dashboard's ticket views: which request is a page, a fragment, a redirect or
//! a static file, and the headers each carries.
//!
//! A page's `ETag` is a hash of its HTML, so a browser revalidating after a ticket changes gets
//! `304 Not Modified` exactly when the page would be the same.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use rusqlite::Connection;
use sha2::{Digest, Sha256};

use ignite_core::time::Timestamp;
use ignite_store::hub::{feed, tickets};

use super::attention::PAGES;
use super::common::{SITE_ICONS, brand};
use super::components::{self as c, PageOptions, esc};
use super::data::ReadError;
use super::references::{dashboard_root, percent_encode, ticket_url};
use super::{
    assets, available, explore, home, landings, machines, pool, session, status, ticket, tips,
};
use crate::routes::{Request, Response};

/// What a path under the project names.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// `/`.
    Home,
    /// A page listed in the dashboard's navigation.
    Fleet(&'static str),
    /// The old address of the Fleet page.
    MachinesRedirect,
    /// `/pool/session/<id>`: the first address of a CLI worker's transcript, which redirects to
    /// the transcript under the session's ticket.
    PoolSession(String),
    /// `/<ticket>/session` or `/<ticket>/session/<id>`: a CLI worker's transcript on a ticket,
    /// the ticket's latest when no session is named, served on the private listener only.
    Session(String, Option<String>),
    /// `/<name>`, with an optional trailing slash: a ticket's page, or a redirect to it from
    /// another name for it.
    Ticket(String),
}

/// The fleet page that `path` (`/fleet`, with an optional trailing slash) names.
fn fleet_page(path: &str) -> Option<&'static str> {
    let name = path.strip_prefix('/')?;
    let name = name.strip_suffix('/').unwrap_or(name);
    PAGES
        .iter()
        .find(|(page, _)| *page == name)
        .map(|(page, _)| *page)
}

fn route(path: &str) -> Option<Route> {
    if path == "/" {
        return Some(Route::Home);
    }
    if matches!(path, "/machines" | "/machines/") {
        return Some(Route::MachinesRedirect);
    }
    if let Some(page) = fleet_page(path) {
        return Some(Route::Fleet(page));
    }
    if let Some(id) = path.strip_prefix("/pool/session/") {
        let id = id.strip_suffix('/').unwrap_or(id);
        return (!id.is_empty() && !id.contains('/')).then(|| Route::PoolSession(id.to_owned()));
    }
    if let Some((name, rest)) = path
        .strip_prefix('/')
        .and_then(|path| path.split_once("/session"))
    {
        let rest = rest.strip_suffix('/').unwrap_or(rest);
        let session = match rest.strip_prefix('/') {
            None if rest.is_empty() => None,
            Some(id) if !id.is_empty() && !id.contains('/') => Some(id.to_owned()),
            _ => return None,
        };
        return (!name.is_empty() && !name.contains('/'))
            .then(|| Route::Session(name.to_owned(), session));
    }
    // `/tickets/<name>` is the address the dashboard gave a ticket until its pages moved to the
    // root, and `/beads/<id>.html` the address of the first dashboard.
    let (name, legacy) = if let Some(name) = path.strip_prefix("/tickets/") {
        (name, false)
    } else if let Some(name) = path.strip_prefix("/beads/") {
        (name, true)
    } else {
        (path.strip_prefix('/')?, false)
    };
    let name = name.strip_suffix('/').unwrap_or(name);
    let name = if legacy {
        name.strip_suffix(".html").unwrap_or(name)
    } else {
        name
    };
    (!name.is_empty() && !name.contains('/')).then(|| Route::Ticket(name.to_owned()))
}

/// The redirect for a page address under the project's name or one of its older names
/// (`aliases`), which is the same address at the hub's root (`/by/Tq44s.7` is `/Tq44s.7`). It is
/// `None` when `path` is not under one of those names or is an API address, which stays under
/// `/<project>/api`. `path` is the whole request path, and the redirect keeps the query.
pub fn alias_redirect(
    path: &str,
    query: &[(String, String)],
    project: &str,
    aliases: &[String],
) -> Option<Response> {
    let rest = path.strip_prefix('/')?;
    let (name, rest) = rest.find('/').map_or((rest, ""), |at| rest.split_at(at));
    if (name != project && !aliases.iter().any(|alias| alias == name))
        || rest == "/api"
        || rest.starts_with("/api/")
    {
        return None;
    }
    Some(redirect(if rest.is_empty() { "/" } else { rest }, query))
}

/// The hub's change-feed position, as the page records it.
pub(crate) fn feed_position(conn: &Connection) -> String {
    feed::current(conn).map_or_else(|_| "0".to_owned(), |seq| seq.0.to_string())
}

fn html(status: u16, body: String, head: bool) -> Response {
    Response {
        status,
        content_type: "text/html; charset=utf-8",
        body: if head { Vec::new() } else { body.into_bytes() },
        headers: Vec::new(),
    }
}

fn etag(body: &str) -> String {
    let digest = Sha256::digest(body.as_bytes());
    format!(
        "W/\"{}\"",
        digest
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    )
}

fn matches(if_none_match: Option<&str>, tag: &str) -> bool {
    let bare = tag.strip_prefix("W/").unwrap_or(tag);
    if_none_match.is_some_and(|value| {
        value.split(',').any(|candidate| {
            let candidate = candidate.trim();
            candidate == "*" || candidate.strip_prefix("W/").unwrap_or(candidate) == bare
        })
    })
}

fn redirect(path: &str, query: &[(String, String)]) -> Response {
    let pairs: Vec<String> = query
        .iter()
        .map(|(key, value)| format!("{}={}", percent_encode(key), percent_encode(value)))
        .collect();
    let location = if pairs.is_empty() {
        path.to_owned()
    } else {
        format!("{path}?{}", pairs.join("&"))
    };
    Response {
        status: 308,
        content_type: "text/html; charset=utf-8",
        body: Vec::new(),
        headers: vec![("Location", location)],
    }
}

fn fragment(seq: &str, body: String, head: bool) -> Response {
    html(200, body, head)
        .with_header("Cache-Control", "no-store".to_owned())
        .with_header("X-Cirrus-Sequence", seq.to_owned())
}

/// The efficiency section of the ticket `id`'s page (see [`super::spend_view`]): its windows and
/// those of every ticket under it. Commands show only on an `editable` (private) page.
fn spend_fragment(
    conn: &Connection,
    root: &str,
    id: &str,
    editable: bool,
) -> Result<String, ReadError> {
    let ids = super::spend::subtree(conn, id)?;
    Ok(match super::spend::load(conn, id, &ids)? {
        Some(spend) => super::spend_view::fragment(&spend, root, editable, id),
        None => c::empty("No sessions have reported spending on this ticket yet."),
    })
}

fn page_reply(options: &PageOptions, if_none_match: Option<&str>, head: bool) -> Response {
    let body = c::document(options, &c::page_regions(options));
    // The feed position changes with every write anywhere, so it is left out of the tag: a page
    // whose own content did not change revalidates to `304`, and its older position only makes
    // the script ask for one fragment it did not need.
    let mut stable = options.clone();
    if let Some(feed) = stable.feed.as_mut() {
        feed.seq.clear();
    }
    let tag = etag(&c::document(&stable, &c::page_regions(&stable)));
    let unchanged = matches(if_none_match, &tag);
    html(if unchanged { 304 } else { 200 }, body, head || unchanged)
        .with_header("Cache-Control", "no-cache".to_owned())
        .with_header("ETag", tag)
}

fn regions(options: &PageOptions) -> String {
    format!(
        "<title>{}</title>{}",
        esc(&options.title),
        c::page_regions(options)
    )
}

fn not_found(root: &str, wordmark: Option<&str>, head: bool) -> Response {
    let body = c::title("Not found", "", "No ticket has this address.")
        + &format!(
            "<p>{}</p>",
            c::link(root, "Back to tickets", None, "c-muted")
        );
    let options = PageOptions {
        title: format!("Not found · {}", brand()),
        body,
        root: root.to_owned(),
        asset_root: Some("/".to_owned()),
        icons: Some(SITE_ICONS),
        crumbs: vec![("Tickets".to_owned(), Some(root.to_owned()))],
        brand: brand(),
        wordmark: wordmark.map(str::to_owned),
        ..PageOptions::default()
    };
    let page = c::document(&options, &c::page_regions(&options));
    html(404, page, head).with_header("Cache-Control", "no-store".to_owned())
}

fn failed(error: &impl std::fmt::Display) -> Response {
    Response {
        status: 500,
        content_type: "text/plain; charset=utf-8",
        body: format!("{error}\n").into_bytes(),
        headers: vec![("Cache-Control", "no-store".to_owned())],
    }
}

/// The dashboard's static file at `path` (such as `/assets/cirrus.<hash>.css`), for a `GET` or
/// `HEAD` request. These live at the hub's root, not under the project.
pub fn static_file(method: &str, path: &str) -> Option<Response> {
    let head = method.eq_ignore_ascii_case("HEAD");
    if !head && !method.eq_ignore_ascii_case("GET") {
        return None;
    }
    let file = assets::serve(path.trim_start_matches('/'))?;
    Some(Response {
        status: 200,
        content_type: file.content_type,
        body: if head { Vec::new() } else { file.body },
        headers: vec![("Cache-Control", file.cache_control.to_owned())],
    })
}

/// Answers a `GET` or `HEAD` for a ticket view of `project`: the home page, a ticket's page or
/// the fragments the script loads. `request.path` is the whole request path. Returns `None` for
/// any other request.
pub fn serve(
    conn: &Connection,
    request: &Request,
    project: &str,
    now: Timestamp,
) -> Option<Response> {
    let site = Site {
        project,
        wordmark: None,
        editable: false,
        private: false,
    };
    serve_with(conn, request, &site, now)
}

/// How the pages of one listener are rendered.
#[derive(Clone, Copy, Debug, Default)]
pub struct Site<'a> {
    pub project: &'a str,
    /// HTML shown at the left of every page's header in place of the name "Ignite"; see
    /// `HubServer::wordmark`.
    pub wordmark: Option<&'a str>,
    /// Whether pages carry edit controls. Only requests from Tailscale users on the private
    /// listener can edit (see `crate::auth`).
    pub editable: bool,
    /// Whether this is the private listener. A transcript can hold secrets, so the worker
    /// session page is served only there; the public copy answers 404 for it. Ticket images
    /// are shown only there too, for the same reason.
    pub private: bool,
}

/// Like [`serve`], for the listener `site`.
pub fn serve_with(
    conn: &Connection,
    request: &Request,
    site: &Site,
    now: Timestamp,
) -> Option<Response> {
    // A database without the table, such as an old copy in a preview, names sessions by id.
    let titles = ignite_store::hub::session_titles::all(conn).unwrap_or_default();
    super::common::with_session_titles(titles, || serve_pages(conn, request, site, now))
}

fn serve_pages(
    conn: &Connection,
    request: &Request,
    site: &Site,
    now: Timestamp,
) -> Option<Response> {
    let Site {
        project,
        wordmark,
        editable,
        private,
    } = *site;
    let head = request.method.eq_ignore_ascii_case("HEAD");
    if !head && !request.method.eq_ignore_ascii_case("GET") {
        return None;
    }
    let route = route(&request.path)?;
    let if_none_match = request.header("if-none-match");
    let fragment_kind = request.param("fragment");
    let root = dashboard_root();
    let seq = feed_position(conn);
    let brand = |options: PageOptions| PageOptions {
        wordmark: wordmark.map(str::to_owned),
        ..options
    };
    Some(match route {
        Route::Home => {
            let panel = || status::fragment(conn, &root, now);
            if fragment_kind == Some("status") {
                return Some(
                    panel().map_or_else(|error| failed(&error), |body| fragment(&seq, body, head)),
                );
            }
            match home::options(conn, &root, &seq, now, editable.then_some(project)).map(brand) {
                Err(error) => failed(&error),
                Ok(options) if fragment_kind == Some("all") => match panel() {
                    Err(error) => failed(&error),
                    Ok(body) => {
                        let body = format!(
                            "{}<template id=\"c-status-fragment\">{body}</template>",
                            regions(&options)
                        );
                        fragment(&seq, body, head)
                    }
                },
                Ok(options) => page_reply(&options, if_none_match, head),
            }
        }
        Route::MachinesRedirect => redirect("/fleet", &request.query),
        Route::Fleet(page) => {
            let built = match page {
                available::PATH => available::options(conn, &root, &seq, now),
                "fleet" => machines::options(conn, &root, &seq, now, editable.then_some(project)),
                "pool" => {
                    pool::options(conn, &root, &seq, private, now, editable.then_some(project))
                }
                "epics" => pool::epics(conn, &root, &seq, now),
                "landings" => landings::options(conn, &root, &seq, now),
                "tips" => tips::options(conn, &root, &seq, now),
                _ => explore::options(conn, &root, &seq, now),
            };
            match built.map(brand) {
                Err(error) => failed(&error),
                Ok(options) if fragment_kind == Some("all") => {
                    fragment(&seq, regions(&options), head)
                }
                Ok(options) => page_reply(&options, if_none_match, head),
            }
        }
        Route::PoolSession(_) | Route::Session(..) => {
            if !private {
                return Some(not_found(&root, wordmark, head));
            }
            session_reply(conn, request, route, site, (&root, &seq))
        }
        Route::Ticket(name) => {
            let id = match tickets::resolve(conn, &name) {
                Ok(Some(id)) => id,
                Ok(None) => return Some(not_found(&root, wordmark, head)),
                Err(error) => return Some(failed(&error)),
            };
            let canonical = ticket_url(&root, id.as_str());
            if canonical != request.path {
                return Some(redirect(&canonical, &request.query));
            }
            if fragment_kind == Some("spend") {
                return Some(
                    spend_fragment(conn, &root, id.as_str(), editable)
                        .map_or_else(|error| failed(&error), |body| fragment(&seq, body, head)),
                );
            }
            match ticket::options(conn, &root, project, id.as_str(), &seq, (editable, private))
                .map(|o| o.map(brand))
            {
                Err(error) => failed(&error),
                Ok(None) => not_found(&root, wordmark, head),
                Ok(Some(options)) if fragment_kind == Some("all") => {
                    fragment(&seq, regions(&options), head)
                }
                Ok(Some(options)) => page_reply(&options, if_none_match, head),
            }
        }
    })
}

/// The reply on the private listener for a session route: the redirect from the page's first
/// address or from another name of the ticket, or the page of the ticket's session.
fn session_reply(
    conn: &Connection,
    request: &Request,
    route: Route,
    site: &Site,
    (root, seq): (&str, &str),
) -> Response {
    let head = request.method.eq_ignore_ascii_case("HEAD");
    let missing = || not_found(root, site.wordmark, head);
    let (name, id) = match route {
        Route::PoolSession(id) => {
            return match session::ticket_of(conn, &id) {
                Err(error) => failed(&error),
                Ok(None) => missing(),
                Ok(Some(ticket)) => {
                    redirect(&session::session_url(root, &ticket, &id), &request.query)
                }
            };
        }
        Route::Session(name, id) => (name, id),
        _ => return missing(),
    };
    let ticket = match tickets::resolve(conn, &name) {
        Ok(Some(ticket)) => ticket,
        Ok(None) => return missing(),
        Err(error) => return failed(&error),
    };
    let canonical = match &id {
        Some(id) => session::session_url(root, ticket.as_str(), id),
        None => session::ticket_sessions_url(root, ticket.as_str()),
    };
    if canonical != request.path {
        return redirect(&canonical, &request.query);
    }
    let built = session::options(
        conn,
        root,
        site.project,
        ticket.as_str(),
        id.as_deref(),
        site.editable,
    );
    match built {
        Err(error) => failed(&error),
        Ok(None) => missing(),
        Ok(Some(options)) => {
            let options = PageOptions {
                wordmark: site.wordmark.map(str::to_owned),
                ..options
            };
            if request.param("fragment") == Some("all") {
                fragment(seq, regions(&options), head)
            } else {
                page_reply(&options, request.header("if-none-match"), head)
            }
        }
    }
}
