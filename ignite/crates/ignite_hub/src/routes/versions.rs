//! `GET /machines/versions`: the Ignite commit the hub runs and the one each registered machine
//! reported in its last heartbeat, with when that was. `ignite status` shows them next to the
//! release branch, and `ignite wait deploy-<commit>` waits until they reach a release.
//!
//! It only reads, like the dashboard's machines page, so it needs no writer's token.

use rusqlite::{Connection, OptionalExtension};
use serde_json::json;

use ignite_core::ids::MachineId;
use ignite_core::protocol::fleet::{FleetVersions, MachineVersion};
use ignite_core::time::Timestamp;

use super::dispatch::HUB_VERSION;
use super::{Request, Response};

/// The response if this module owns the request's path.
pub fn handle(conn: &mut Connection, request: &Request, _now: Timestamp) -> Option<Response> {
    if request.path != "/machines/versions" {
        return None;
    }
    if request.method != "GET" {
        return Some(failure(405, "use GET for /machines/versions"));
    }
    Some(match versions(conn) {
        Ok(versions) => Response::json(200, &json!(versions)),
        Err(error) => failure(500, &error.to_string()),
    })
}

fn versions(conn: &Connection) -> rusqlite::Result<FleetVersions> {
    let hub_version: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [HUB_VERSION],
            |row| row.get(0),
        )
        .optional()?;
    let mut statement =
        conn.prepare("SELECT id, name, version, last_seen FROM machines ORDER BY name, id")?;
    let rows = statement.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
        ))
    })?;
    let mut machines = Vec::new();
    for row in rows {
        let (id, name, version, last_seen) = row?;
        // A row the hub cannot read back is left out rather than failing the whole answer.
        let (Ok(machine), Ok(last_seen)) = (MachineId::new(id), Timestamp::parse(&last_seen))
        else {
            continue;
        };
        machines.push(MachineVersion {
            machine,
            name,
            version,
            last_seen,
        });
    }
    Ok(FleetVersions {
        hub_version: hub_version.unwrap_or_default(),
        machines,
    })
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({ "error": message }))
}

#[cfg(test)]
mod tests {
    use ignite_core::config::Os;
    use ignite_core::protocol::hub::{Heartbeat, MachineRegister};
    use ignite_store::hub;

    use super::*;
    use crate::routes::dispatch::set_hub_version;

    fn get(conn: &mut Connection, method: &str) -> Response {
        let request = Request {
            method: method.to_owned(),
            path: "/machines/versions".to_owned(),
            query: Vec::new(),
            writer: None,
            body: Vec::new(),
            headers: Vec::new(),
        };
        handle(conn, &request, Timestamp::EPOCH).unwrap()
    }

    fn register(conn: &mut Connection, id: &str, name: &str, at: f64) {
        let machine = MachineId::new(id).unwrap();
        ignite_store::hub::fleet::register(
            conn,
            &MachineRegister {
                machine: machine.clone(),
                name: name.to_owned(),
                os: Os::Macos,
                cores: 8,
                memory_mb: 1024,
                families: Vec::new(),
                version: "aaaaaaa".to_owned(),
            },
            Timestamp::EPOCH,
        )
        .unwrap();
        ignite_store::hub::fleet::heartbeat(
            conn,
            &Heartbeat {
                machine,
                version: "bbbbbbb".to_owned(),
                workers: Vec::new(),
            },
            Timestamp::EPOCH.plus_seconds(at),
        )
        .unwrap();
    }

    /// The hub and each machine, by name, with the version of its last heartbeat.
    #[test]
    fn answers_the_hub_and_each_machine() {
        let mut conn = hub::open_in_memory().unwrap();
        set_hub_version(&conn, "ccccccc").unwrap();
        register(&mut conn, "m-2", "multivac", 20.0);
        register(&mut conn, "m-1", "jarvis", 10.0);
        let reply = get(&mut conn, "GET");
        assert_eq!(reply.status, 200);
        let versions: FleetVersions = serde_json::from_slice(&reply.body).unwrap();
        assert_eq!(versions.hub_version, "ccccccc");
        let names: Vec<_> = versions.machines.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["jarvis", "multivac"]);
        assert_eq!(versions.machines[0].version, "bbbbbbb");
        assert_eq!(
            versions.machines[1].last_seen,
            Timestamp::EPOCH.plus_seconds(20.0)
        );
        assert_eq!(get(&mut conn, "POST").status, 405);
    }
}
