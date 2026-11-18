//! Ignite's SQLite databases.
//!
//! - [`hub`]: the hub's shared database: tickets, the result cache, check records, tips,
//!   records and the fleet.
//! - [`local`]: each daemon's own database: its trees, grants, handles, unsent records and its
//!   copy of the result cache.
//! - [`import`]: reading the old systems' data into the hub's database at the switch.
//!
//! Each database's schema is SQL applied by [`migrate`], which records the schema version in the
//! `meta` table. Each table has a row struct with the table's columns as fields.

pub mod hub;
pub mod import;
pub mod local;
pub mod migrate;

use std::fmt;

/// Why a store operation failed.
#[derive(Debug)]
pub enum StoreError {
    Sqlite(rusqlite::Error),
    /// The database holds something the code does not accept, such as a newer schema.
    Invalid(String),
    /// A request named a ticket or other row that does not exist (a 404 at the hub).
    NotFound(String),
    /// A request the store refuses as malformed, such as a ticket without a title or a parent
    /// that would make a cycle (a 400 at the hub).
    Rejected(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Sqlite(error) => write!(f, "sqlite: {error}"),
            StoreError::Invalid(message)
            | StoreError::NotFound(message)
            | StoreError::Rejected(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        StoreError::Sqlite(error)
    }
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// Defines one row struct per table, each with its table's name in `TABLE` and its column names
/// in `COLUMNS`, plus `TABLES` listing them all, so a test can compare the structs with the
/// schema. A column whose name is a Rust keyword uses a raw identifier (`r#type`), and its entry
/// in `COLUMNS` keeps the `r#` prefix. A `const fn` cannot declare structs, so this is a macro.
macro_rules! tables {
    ($(
        $(#[$doc:meta])*
        $name:ident = $table:literal {
            $( $(#[$field_doc:meta])* $field:ident : $type:ty ),* $(,)?
        }
    )*) => {
        $(
            $(#[$doc])*
            #[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
            pub struct $name {
                $( $(#[$field_doc])* pub $field: $type ),*
            }

            impl $name {
                pub const TABLE: &'static str = $table;
                pub const COLUMNS: &'static [&'static str] = &[$(stringify!($field)),*];
            }
        )*

        /// Every table with its columns, in declaration order.
        pub const TABLES: &[(&str, &[&str])] = &[$(($table, $name::COLUMNS)),*];
    };
}

pub(crate) use tables;

#[cfg(test)]
pub(crate) mod test_support {
    use rusqlite::Connection;

    /// Asserts that every table in `tables` exists with exactly the listed columns, in order,
    /// and that the database holds no other table but `meta` and SQLite's own `sqlite_sequence`,
    /// which `AUTOINCREMENT` creates.
    pub fn assert_rows_match_schema(conn: &Connection, tables: &[(&str, &[&str])]) {
        let mut names: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' \
             AND name NOT IN ('meta', 'sqlite_sequence')",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .map(Result::unwrap)
            .collect();
        names.sort();
        let mut declared: Vec<String> = tables.iter().map(|(t, _)| (*t).to_owned()).collect();
        declared.sort();
        assert_eq!(
            names, declared,
            "tables in the schema and row structs differ"
        );
        for (table, columns) in tables {
            let actual: Vec<String> = conn
                .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
                .unwrap()
                .query_map([], |row| row.get(0))
                .unwrap()
                .map(Result::unwrap)
                .collect();
            let expected: Vec<&str> = columns.iter().map(|c| c.trim_start_matches("r#")).collect();
            assert_eq!(actual, expected, "columns of {table}");
        }
    }
}
