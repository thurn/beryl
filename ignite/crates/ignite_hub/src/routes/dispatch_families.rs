//! Reviewed check declarations used by the hub's dispatch snapshot.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::dispatch::RequiredFamily;
use ignite_core::ids::CheckName;
use ignite_core::inputs::path_matches;
use ignite_core::manifest::{AnyPlatform, InputPattern, Kind, Manifest, Platforms};
use ignite_store::{Result, StoreError};

use super::{queue::held, resolve_named_file};

const KEY: &str = "dispatch_check_families";

#[derive(Deserialize, Serialize)]
struct Declaration {
    platforms: Platforms,
    paths: BTreeSet<String>,
}

/// Publishes the path and platform declarations from a reviewed manifest as one snapshot.
/// Every queue reader then sees the same declarations within its database transaction.
pub fn publish(conn: &Connection, manifest: &Manifest) -> Result<()> {
    let declarations: BTreeMap<_, _> = manifest
        .families
        .iter()
        .filter(|(_, declared)| declared.family.kind == Kind::Regression)
        .map(|(name, declared)| {
            let family = &declared.family;
            let paths = family
                .inputs
                .iter()
                .chain(family.package_inputs.values().flatten())
                .chain(&family.digest_inputs)
                .filter_map(|pattern| match pattern {
                    InputPattern::Path(path) => Some(path.clone()),
                    InputPattern::Crate(_) | InputPattern::CrateClosure => None,
                })
                .collect();
            (
                name,
                Declaration {
                    platforms: family.platforms.clone(),
                    paths,
                },
            )
        })
        .collect();
    let json = serde_json::to_string(&declarations)
        .map_err(|error| StoreError::Invalid(error.to_string()))?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value WHERE value != excluded.value",
        params![KEY, json],
    )?;
    Ok(())
}

pub(super) struct Requirements {
    declarations: BTreeMap<String, Declaration>,
    regressions: HashMap<String, Vec<CheckName>>,
}

impl Requirements {
    pub(super) fn load(conn: &Connection) -> Result<Self> {
        let raw: Option<String> = conn
            .query_row("SELECT value FROM meta WHERE key = ?1", [KEY], |row| {
                row.get(0)
            })
            .optional()?;
        let declarations = raw
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|error| StoreError::Invalid(error.to_string()))?
            .unwrap_or_default();
        let mut statement =
            conn.prepare("SELECT ticket, check_name FROM regressions WHERE state IN (?1, ?2)")?;
        let rows = statement.query_map(held::FAILING, |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut regressions: HashMap<String, Vec<CheckName>> = HashMap::new();
        for row in rows {
            let (ticket, check) = row?;
            let checks = regressions.entry(ticket).or_default();
            // Old records may predate qualified check names. They still mark a regression.
            checks.extend(check.parse::<CheckName>().ok());
        }
        Ok(Self {
            declarations,
            regressions,
        })
    }

    pub(super) fn is_regression(&self, ticket: &str) -> bool {
        self.regressions.contains_key(ticket)
    }

    pub(super) fn for_ticket(
        &self,
        ticket: &str,
        os: Option<Os>,
        files: &[String],
        known: &[String],
    ) -> Vec<RequiredFamily> {
        if let Some(checks) = self.regressions.get(ticket) {
            return checks
                .iter()
                .map(CheckName::family)
                .collect::<BTreeSet<_>>()
                .into_iter()
                .map(|name| RequiredFamily {
                    name: name.to_owned(),
                    platforms: self
                        .declarations
                        .get(name)
                        .map_or(Platforms::Any(AnyPlatform::Any), |declared| {
                            declared.platforms.clone()
                        }),
                })
                .collect();
        }
        if files.is_empty() || self.declarations.is_empty() {
            return Vec::new();
        }
        let files: Vec<_> = files
            .iter()
            .map(|file| {
                resolve_named_file(file, known)
                    .unwrap_or_else(|| file.trim_start_matches("./").to_owned())
            })
            .collect();
        self.declarations
            .iter()
            .filter(|(_, declared)| {
                os.is_none_or(|os| declared.platforms.supports(os))
                    && files.iter().any(|file| {
                        declared
                            .paths
                            .iter()
                            .any(|pattern| path_matches(pattern, file))
                    })
            })
            .map(|(name, declared)| RequiredFamily {
                name: name.clone(),
                platforms: declared.platforms.clone(),
            })
            .collect()
    }
}

#[cfg(test)]
#[path = "dispatch_families_tests.rs"]
mod tests;
