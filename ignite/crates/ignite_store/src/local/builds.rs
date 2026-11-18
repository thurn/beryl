//! What the build manager knows about each build product in each tree: the input hash a tree
//! holds a product built at, or the input hash its last build failed at. A restarted daemon reads
//! these rows, so it answers a request for a product the tree already holds without running
//! Cargo again.

use rusqlite::{Connection, params};

use crate::Result;
use crate::local::rows;

/// The `outcome` of a product the tree holds.
pub const HELD: &str = "held";
/// The `outcome` of a product whose last build exited with a failing code.
pub const FAILED: &str = "failed";

/// Every recorded row.
pub fn all(conn: &Connection) -> Result<Vec<rows::Build>> {
    let mut statement = conn.prepare(
        "SELECT tree, product, packages, hash, outcome, output, updated_at \
         FROM builds ORDER BY tree, product, packages",
    )?;
    let found = statement
        .query_map([], |row| {
            Ok(rows::Build {
                tree: row.get(0)?,
                product: row.get(1)?,
                packages: row.get(2)?,
                hash: row.get(3)?,
                outcome: row.get(4)?,
                output: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(found)
}

/// Records what is known about a product in a tree, replacing any earlier row for the same
/// tree, product and packages.
pub fn set(conn: &Connection, build: &rows::Build) -> Result<()> {
    conn.execute(
        "INSERT OR REPLACE INTO builds \
         (tree, product, packages, hash, outcome, output, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            build.tree,
            build.product,
            build.packages,
            build.hash,
            build.outcome,
            build.output,
            build.updated_at,
        ],
    )?;
    Ok(())
}

/// Forgets that the tree holds a product, keeping a recorded failure.
pub fn drop_held(conn: &Connection, tree: &str, product: &str, packages: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM builds WHERE tree = ?1 AND product = ?2 AND packages = ?3 AND outcome = ?4",
        params![tree, product, packages, HELD],
    )?;
    Ok(())
}

/// Forgets the row for a tree, product and packages.
pub fn remove(conn: &Connection, tree: &str, product: &str, packages: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM builds WHERE tree = ?1 AND product = ?2 AND packages = ?3",
        params![tree, product, packages],
    )?;
    Ok(())
}

/// Forgets everything about a tree, for when its build directories are deleted.
pub fn clear_tree(conn: &Connection, tree: &str) -> Result<()> {
    conn.execute("DELETE FROM builds WHERE tree = ?1", [tree])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::local::open_in_memory;

    fn row(tree: &str, product: &str, outcome: &str) -> rows::Build {
        rows::Build {
            tree: tree.to_owned(),
            product: product.to_owned(),
            packages: "[]".to_owned(),
            hash: "h1".to_owned(),
            outcome: outcome.to_owned(),
            output: None,
            updated_at: "2027-01-01T00:00:00.000Z".to_owned(),
        }
    }

    #[test]
    fn rows_are_replaced_dropped_and_cleared() {
        let conn = open_in_memory().unwrap();
        assert!(all(&conn).unwrap().is_empty());
        set(&conn, &row("tree-01", "tests", HELD)).unwrap();
        let mut failed = row("tree-01", "tests", FAILED);
        failed.output = Some("/records/build.txt".to_owned());
        set(&conn, &failed).unwrap();
        set(&conn, &row("tree-02", "tests", HELD)).unwrap();
        assert_eq!(
            all(&conn).unwrap(),
            vec![failed.clone(), row("tree-02", "tests", HELD)]
        );
        // A failure is not a held product, so it stays.
        drop_held(&conn, "tree-01", "tests", "[]").unwrap();
        drop_held(&conn, "tree-02", "tests", "[]").unwrap();
        assert_eq!(all(&conn).unwrap(), vec![failed]);
        set(&conn, &row("tree-01", "lint", HELD)).unwrap();
        remove(&conn, "tree-01", "lint", "[]").unwrap();
        assert_eq!(all(&conn).unwrap().len(), 1);
        clear_tree(&conn, "tree-01").unwrap();
        assert!(all(&conn).unwrap().is_empty());
    }
}
