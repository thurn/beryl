//! Reviewed check declarations used by the hub's dispatch snapshot.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};

use ignite_core::config::Os;
use ignite_core::dispatch::RequiredFamily;
use ignite_core::dispatch_pressure::likely_compiles;
use ignite_core::ids::CheckName;
use ignite_core::inputs::path_matches;
use ignite_core::manifest::{AnyPlatform, InputPattern, Kind, Manifest, Platforms, Product};
use ignite_core::protocol::tickets::TicketType;
use ignite_store::{Result, StoreError};

use super::queue::held;

const KEY: &str = "dispatch_check_families";

#[derive(Deserialize, Serialize)]
struct Declaration {
    platforms: Platforms,
    /// Path patterns for ordinary ticket eligibility.
    paths: BTreeSet<String>,
    /// Present for families that list or run with a product that uses a build slot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    build_paths: Option<BTreeSet<String>>,
}

fn paths<'a>(patterns: impl Iterator<Item = &'a InputPattern>) -> BTreeSet<String> {
    patterns
        .filter_map(|pattern| match pattern {
            InputPattern::Path(path) => Some(path.clone()),
            InputPattern::Crate(_) | InputPattern::CrateClosure => None,
        })
        .collect()
}

fn matches_files(patterns: &BTreeSet<String>, files: &[String]) -> bool {
    files
        .iter()
        .any(|file| patterns.iter().any(|pattern| path_matches(pattern, file)))
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
            let inputs = family
                .inputs
                .iter()
                .chain(family.package_inputs.values().flatten())
                .chain(&family.digest_inputs);
            let products = family
                .needs
                .iter()
                .chain(family.list_needs.iter().flatten())
                .filter_map(|name| manifest.products.get(name));
            let build_paths = products.clone().any(Product::uses_build_slot).then(|| {
                paths(
                    inputs
                        .clone()
                        .chain(products.flat_map(|product| &product.inputs)),
                )
            });
            (
                name,
                Declaration {
                    platforms: family.platforms.clone(),
                    paths: family.dispatch_paths.as_ref().map_or_else(
                        || paths(inputs),
                        |patterns| patterns.iter().cloned().collect(),
                    ),
                    build_paths,
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

    /// Estimate from the reviewed landing checks before the ticket has a tree or a build record.
    /// Use declared path patterns because per-check inputs are known only after listing checks.
    ///
    /// A bug always counts as build work. Which files its fix touches is unknown until someone
    /// reproduces it, so the files its text names are what to read (a report, a guide), not
    /// what it will change. Judged by those files, a burst of thirteen bug reports passed as
    /// light work and started on one machine within 80 seconds, past the build-ticket limit
    /// and the pacing between build handoffs.
    pub(super) fn needs_build_slot(
        &self,
        ticket: &str,
        ticket_type: TicketType,
        os: Option<Os>,
        cores: f64,
        files: &[String],
    ) -> bool {
        ticket_type == TicketType::Bug
            || likely_compiles(cores, files)
            || self.regressions.get(ticket).is_some_and(|checks| {
                checks.iter().any(|check| {
                    self.declarations
                        .get(check.family())
                        .is_some_and(|family| family.build_paths.is_some())
                })
            })
            || self.declarations.values().any(|family| {
                os.is_none_or(|os| family.platforms.supports(os))
                    && family
                        .build_paths
                        .as_ref()
                        .is_some_and(|paths| matches_files(paths, files))
            })
    }

    pub(super) fn for_ticket(
        &self,
        ticket: &str,
        os: Option<Os>,
        files: &[String],
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
        self.declarations
            .iter()
            .filter(|(_, declared)| {
                os.is_none_or(|os| declared.platforms.supports(os))
                    && matches_files(&declared.paths, files)
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
