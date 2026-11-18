//! The listing routes: `/listings/lookup` and `/listings/store`. A check family's listing is
//! stored under the family's name and its listing hash by the machine that made it, and any
//! machine whose tree has the same listing hash reuses it instead of building and listing the
//! family itself (see "Checks" in `docs/ignite.md`). The hub keeps each listing as the text the
//! machine sent and never reads it.
//!
//! `/listings/lookup` only reads; `/listings/store` needs an authorized writer (HTTP 401
//! otherwise).

use rusqlite::Connection;
use serde::de::DeserializeOwned;
use serde_json::json;

use ignite_core::protocol::hub::{ListingLookup, ListingReply, StoredListing};
use ignite_core::time::Timestamp;
use ignite_store::hub::listings;

use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    let stores = match request.path.as_str() {
        "/listings/lookup" => false,
        "/listings/store" => true,
        _ => return None,
    };
    if request.method != "POST" {
        return Some(failure(405, &format!("use POST for {}", request.path)));
    }
    let writer = request.writer.as_ref();
    if stores && writer.is_none() {
        return Some(failure(401, "writes need a registered machine's token"));
    }
    let answer = if stores {
        body::<StoredListing>(request).and_then(|stored| {
            let machine = writer.map_or("", |w| w.machine.as_str());
            listings::put(
                conn,
                &stored.key.family,
                stored.key.hash.as_str(),
                &stored.listing,
                machine,
                &now.to_string(),
            )
            .map(|()| Response::json(200, &json!({ "ok": true })))
            .map_err(|e| failure(500, &e.to_string()))
        })
    } else {
        body::<ListingLookup>(request).and_then(|lookup| {
            let found: Result<Vec<Option<String>>, _> = lookup
                .keys
                .iter()
                .map(|key| listings::get(conn, &key.family, key.hash.as_str()))
                .collect();
            found
                .map(|listings| Response::json(200, &json!(ListingReply { listings })))
                .map_err(|e| failure(500, &e.to_string()))
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
    use ignite_core::ids::{InputHash, MachineId};
    use ignite_core::protocol::hub::ListingKey;
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

    fn key(family: &str) -> ListingKey {
        ListingKey {
            family: family.to_owned(),
            hash: InputHash::new("a".repeat(64)).unwrap(),
        }
    }

    #[test]
    fn a_stored_listing_is_found_by_family_and_hash() {
        let mut conn = hub::open_in_memory().unwrap();
        let now = Timestamp::EPOCH;
        let stored = StoredListing {
            key: key("unit"),
            listing: "{\"checks\": []}".to_owned(),
        };
        let refused = handle(&mut conn, &request("/listings/store", &stored, false), now);
        assert_eq!(refused.unwrap().status, 401, "a store needs a writer");
        let done = handle(&mut conn, &request("/listings/store", &stored, true), now);
        assert_eq!(done.unwrap().status, 200);
        let lookup = ListingLookup {
            keys: vec![key("unit"), key("other")],
        };
        let found = handle(&mut conn, &request("/listings/lookup", &lookup, false), now).unwrap();
        assert_eq!(found.status, 200);
        let reply: ListingReply = serde_json::from_slice(&found.body).unwrap();
        assert_eq!(reply.listings, [Some(stored.listing), None]);
        assert!(handle(&mut conn, &request("/results/lookup", &lookup, false), now).is_none());
    }
}
