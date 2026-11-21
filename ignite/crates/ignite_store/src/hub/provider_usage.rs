//! The fleet's newest usage sample per provider and plan window.

use std::collections::BTreeMap;

use rusqlite::{Connection, params};

use ignite_core::fleet::{FleetSettings, ProviderSample};
use ignite_core::ids::MachineId;
use ignite_core::plan_usage::{Hold, hold};
use ignite_core::time::Timestamp;

use super::rows;
use crate::{Result, StoreError};

/// Keeps the sample if this window has no newer reading from any machine.
pub fn record(conn: &Connection, sample: &ProviderSample) -> Result<()> {
    conn.execute(
        "INSERT INTO provider_usage (provider, at, machine, status, label, percent_used, resets_at, note) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8) \
         ON CONFLICT(provider, (label IS NULL), coalesce(label, '')) DO UPDATE SET \
           at = excluded.at, machine = excluded.machine, status = excluded.status, \
           percent_used = excluded.percent_used, resets_at = excluded.resets_at, note = excluded.note \
         WHERE excluded.at > provider_usage.at",
        params![
            sample.provider, sample.at.to_string(), sample.machine.as_str(), sample.status,
            sample.label, sample.percent_used, sample.resets_at, sample.note,
        ],
    )?;
    Ok(())
}

pub fn latest(conn: &Connection) -> Result<Vec<ProviderSample>> {
    let mut statement = conn.prepare(
        "SELECT provider, at, machine, status, label, percent_used, resets_at, note \
         FROM provider_usage ORDER BY provider, label NULLS LAST",
    )?;
    statement
        .query_map([], |row| {
            Ok(rows::ProviderUsage {
                provider: row.get(0)?,
                at: row.get(1)?,
                machine: row.get(2)?,
                status: row.get(3)?,
                label: row.get(4)?,
                percent_used: row.get(5)?,
                resets_at: row.get(6)?,
                note: row.get(7)?,
            })
        })?
        .map(|row| {
            let row = row?;
            Ok(ProviderSample {
                provider: row.provider,
                at: Timestamp::parse(&row.at)
                    .map_err(|error| StoreError::Invalid(error.to_string()))?,
                machine: MachineId::new(row.machine)
                    .map_err(|error| StoreError::Invalid(error.to_string()))?,
                status: row.status,
                label: row.label,
                percent_used: row.percent_used,
                resets_at: row.resets_at,
                note: row.note,
            })
        })
        .collect()
}

/// The windows that still hold hand-offs, grouped by provider.
pub fn held(
    conn: &Connection,
    settings: &FleetSettings,
    now: Timestamp,
) -> Result<BTreeMap<String, Vec<Hold>>> {
    let mut holds = BTreeMap::<String, Vec<Hold>>::new();
    for sample in latest(conn)? {
        if let Some(label) = sample.label.as_deref()
            && let Some(held) = hold(
                label,
                sample.percent_used,
                sample.resets_at.as_deref(),
                settings.provider(&sample.provider).stop_percent,
                now,
            )
        {
            holds.entry(sample.provider).or_default().push(held);
        }
    }
    Ok(holds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hub::open_in_memory;
    use ignite_core::fleet::ProviderSetting;

    #[test]
    fn holds_use_the_newest_fleet_sample_for_each_provider_and_window() {
        let conn = open_in_memory().unwrap();
        let first = Timestamp::EPOCH;
        let then = first.plus_seconds(1.0);
        let reset = then.plus_seconds(1.0);
        let mut settings = FleetSettings::default();
        settings.providers.insert(
            "claude".to_owned(),
            ProviderSetting {
                stop_percent: 90,
                ..ProviderSetting::default()
            },
        );

        let claude = ProviderSample {
            provider: "claude".to_owned(),
            at: first,
            machine: "m-1".parse().unwrap(),
            status: "ok".to_owned(),
            label: Some("Weekly · Sonnet".to_owned()),
            percent_used: Some(90.0),
            resets_at: Some(reset.to_string()),
            note: None,
        };
        record(&conn, &claude).unwrap();
        let codex = ProviderSample {
            provider: "codex".to_owned(),
            ..claude.clone()
        };
        record(&conn, &codex).unwrap();
        let no_windows = ProviderSample {
            at: then,
            label: None,
            percent_used: None,
            resets_at: None,
            ..claude.clone()
        };
        record(&conn, &no_windows).unwrap();
        record(&conn, &no_windows).unwrap();
        assert_eq!(
            latest(&conn).unwrap(),
            [claude.clone(), no_windows.clone(), codex.clone()]
        );
        let holds = held(&conn, &settings, then).unwrap();
        assert!(holds["claude"][0].holds_model("claude-sonnet"));
        assert!(!holds["claude"][0].holds_model("claude-opus"));
        assert!(!holds.contains_key("codex"));
        assert!(held(&conn, &settings, reset).unwrap().is_empty());

        let lower = ProviderSample {
            at: then,
            machine: "m-2".parse().unwrap(),
            percent_used: Some(89.0),
            ..claude.clone()
        };
        record(&conn, &lower).unwrap();
        record(&conn, &claude).unwrap();
        assert_eq!(latest(&conn).unwrap(), [lower, no_windows, codex]);
        assert!(held(&conn, &settings, then).unwrap().is_empty());
    }
}
