//! Provider settings, plan windows and CLI probe reports on the Fleet page.
#![allow(clippy::format_collect)]

use std::collections::{BTreeMap, BTreeSet};

use rusqlite::Connection;

use ignite_core::capacity::ProviderCapacity;
use ignite_core::fleet::{FleetMachine, FleetView, ProviderSample, ProviderSetting};
use ignite_core::plan_usage::{Hold, parse_resets_at};
use ignite_store::hub::rows::Machine;

use super::components::{self as c, esc};
use super::data::ReadError;

pub(super) fn section(
    conn: &Connection,
    view: &FleetView,
    machines: &[Machine],
    editable: bool,
) -> Result<String, ReadError> {
    let mut names = view
        .settings
        .providers
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    for machine in machines {
        let apps: Vec<String> = serde_json::from_str(&machine.cli_apps)
            .map_err(|error| ReadError(error.to_string()))?;
        names.extend(apps);
    }
    let providers: BTreeMap<_, _> = names
        .iter()
        .map(|name| (name.as_str(), view.settings.provider(name)))
        .collect();
    let total_share: f64 = providers
        .values()
        .map(|setting| enabled_share(*setting))
        .sum();
    let counts = worker_counts(conn)?;
    let total_workers: f64 = counts.values().copied().map(f64::from).sum();
    let body = if providers.is_empty() {
        c::empty("No providers configured or reported.")
    } else {
        c::rows(providers.iter().map(|(&name, &setting)| {
            let fraction = if total_share > 0.0 {
                enabled_share(setting) / total_share
            } else {
                0.0
            };
            let share = if setting.enabled && setting.share == 0 {
                "Off".to_owned()
            } else {
                percent(fraction * 100.0)
            };
            let running = counts.get(name).copied().unwrap_or_default();
            let workers = format!("{running} of {} by share", decimal(total_workers * fraction));
            let controls = settings_fields(name, setting, &share, workers, editable);
            let holds = hold_rows(name, setting, view, &providers);
            let usage = usage_rows(name, setting, &view.usage);
            let probes = probe_rows(name, &view.machines);
            format!(
                r#"<div class="c-fleet-provider" data-fleet-provider="{name}"><h3 class="c-subsection-title">{name}</h3>{controls}{holds}{usage}{probes}</div>"#,
                name = esc(name)
            )
        }))
    };
    Ok(c::section("Providers", &body, None, Some("providers"), 2))
}

fn enabled_share(setting: ProviderSetting) -> f64 {
    if setting.enabled {
        f64::from(setting.share)
    } else {
        0.0
    }
}

fn decimal(value: f64) -> String {
    let rounded = format!("{value:.1}");
    rounded.strip_suffix(".0").unwrap_or(&rounded).to_owned()
}

fn percent(value: f64) -> String {
    if value > 0.0 && value < 0.05 {
        "&lt;0.1%".to_owned()
    } else {
        format!("{}%", decimal(value))
    }
}

fn worker_counts(conn: &Connection) -> Result<BTreeMap<String, u32>, ReadError> {
    // Older claims have no recorded provider. Use the worker still reporting that ticket.
    let mut query = conn.prepare(
        "SELECT COALESCE(t.provider, w.app) AS provider, COUNT(*)
         FROM tickets t LEFT JOIN workers w
           ON w.machine = t.machine AND w.session = t.session AND w.ticket = t.id AND w.role = 'worker'
         WHERE t.status = 'in_progress' AND COALESCE(t.provider, w.app) IS NOT NULL
         GROUP BY COALESCE(t.provider, w.app)",
    )?;
    Ok(query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<Result<_, _>>()?)
}

fn settings_fields(
    name: &str,
    setting: ProviderSetting,
    share_label: &str,
    workers: String,
    editable: bool,
) -> String {
    let ProviderSetting {
        enabled,
        share,
        stop_percent,
        five_hour_stop_percent: _,
        ignore_usage_threshold,
    } = setting;
    let five_hour_stop = setting.five_hour_stop();
    let state = if enabled { "On" } else { "Off" };
    let (state, share, five_hour, weekly, usage_policy) = if editable {
        let name = esc(name);
        let checked = if enabled { " checked" } else { "" };
        let (ignore_checked, threshold_readonly) = if ignore_usage_threshold {
            (" checked", " readonly")
        } else {
            ("", "")
        };
        (
            format!(
                r#"<label class="c-fleet-switch"><input type="checkbox" role="switch" autocomplete="off" data-fleet-field="enabled" aria-label="{name} enabled"{checked}>{state}</label>"#
            ),
            format!(
                r#"<div class="c-edit-form"><input type="number" autocomplete="off" data-fleet-field="share" aria-label="{name} share" min="0" max="1000" step="1" required value="{share}"></div>"#
            ),
            format!(
                r#"<div class="c-edit-form"><input type="number" autocomplete="off" data-fleet-field="five_hour_stop_percent" aria-label="{name} 5-hour stop percentage" min="1" max="100" step="1" required value="{five_hour_stop}"{threshold_readonly}></div>"#
            ),
            format!(
                r#"<div class="c-edit-form"><input type="number" autocomplete="off" data-fleet-field="stop_percent" aria-label="{name} weekly stop percentage" min="1" max="100" step="1" required value="{stop_percent}"{threshold_readonly}></div>"#
            ),
            format!(
                r#"<label class="c-fleet-switch"><input type="checkbox" autocomplete="off" data-fleet-field="ignore_usage_threshold" aria-label="{name} continue past usage threshold"{ignore_checked}>Continue past threshold</label>"#
            ),
        )
    } else {
        (
            state.to_owned(),
            share.to_string(),
            format!("{five_hour_stop}%"),
            format!("{stop_percent}%"),
            if ignore_usage_threshold {
                "Continue past threshold"
            } else {
                "Stop at threshold"
            }
            .to_owned(),
        )
    };
    c::meta(&[
        ("State".to_owned(), state),
        ("Share".to_owned(), share + &c::muted(share_label)),
        ("5-hour stop".to_owned(), five_hour),
        ("Weekly stop".to_owned(), weekly),
        ("Usage policy".to_owned(), usage_policy),
        ("Workers now".to_owned(), workers),
    ])
}

