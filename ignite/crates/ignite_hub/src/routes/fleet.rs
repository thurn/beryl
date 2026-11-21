//! The fleet settings, recent changes, registered machines and provider usage.

use rusqlite::{Connection, TransactionBehavior};
use serde_json::json;

use ignite_core::config::{Config, Os, PoolMode};
use ignite_core::fleet::{
    FleetMachine, FleetSettings, FleetUpdate, FleetView, MachineSetting, MachineState,
    ProviderSetting,
};
use ignite_core::time::Timestamp;
use ignite_store::hub::fleet_settings::{self, PutOutcome};
use ignite_store::hub::{fleet, provider_usage};
use ignite_store::{Result, StoreError};

use super::{Request, Response};

pub fn handle(conn: &mut Connection, request: &Request, now: Timestamp) -> Option<Response> {
    if request.path != "/fleet" {
        return None;
    }
    Some(match request.method.as_str() {
        "GET" => view(conn, now).map_or_else(
            |error| failure(500, &error.to_string()),
            |view| Response::json(200, &json!(view)),
        ),
        "PUT" => update(conn, request, now),
        _ => failure(405, "use GET or PUT for /fleet"),
    })
}

fn view(conn: &mut Connection, now: Timestamp) -> Result<FleetView> {
    // A read-only snapshot keeps the settings, audit log and holds consistent while
    // allowing operators to write from other connections.
    let snapshot = conn.transaction_with_behavior(TransactionBehavior::Deferred)?;
    let settings = fleet_settings::settings(&snapshot)?;
    let changes = fleet_settings::changes(&snapshot, 20)?;
    let machines = fleet::machines(&snapshot)?
        .into_iter()
        .map(|machine| {
            Ok(FleetMachine {
                id: machine.id.parse().map_err(invalid)?,
                name: machine.name,
                os: match machine.os.as_str() {
                    "macos" => Os::Macos,
                    "windows" => Os::Windows,
                    other => return Err(invalid(format!("unknown machine OS {other}"))),
                },
                last_seen: machine.last_seen.parse().map_err(invalid)?,
                // The hub has no stored capacity reports yet.
                capacity: None,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let usage = provider_usage::latest(&snapshot)?;
    let holds = provider_usage::held(&snapshot, &settings, now)?;
    snapshot.commit()?;
    Ok(FleetView {
        settings,
        changes,
        machines,
        usage,
        holds,
    })
}

fn update(conn: &mut Connection, request: &Request, now: Timestamp) -> Response {
    let Some(writer) = &request.writer else {
        return failure(
            401,
            "fleet writes need a machine token or a dashboard login",
        );
    };
    let update: FleetUpdate = match serde_json::from_slice(&request.body) {
        Ok(update) => update,
        Err(error) => return failure(400, &error.to_string()),
    };
    match fleet_settings::put(conn, update.version, update.settings, &writer.actor, now) {
        Ok(PutOutcome::Stored(settings)) => Response::json(200, &json!(settings)),
        Ok(PutOutcome::Conflict(settings)) => Response::json(409, &json!(settings)),
        Err(StoreError::Rejected(message)) => failure(400, &message),
        Err(error) => failure(500, &error.to_string()),
    }
}

/// Imports the old pool configuration once, as the first audited settings change.
pub(crate) fn seed(conn: &mut Connection, config: &Config, now: Timestamp) -> Result<()> {
    if fleet_settings::settings(conn)?.version != 0 {
        return Ok(());
    }
    // Another hub may have seeded the database since the read. Its settings win.
    fleet_settings::put(conn, 0, initial_settings(config), "ignite.toml", now)?;
    Ok(())
}

fn initial_settings(config: &Config) -> FleetSettings {
    let mut settings = FleetSettings::default();
    let stop_percent = config.pool.usage_hold_percent;
    for name in config.pool.cli.keys() {
        settings.providers.insert(
            name.clone(),
            ProviderSetting {
                stop_percent,
                ..ProviderSetting::default()
            },
        );
    }
    for (name, machine) in &config.machine {
        let mut state = MachineState::ChecksOnly;
        for (provider, pool) in &machine.pool {
            if pool.mode == PoolMode::Cli {
                state = MachineState::Active;
                let provider = settings
                    .providers
                    .entry(provider.clone())
                    .or_insert_with(|| ProviderSetting {
                        stop_percent,
                        ..ProviderSetting::default()
                    });
                provider.share = provider.share.saturating_add(pool.slots);
            }
        }
        settings.machines.insert(
            name.clone(),
            MachineSetting {
                state,
                disk_cap_gb: machine.trees_disk_gb,
            },
        );
    }
    settings
}

fn invalid(error: impl std::fmt::Display) -> StoreError {
    StoreError::Invalid(error.to_string())
}

fn failure(status: u16, message: &str) -> Response {
    Response::json(status, &json!({"error": message}))
}
