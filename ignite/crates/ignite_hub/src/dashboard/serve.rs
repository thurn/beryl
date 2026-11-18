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
use super::references::{dashboard_root, percent_encode, ticket_url};
use super::{assets, explore, home, landings, machines, pool, status, ticket, tips};
use crate::routes::{Request, Response};

/// What a path under the project names.
#[derive(Debug, PartialEq, Eq)]
enum Route {
    /// `/`.
    Home,
    /// One of the fleet pages (`machines`, `pool`, `epics`, `landings`, `tips`, `finds`).
    Fleet(&'static str),
    /// `/<name>`, with an optional trailing slash: a ticket's page, or a redirect to it from
    /// another name for it.
    Ticket(String),
}

/// The fleet page that `path` (`/machines`, with an optional trailing slash) names.
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
    if let Some(page) = fleet_page(path) {
        return Some(Route::Fleet(page));
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
    serve_with(conn, request, project, None, now)
}

/// Like [`serve`], with `wordmark` (HTML) shown at the left of every page's header in place of
/// the name "Ignite"; see `HubServer::wordmark`.
pub fn serve_with(
    conn: &Connection,
    request: &Request,
    project: &str,
    wordmark: Option<&str>,
    now: Timestamp,
) -> Option<Response> {
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
            match home::options(conn, &root, &seq, now).map(brand) {
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
        Route::Fleet(page) => {
            let built = match page {
                "machines" => machines::options(conn, &root, &seq, now),
                "pool" => pool::options(conn, &root, &seq, now),
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
            match ticket::options(conn, &root, project, id.as_str(), &seq).map(|o| o.map(brand)) {
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
