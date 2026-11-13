//! The spec server's bug report routes' guard and download: who may write or fetch reports,
//! and the bundle of the active ones (`GET /bug-reports.zip`), and `POST /report`, which files
//! a spec page's note as one.

use std::collections::HashMap;
use std::io::{self, Seek, Write};
use std::net::TcpStream;

use beryl_session::Sessions;
use beryl_session::bugs::{self, BugReport, Images, PageInfo, SpecMoment};
use serde_json::{Value, json};

use super::super::{View, find};
use super::{Instances, checkpoint, instance_key, lock};
use crate::serve::respond;

/// Report writes can create local beads, and bundles contain private diagnostics.
/// Accept the loopback Vite proxy's same-origin requests and local command-line clients.
pub(super) fn local_request(headers: &HashMap<String, String>) -> bool {
    let Some(host) = headers.get("host") else {
        return false;
    };
    let (name, port) = host.split_once(':').unwrap_or((host, "80"));
    matches!(name, "localhost" | "127.0.0.1")
        && port.parse::<u16>().is_ok()
        && headers
            .get("origin")
            .is_none_or(|origin| origin.strip_prefix("http://") == Some(host))
        && headers
            .get("sec-fetch-site")
            .is_none_or(|site| matches!(site.as_str(), "same-origin" | "none"))
}

/// `GET /bug-reports.zip`: the active local reports, zipped.
pub(super) fn export(stream: &mut TcpStream) -> Result<(), String> {
    let bundle = (|| -> io::Result<_> {
        let mut bundle = tempfile::tempfile()?;
        let folders = bugs::archive::active(&bugs::dir())?;
        bugs::archive::write(&folders, &mut bundle)?;
        bundle.rewind()?;
        Ok((bundle.metadata()?.len(), bundle))
    })();
    let (length, mut bundle) = match bundle {
        Ok(bundle) => bundle,
        Err(error) => {
            eprintln!("specs: could not export bug reports: {error}");
            return respond(stream, "500 Internal Server Error", "{}");
        }
    };
    // Keep the ZIP off the spec API's cross-origin JSON response path.
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: application/zip\r\n\
         Content-Disposition: attachment; filename=beryl-bug-reports.zip\r\n\
         Cache-Control: no-store\r\nCross-Origin-Resource-Policy: same-origin\r\n\
         Content-Length: {length}\r\nConnection: close\r\n\r\n"
    )
    .and_then(|()| io::copy(&mut bundle, stream).map(|_| ()))
    .map_err(|e| e.to_string())
}

/// `POST /report`: a note from the spec page about the moment an instance plays, written as a
/// bug report with what its game knows, if it still runs.
pub(super) fn report(instances: &Instances, body: &[u8]) -> Value {
    let request: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let (Some(key), Some(description)) = (request["key"].as_str(), request["description"].as_str())
    else {
        return json!({ "err": { "kind": "badRequest", "message": "want key and description" } });
    };
    if description.trim().is_empty() {
        return json!({ "err": { "kind": "badRequest", "message": "an empty note" } });
    }
    let mut parts = key.splitn(4, '/');
    let (Some(card), Some(moment), Some(view), Some(id)) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return json!({ "err": { "kind": "badRequest", "message": format!("no moment in {key}") } });
    };
    if (find(card, moment).is_none()
        && checkpoint(card, moment).is_none()
        && super::super::playground::find(card, moment).is_none())
        || View::parse(view).is_none()
    {
        return json!({ "err": { "kind": "spec", "message": format!("no moment {key}") } });
    }
    let spec = SpecMoment {
        card: card.to_owned(),
        moment: moment.to_owned(),
        view: view.to_owned(),
    };
    let viewer_page: Option<PageInfo> = match serde_json::from_value(request["viewerPage"].clone())
    {
        Ok(page) => page,
        Err(e) => return json!({ "err": { "kind": "badRequest", "message": e.to_string() } }),
    };
    let spec_viewer = viewer_page.is_some();
    let page = viewer_page.unwrap_or_else(|| PageInfo {
        url: format!(
            "/{}/{spec}",
            if moment.starts_with("playground-") {
                "playground"
            } else {
                "scenarios"
            }
        ),
        width: 0.0,
        height: 0.0,
        scale: 1.0,
    });
    let args = json!({ "request": {
        "description": description,
        "ai": false,
        "snapshot": null,
        "region": null,
        "page": page,
        "uiLog": [],
        "updates": [],
    }});
    let list = lock(instances);
    match list
        .iter()
        .find(|i| i.key == instance_key(card, moment, view, id))
    {
        Some(i) => {
            let feed = if view == "theirs" {
                i.mirror.as_ref().unwrap_or(&i.feed)
            } else {
                &i.feed
            };
            let mut args = args;
            args["request"]["updates"] = json!(lock(&feed.0).history);
            write_report(&i.sessions, args, spec, spec_viewer)
        }
        None => {
            json!({ "err": { "kind": "ended", "message": "This board expired. Reset it before reporting." } })
        }
    }
}

/// Writes the `report_bug` request in `args` as a report about `spec`, with what `sessions`
/// knows of its game.
pub(super) fn write_report(
    sessions: &Sessions,
    args: Value,
    spec: SpecMoment,
    spec_viewer: bool,
) -> Value {
    #[derive(serde::Deserialize)]
    struct Request {
        request: BugReport,
    }
    let mut report = match serde_json::from_value::<Request>(args) {
        Ok(r) => r.request,
        Err(e) => return json!({ "err": { "kind": "badRequest", "message": e.to_string() } }),
    };
    report.spec = Some(spec);
    report.spec_viewer = spec_viewer;
    let root = bugs::dir();
    match bugs::write(
        &root,
        &report,
        &Images::default(),
        sessions,
        &bugs::Environment::current(),
    ) {
        Ok(saved) => {
            bugs::filing::enqueue(&root, &saved.id, None);
            json!({ "ok": saved })
        }
        Err(e) => json!({ "err": e }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_routes_only_accept_local_requests() {
        let headers = |host: &str, origin: &str, site: &str| {
            [
                ("host".to_owned(), host.to_owned()),
                ("origin".to_owned(), origin.to_owned()),
                ("sec-fetch-site".to_owned(), site.to_owned()),
            ]
            .into_iter()
            .collect()
        };
        assert!(local_request(&headers(
            "localhost:1430",
            "http://localhost:1430",
            "same-origin"
        )));
        assert!(!local_request(&headers(
            "localhost:1430",
            "http://localhost:9999",
            "same-site"
        )));
        assert!(!local_request(&headers(
            "example.org:1430",
            "http://example.org:1430",
            "same-origin"
        )));
        let mut navigation = HashMap::from([("host".to_owned(), "127.0.0.1:1430".to_owned())]);
        assert!(local_request(&navigation));
        navigation.insert("sec-fetch-site".to_owned(), "cross-site".to_owned());
        assert!(!local_request(&navigation));
    }
}
