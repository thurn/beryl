//! The verdict routes: `/verdicts/lookup` and `/verdicts/store`. A verdict records that the
//! landing gate passed for one tree of files on one base under one set of gate settings (see
//! "Landing by verdict" in `docs/ignite.md`), and any machine that finds the same key lands
//! the change without running the gate again. The hub keeps each verdict as the JSON the
//! machine sent and reads only its key.
//!
//! `/verdicts/lookup` only reads and replies with the verdict or `null`; `/verdicts/store`
//! needs an authorized writer (HTTP 401 otherwise).

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::json;

use ignite_core::protocol::hub::{Verdict, VerdictKey};
use ignite_core::time::Timestamp;
use ignite_store::hub::verdicts;

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, _now: Timestamp) -> Option<Response> {
    let stores = match request.path.as_str() {
        "/verdicts/lookup" => false,
        "/verdicts/store" => true,
        _ => return None,
    };
    if request.method != "POST" {
        return Some(failure(405, &format!("use POST for {}", request.path)));
    }
    if stores && request.writer.is_none() {
        return Some(failure(401, "writes need a registered machine's token"));
    }
    let answer = if stores {
        body::<Verdict>(request).and_then(|verdict| {
            let text = serde_json::to_string(&verdict).map_err(|e| failure(500, &e.to_string()))?;
            verdicts::put(
                conn,
                &verdict.key.tree,
                verdict.key.base.as_str(),
                &verdict.key.settings,
                &text,
                &verdict.passed_at.to_string(),
            )
            .map(|()| Response::json(200, &json!({ "ok": true })))
            .map_err(|e| failure(500, &e.to_string()))
        })
    } else {
        body::<VerdictKey>(request).and_then(|key| {
            let found = verdicts::get(conn, &key.tree, key.base.as_str(), &key.settings)
                .map_err(|e| failure(500, &e.to_string()))?;
            let verdict: Option<Verdict> = match found {
                Some(text) => {
                    Some(serde_json::from_str(&text).map_err(|e| failure(500, &e.to_string()))?)
                }
                None => None,
            };
            Ok(Response::json(200, &json!(verdict)))
        })
    };
    Some(answer.unwrap_or_else(|response| response))
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

fn body<T: DeserializeOwned>(request: &Request) -> Result<T, Response> {
    serde_json::from_slice(&request.body)
        .map_err(|error| failure(400, &format!("bad request body: {error}")))
}

#[cfg(test)]
mod tests {
    use ignite_core::ids::MachineId;
    use ignite_store::hub;

    use super::*;
    use crate::auth::Writer;

    fn request(path: &str, body: &impl serde::Serialize, writer: bool) -> Request {
        Request {
            method: "POST".to_owned(),
            path: path.to_owned(),
            query: Vec::new(),
            writer: writer.then(|| Writer {
                machine: MachineId::new("m-1").unwrap(),
                actor: "daemon".to_owned(),
            }),
            body: serde_json::to_vec(body).unwrap(),
            headers: Vec::new(),
        }
    }

    fn key(tree: &str, base: &str, settings: &str) -> VerdictKey {
        VerdictKey {
            tree: tree.to_owned(),
            base: base.parse().unwrap(),
            settings: settings.to_owned(),
        }
    }

    fn lookup(conn: &mut Connection, key: &VerdictKey) -> Option<Verdict> {
        let now = Timestamp::EPOCH;
        let reply = handle(conn, &request("/verdicts/lookup", key, false), now).unwrap();
        assert_eq!(reply.status, 200);
        serde_json::from_slice(&reply.body).unwrap()
    }

    #[test]
    fn a_stored_verdict_is_found_by_tree_base_and_settings_only() {
        let mut conn = hub::open_in_memory().unwrap();
        let now = Timestamp::EPOCH;
        let verdict = Verdict {
            key: key("tree-a", "3a9e4c1", "settings-a"),
            head: "4b0f5d2".parse().unwrap(),
            machine: "host".to_owned(),
            passed_at: now,
            families: vec!["unit".to_owned()],
            left_to_tip: 2,
            always_not_run: Vec::new(),
        };
        let refused = handle(&mut conn, &request("/verdicts/store", &verdict, false), now);
        assert_eq!(refused.unwrap().status, 401, "a store needs a writer");
        let done = handle(&mut conn, &request("/verdicts/store", &verdict, true), now);
        assert_eq!(done.unwrap().status, 200);
        assert_eq!(lookup(&mut conn, &verdict.key), Some(verdict.clone()));
        assert_eq!(
            lookup(&mut conn, &key("tree-b", "3a9e4c1", "settings-a")),
            None
        );
        assert_eq!(
            lookup(&mut conn, &key("tree-a", "5c1a6e3", "settings-a")),
            None
        );
        assert_eq!(
            lookup(&mut conn, &key("tree-a", "3a9e4c1", "settings-b")),
            None
        );
        let other = request("/listings/lookup", &verdict.key, false);
        assert!(handle(&mut conn, &other, now).is_none());
    }
}