fn hold_rows(
    name: &str,
    setting: ProviderSetting,
    view: &FleetView,
    providers: &BTreeMap<&str, ProviderSetting>,
) -> String {
    let Some(holds) = view.holds.get(name) else {
        return String::new();
    };
    let windows = holds
        .iter()
        .map(|hold| {
            let window = format!("{} at {}", esc(&hold.label), percent(hold.percent_used));
            let message = match &hold.resets_at {
                Some(at) => format!("Held until {}: {window}", reset_time(at)),
                None => format!("Held: {window} (reset unknown)"),
            };
            format!(r#"<p><span class="c-dot c-tone-amber"><span>{message}</span></span></p>"#)
        })
        .collect::<String>();
    let failover = if enabled_share(setting) > 0.0 {
        let whole_provider = holds.iter().any(|hold| hold.family.is_none());
        let available = providers
            .iter()
            .filter(|&(&other, setting)| {
                other != name
                    && setting.takes_work()
                    && can_take_work(view.holds.get(other).map(Vec::as_slice), holds)
            })
            .map(|(name, _)| esc(name))
            .collect::<Vec<_>>();
        let message = if available.is_empty() {
            "No other provider can take the held work.".to_owned()
        } else {
            let work = if whole_provider {
                "Share"
            } else {
                "Affected work"
            };
            format!("{work} can go to {}.", available.join(", "))
        };
        format!("<p>{message}</p>")
    } else {
        String::new()
    };
    format!(r#"<div class="c-fleet-holds">{windows}{failover}</div>"#)
}

fn can_take_work(other: Option<&[Hold]>, held: &[Hold]) -> bool {
    other.is_none_or(|other| {
        !other.iter().any(|hold| hold.family.is_none())
            && held.iter().any(|window| {
                window.family.is_none() || !other.iter().any(|hold| hold.family == window.family)
            })
    })
}

fn reset_time(at: &str) -> String {
    let at = parse_resets_at(at).map_or_else(|| at.to_owned(), |stamp| stamp.to_string());
    c::time(Some(&at), "Unknown")
}

fn usage_rows(name: &str, setting: ProviderSetting, samples: &[ProviderSample]) -> String {
    let windows: Vec<_> = samples
        .iter()
        .filter(|sample| sample.provider == name)
        .filter_map(|sample| {
            let stop = (!setting.ignore_usage_threshold)
                .then(|| setting.stop_percent_for(sample.label.as_deref().unwrap_or_default()));
            usage_row(sample, stop)
        })
        .collect();
    let body = if windows.is_empty() {
        c::muted("No usage reported")
    } else {
        c::rows(windows)
    };
    c::subsection("Plan usage", &body, None, 4)
}

fn usage_row(sample: &ProviderSample, stop: Option<u32>) -> Option<String> {
    let label = esc(sample.label.as_deref()?);
    let usage = sample.percent_used.filter(|used| used.is_finite()).map_or_else(
        || c::muted("Usage unavailable"),
        |used| {
            let fill = used.clamp(0.0, 100.0);
            let (description, marker) = stop.map_or_else(
                || (String::new(), String::new()),
                |stop| {
                    (
                        format!(", stop at {stop}%"),
                        format!(r#" style="--fleet-stop:{stop}%""#),
                    )
                },
            );
            format!(
                r#"<span class="c-progress" role="meter" aria-label="{label} usage{description}" aria-valuenow="{fill}" aria-valuemin="0" aria-valuemax="100"><span class="c-progress-track c-fleet-meter"{marker}><span class="c-progress-fill" style="width:{fill}%"></span></span>{}</span>"#,
                percent(used),
            )
        },
    );
    let reset = sample.resets_at.as_deref().map_or_else(
        || c::muted("Reset unknown"),
        |at| format!("Resets {}", reset_time(at)),
    );
    Some(c::row(
        &format!("<span>{label}</span>{usage}"),
        &reset,
        None,
    ))
}

fn probe_label(provider: Option<&ProviderCapacity>) -> String {
    match provider {
        Some(provider) if provider.hold_reason.is_some() => {
            esc(provider.hold_reason.as_deref().unwrap_or_default())
        }
        Some(provider) if provider.probed => "CLI answered".to_owned(),
        Some(_) => "No probe response".to_owned(),
        None => c::muted("Not reported"),
    }
}

fn probe_rows(name: &str, machines: &[FleetMachine]) -> String {
    let body = if machines.is_empty() {
        c::muted("No machines registered")
    } else {
        c::rows(machines.iter().map(|machine| {
            let provider = machine.capacity.as_ref().and_then(|report| {
                report
                    .providers
                    .iter()
                    .find(|provider| provider.name == name)
            });
            c::row(&esc(&machine.name), &probe_label(provider), None)
        }))
    };
    c::subsection("CLI probes", &body, None, 4)
}
