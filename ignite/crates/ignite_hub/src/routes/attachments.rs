//! The attachment routes (see `ignite_core::protocol::attachments`):
//!
//! - `POST /tickets/<id>/attachments?name=<name>&caption=<text>` stores the request body, an
//!   image, as an attachment of the ticket and answers its row. It needs a write token.
//! - `GET /tickets/<id>/attachments/<name>` answers the file's bytes, for `ignite ticket
//!   attachment get`.
//!
//! The dashboard shows a file at `/attachments/<sha256>` ([`serve_file`]). A pasted screenshot
//! can show anything on a person's screen, so every one of these answers only on the private
//! listener; the public read-only copy answers 404, as it does for transcripts.
//!
//! An upload is refused with 413 when it is over [`MAX_ATTACHMENT_BYTES`], with 415 when it is
//! not a PNG, JPEG, GIF or WebP (judged by its bytes), and with 507 when the files of open
//! tickets alone would then be over the budget.

use rusqlite::Connection;
use serde_json::json;

use ignite_core::protocol::attachments::{
    MAX_ATTACHMENT_BYTES, clean_name, size_text, sniff_attachment,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::{attachments, tickets};

use super::{Request, Response};
use crate::attachments::{AttachmentFiles, is_sha256, sha256_hex};

/// The longest caption kept, in characters.
const MAX_CAPTION_CHARS: usize = 500;

/// How long a browser may keep a file: its address is its hash, so it never changes.
const IMMUTABLE: &str = "private, max-age=31536000, immutable";

/// The response if this module owns the request's path. `private` says the request came on the
/// private listener.
pub fn handle(
    conn: &mut Connection,
    request: &Request,
    private: bool,
    files: &AttachmentFiles,
    now: Timestamp,
) -> Option<Response> {
    let rest = request.path.strip_prefix("/tickets/")?;
    let (ticket, rest) = rest.split_once('/')?;
    let name = match rest {
        "attachments" => None,
        _ => Some(rest.strip_prefix("attachments/")?),
    };
    if ticket.is_empty() || name.is_some_and(|name| name.is_empty() || name.contains('/')) {
        return None;
    }
    if !private {
        return Some(failure(404, "Unknown route"));
    }
    Some(match (name, request.method.as_str()) {
        (None, "POST") => upload(conn, request, ticket, files, now),
        (Some(name), "GET" | "HEAD") => download(conn, request, ticket, name, files),
        (None, _) => failure(405, "use POST for /tickets/<id>/attachments"),
        (Some(_), _) => failure(405, "use GET for /tickets/<id>/attachments/<name>"),
    })
}

fn upload(
    conn: &mut Connection,
    request: &Request,
    ticket: &str,
    files: &AttachmentFiles,
    now: Timestamp,
) -> Response {
    let Some(writer) = &request.writer else {
        return failure(401, "Missing or unknown token");
    };
    let id = match tickets::resolve(conn, ticket) {
        Ok(Some(id)) => id,
        Ok(None) => return failure(404, &format!("Ticket {ticket} not found")),
        Err(error) => return failure(500, &error.to_string()),
    };
    let body = &request.body;
    if body.len() as u64 > MAX_ATTACHMENT_BYTES {
        return failure(
            413,
            &format!(
                "the attachment is {}, over the limit of {}",
                size_text(body.len() as u64),
                size_text(MAX_ATTACHMENT_BYTES)
            ),
        );
    }
    let requested_name = request.param("name").unwrap_or("");
    let Some(image) = sniff_attachment(requested_name, body) else {
        return failure(
            415,
            "only PNG, JPEG, GIF, WebP, JSON, JSON Lines, plain text and gzip files can be attached",
        );
    };
    let sha256 = sha256_hex(body);
    match fits(conn, files, &sha256, body.len() as u64) {
        Ok(None) => {}
        Ok(Some(full)) => return failure(507, &full),
        Err(error) => return failure(500, &error.to_string()),
    }
    if let Err(error) = files.write(body) {
        return failure(500, &format!("storing the attachment: {error}"));
    }
    let name = clean_name(
        if requested_name.is_empty() {
            "image"
        } else {
            requested_name
        },
        image.extension,
    );
    let caption: Option<String> = request
        .param("caption")
        .map(|text| text.trim().chars().take(MAX_CAPTION_CHARS).collect())
        .filter(|text: &String| !text.is_empty());
    let upload = attachments::Upload {
        ticket: &id,
        name: &name,
        sha256: &sha256,
        image,
        bytes: body.len() as u64,
        caption: caption.as_deref(),
    };
    match attachments::insert(conn, &upload, &writer.actor, now) {
        Ok(stored) => Response::json(200, &json!(stored)),
        Err(error) => failure(500, &error.to_string()),
    }
}

/// Why a new file of `bytes` does not fit, or `None` when it does: the file is stored already,
/// or the files of open tickets with it stay within the budget. Files of closed tickets do not
/// count, since upkeep removes them to make room.
fn fits(
    conn: &Connection,
    files: &AttachmentFiles,
    sha256: &str,
    bytes: u64,
) -> ignite_store::Result<Option<String>> {
    if attachments::kept_media_type(conn, sha256)?.is_some() {
        return Ok(None);
    }
    let open = attachments::kept_bytes(conn, true)?;
    if open.saturating_add(bytes) <= files.budget() {
        return Ok(None);
    }
    Ok(Some(format!(
        "attachment storage is full: open tickets keep {} of images and the budget \
         (`[hub] attachment_budget_mb`) is {}",
        size_text(open),
        size_text(files.budget())
    )))
}

fn download(
    conn: &Connection,
    request: &Request,
    ticket: &str,
    name: &str,
    files: &AttachmentFiles,
) -> Response {
    let id = match tickets::resolve(conn, ticket) {
        Ok(Some(id)) => id,
        Ok(None) => return failure(404, &format!("Ticket {ticket} not found")),
        Err(error) => return failure(500, &error.to_string()),
    };
    let found = match attachments::get(conn, id.as_str(), name) {
        Ok(Some(found)) => found,
        Ok(None) => return failure(404, &format!("{id} has no attachment {name}")),
        Err(error) => return failure(500, &error.to_string()),
    };
    if let Some(at) = found.removed_at {
        return failure(410, &format!("the file of {name} was removed at {at}"));
    }
    bytes(request, files, &found.sha256, &found.media_type, None)
}

/// The dashboard's `GET /attachments/<sha256>`: the file, while some attachment keeps it, on the
/// private listener only.
pub fn serve_file(
    conn: &Connection,
    request: &Request,
    sha256: &str,
    private: bool,
    files: &AttachmentFiles,
) -> Response {
    if !private || !is_sha256(sha256) {
        return failure(404, "Unknown route");
    }
    if !matches!(request.method.as_str(), "GET" | "HEAD") {
        return failure(405, "use GET for /attachments/<sha256>");
    }
    match attachments::kept_media_type(conn, sha256) {
        Ok(Some(media_type)) => bytes(request, files, sha256, &media_type, Some(IMMUTABLE)),
        Ok(None) => failure(404, "no attachment keeps this file"),
        Err(error) => failure(500, &error.to_string()),
    }
}

fn bytes(
    request: &Request,
    files: &AttachmentFiles,
    sha256: &str,
    media_type: &str,
    cache: Option<&str>,
) -> Response {
    let body = match files.read(sha256) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return failure(410, "the file is no longer stored");
        }
        Err(error) => return failure(500, &error.to_string()),
    };
    let mut response = Response {
        status: 200,
        content_type: static_media_type(media_type),
        body: if request.method == "HEAD" {
            Vec::new()
        } else {
            body
        },
        headers: vec![("X-Content-Type-Options", "nosniff".to_owned())],
    };
    if let Some(cache) = cache {
        response = response.with_header("Cache-Control", cache.to_owned());
    }
    response
}

/// A stored media type as one of the types the hub accepts; anything else is served as bytes.
fn static_media_type(media_type: &str) -> &'static str {
    match media_type {
        "image/png" => "image/png",
        "image/jpeg" => "image/jpeg",
        "image/gif" => "image/gif",
        "image/webp" => "image/webp",
        "application/json" => "application/json",
        "application/x-ndjson" => "application/x-ndjson",
        "application/gzip" => "application/gzip",
        "text/plain" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}
