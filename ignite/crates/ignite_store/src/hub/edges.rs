//! Dependency edges: `src` depends on `dst`. Edges are plain rows; the batch route changes them
//! through [`add`] and [`remove`], which leave the version and feed bookkeeping to the batch.

use rusqlite::{Connection, OptionalExtension, Row, params};

use ignite_core::protocol::tickets::{Edge, EdgeKind};

use crate::{Result, StoreError};

/// Edges in both directions of one ticket: its own, then those pointing at it, each sorted by
/// `src` and then `dst`.
pub fn of(conn: &Connection, id: &str) -> Result<Vec<Edge>> {
    collect(
        conn,
        "SELECT src, dst, kind, external FROM edges WHERE src = ?1 \
         UNION ALL SELECT src, dst, kind, external FROM edges WHERE dst = ?1 AND src != ?1 \
         ORDER BY src, dst",
        &[id],
    )
}

/// The outgoing edges of one ticket, sorted by `dst`.
pub fn from(conn: &Connection, id: &str) -> Result<Vec<Edge>> {
    collect(
        conn,
        "SELECT src, dst, kind, external FROM edges WHERE src = ?1 ORDER BY dst",
        &[id],
    )
}

/// Every edge whose two ends are live tickets, or every edge at all with `all`, sorted by `src`
/// and then `dst`.
pub fn graph(conn: &Connection, all: bool) -> Result<Vec<Edge>> {
    let sql = if all {
        "SELECT src, dst, kind, external FROM edges ORDER BY src, dst"
    } else {
        "SELECT edges.src, edges.dst, edges.kind, edges.external FROM edges \
         JOIN tickets s ON s.id = edges.src AND s.status != 'closed' \
         JOIN tickets d ON d.id = edges.dst AND d.status != 'closed' \
         ORDER BY edges.src, edges.dst"
    };
    collect(conn, sql, &[])
}

/// The kind of the edge from `src` to `dst`, if there is one.
pub fn kind(conn: &Connection, src: &str, dst: &str) -> Result<Option<EdgeKind>> {
    let text: Option<String> = conn
        .query_row(
            "SELECT kind FROM edges WHERE src = ?1 AND dst = ?2",
            [src, dst],
            |row| row.get(0),
        )
        .optional()?;
    text.map(|text| parse_kind(&text)).transpose()
}

/// The kind of the edge from `src` to `dst` and whether the user approved it as external.
pub fn get(conn: &Connection, src: &str, dst: &str) -> Result<Option<(EdgeKind, bool)>> {
    let row: Option<(String, bool)> = conn
        .query_row(
            "SELECT kind, external FROM edges WHERE src = ?1 AND dst = ?2",
            [src, dst],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    row.map(|(text, external)| Ok((parse_kind(&text)?, external)))
        .transpose()
}

/// Adds `src` depends on `dst`, or changes the kind of an edge that exists, as an edge the
/// epic rule checks.
pub fn add(conn: &Connection, src: &str, dst: &str, kind: EdgeKind) -> Result<()> {
    add_external(conn, src, dst, kind, false)
}

/// Adds `src` depends on `dst`, or changes the kind and approval of an edge that exists.
/// `external` records that the user approved `dst` lying outside `src`'s outermost epic.
pub fn add_external(
    conn: &Connection,
    src: &str,
    dst: &str,
    kind: EdgeKind,
    external: bool,
) -> Result<()> {
    conn.execute(
        "INSERT INTO edges (src, dst, kind, external) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(src, dst) DO UPDATE SET kind = excluded.kind, external = excluded.external",
        params![src, dst, kind_name(kind), external],
    )?;
    Ok(())
}

pub fn remove(conn: &Connection, src: &str, dst: &str) -> Result<()> {
    conn.execute("DELETE FROM edges WHERE src = ?1 AND dst = ?2", [src, dst])?;
    Ok(())
}

/// The stored name of an edge kind, such as `waits-for`.
pub fn kind_name(kind: EdgeKind) -> &'static str {
    match kind {
        EdgeKind::Blocks => "blocks",
        EdgeKind::ConditionalBlocks => "conditional-blocks",
        EdgeKind::WaitsFor => "waits-for",
        EdgeKind::Related => "related",
        EdgeKind::DiscoveredFrom => "discovered-from",
    }
}

fn parse_kind(text: &str) -> Result<EdgeKind> {
    serde_json::from_value(serde_json::Value::String(text.to_owned()))
        .map_err(|_| StoreError::Invalid(format!("unknown edge kind {text:?}")))
}

/// One stored edge: `(src, dst, kind, external)`.
pub(crate) type Stored = (String, String, String, bool);

/// Reads one `(src, dst, kind, external)` row.
pub(crate) fn read(row: &Row<'_>) -> rusqlite::Result<Stored> {
    Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?))
}

/// Turns stored text into an edge.
pub(crate) fn decode((src, dst, kind, external): Stored) -> Result<Edge> {
    let invalid = |error: ignite_core::ids::IdError| StoreError::Invalid(format!("edge: {error}"));
    Ok(Edge {
        src: src.parse().map_err(invalid)?,
        dst: dst.parse().map_err(invalid)?,
        kind: parse_kind(&kind)?,
        external,
    })
}

pub(crate) fn collect(conn: &Connection, sql: &str, args: &[&str]) -> Result<Vec<Edge>> {
    let mut statement = conn.prepare(sql)?;
    let rows = statement.query_map(rusqlite::params_from_iter(args), read)?;
    rows.map(|row| decode(row?)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;

    fn insert(conn: &Connection, id: &str, status: &str) {
        conn.execute(
            "INSERT INTO tickets (id, title, status, created_at, created_by, updated_at, version) \
             VALUES (?1, ?1, ?2, '2026-11-01T00:00:00.000Z', 't', '2026-11-01T00:00:00.000Z', 0)",
            [id, status],
        )
        .unwrap();
    }

    #[test]
    fn edges_are_added_rekinded_listed_and_removed() {
        let conn = open_in_memory().unwrap();
        for (id, status) in [("a", "open"), ("b", "open"), ("c", "closed")] {
            insert(&conn, id, status);
        }
        add(&conn, "a", "b", EdgeKind::Blocks).unwrap();
        add(&conn, "a", "b", EdgeKind::WaitsFor).unwrap();
        add(&conn, "c", "a", EdgeKind::Related).unwrap();
        assert_eq!(kind(&conn, "a", "b").unwrap(), Some(EdgeKind::WaitsFor));
        let pairs = |edges: Vec<Edge>| -> Vec<(String, String)> {
            edges
                .into_iter()
                .map(|e| (e.src.to_string(), e.dst.to_string()))
                .collect()
        };
        let pair = |a: &str, b: &str| (a.to_owned(), b.to_owned());
        assert_eq!(
            pairs(of(&conn, "a").unwrap()),
            [pair("a", "b"), pair("c", "a")]
        );
        assert_eq!(pairs(graph(&conn, false).unwrap()), [pair("a", "b")]);
        assert_eq!(graph(&conn, true).unwrap().len(), 2);
        remove(&conn, "a", "b").unwrap();
        assert_eq!(kind(&conn, "a", "b").unwrap(), None);
    }
}
