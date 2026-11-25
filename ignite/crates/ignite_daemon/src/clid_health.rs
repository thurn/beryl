//! Tracks CLI holds, probes, plan usage and turn costs.
//!
//! - A CLI hold (a row of `cli_holds`) means the app's turns fail for a reason that is not the
//!   session's own: the CLI is not logged in, refuses the model, is missing, exits before its
//!   first line, meets an API error or reaches a plan limit outside the Claude dialect.
//!   A hold blocks new turns ([`Health::may_start`]) and hand-offs ([`Health::offers`]). Slots
//!   blocked from starting their next turn are parked with their tickets held.
//! - Each hold posts one desktop notification when it starts and sends one justiciar report,
//!   tried again each [`Health::tick`] until the hub takes it.
//! - A probe turn is a one-word turn on the cheapest tier of `[pool.models.<app>]`, run in the
//!   Ignite home and never in the checkout. One runs for every app in CLI mode when the daemon
//!   starts, before the app's slots are first offered, and one every [`PROBE_EVERY_S`] while the
//!   app is held. A probe that succeeds ends the hold so parked slots can start their next
//!   turn after checking that their ticket claims are still current.
//! - A Claude-dialect turn that reaches its plan limit stores a 100% sample for the window,
//!   so the hub stops affected hand-offs for that provider until the window resets.
//! - Claude streams usage samples during a turn. Codex saves them to its session transcript.
//!   The daemon stores both as rows of `plan_usage` and sends them to the hub.
//! - Each turn's cost and token counts go to the hub's metrics under [`TURN_SOURCE`], with its
//!   ticket, through the outbox, so none is lost while the hub is down.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use ignite_core::cli::{
    CliApp, Hold, HoldReason, ProbeFailure, SlotState, TurnEnd, TurnSpec, UsageSample,
};
use ignite_core::config::Config;
use ignite_core::plan_usage::parse_resets_at;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Error, Result};
use ignite_store::local::rows::{CliHold, PlanUsage};
use ignite_store::local::{cli_slots, plan_usage};

use crate::pool::Pool;
use crate::server::Context;

/// Seconds between probe turns while an app is on a CLI hold.
pub const PROBE_EVERY_S: f64 = 300.0;
/// The prompt of a probe turn.
pub const PROBE_PROMPT: &str = "Reply with the single word: ready";
/// Seconds a probe turn may run before its supervisor ends it.
pub const PROBE_HANG_S: u64 = 300;
/// The source under which each turn's cost is posted to the hub's metrics.
pub const TURN_SOURCE: &str = "cli_turn";
/// The tiers a probe prefers, cheapest first. An app whose `[pool.models.<app>]` names none of
/// them probes on its first tier.
pub const PROBE_TIERS: [&str; 4] = ["light", "haiku", "sonnet", "opus"];
/// How long a plan-limit sample holds when nothing says when its window resets: long enough
/// not to hand out tickets that would meet the limit again at once, short enough that a wrong
/// guess costs an hour at most.
pub const UNKNOWN_RESET_S: f64 = 3600.0;
const PLAN_LIMIT_MESSAGE: &str = "The plan's limit was reached.";

#[path = "clid_agy_usage.rs"]
mod agy_usage;
#[path = "clid_codex_usage.rs"]
mod codex_usage;
#[path = "clid_health_effects.rs"]
mod effects;

pub use effects::{HealthEffects, record_turn};

/// Whether a CLI dialect reports plan-window usage samples so a `PlanLimit` end posts a
/// 100% sample instead of putting the provider on a local CLI hold.
fn reports_plan_usage(dialect: &str) -> bool {
    matches!(dialect, "claude" | "agy")
}

/// The start of the `park_reason` the loop gives a slot that a CLI hold parked (see `end`).
const HOLD_PARK_PREFIX: &str = "CLI hold (";
/// What `decide` says when it parks a turn that would have started during a hold.
const DECIDE_HOLD_TEXT: &str = " is on a CLI hold (";

/// Whether a probe turn for the app on `hold` is due at `now`, given when the last one ran:
/// [`PROBE_EVERY_S`] after the later of that and the hold's start.
pub fn probe_due(hold: &Hold, last_probe: Option<Timestamp>, now: Timestamp) -> bool {
    let last = last_probe.map_or(hold.since, |at| at.max(hold.since));
    now.seconds_since(last) >= PROBE_EVERY_S
}

/// The `park_reason` of a slot parked by `hold`. A slot parked with this reason starts its next
/// turn when the hold ends.
pub fn hold_park_reason(hold: &Hold) -> String {
    format!(
        "{HOLD_PARK_PREFIX}{}): {}",
        hold.reason.as_str(),
        hold.message
    )
}

/// Whether a slot's `park_reason` says a CLI hold parked it: [`hold_park_reason`]'s text, or the
/// report `decide` gives a turn it did not start because the app was held. Any other parked
/// slot waits for a person.
pub fn parked_by_hold(reason: Option<&str>) -> bool {
    reason.is_some_and(|text| text.starts_with(HOLD_PARK_PREFIX) || text.contains(DECIDE_HOLD_TEXT))
}

/// The label `get_usage` gives the window the CLI calls `window`: `seven_day` is
/// "Weekly · all models", `five_hour` is "5-hour", `seven_day_<family>` is
/// "Weekly · <Family>", and any other window keeps its own name, which holds every hand-off for its
/// provider (see `ignite_core::plan_usage`).
pub fn window_label(window: &str) -> String {
    match window {
        "seven_day" => "Weekly · all models".to_owned(),
        "five_hour" => "5-hour".to_owned(),
        other => match other.strip_prefix("seven_day_") {
            Some(family) if !family.is_empty() => {
                let mut chars = family.chars();
                let first = chars.next().map(|c| c.to_uppercase().collect::<String>());
                format!("Weekly · {}{}", first.unwrap_or_default(), chars.as_str())
            }
            _ => other.to_owned(),
        },
    }
}

/// A utilization from 0 to 1 as a percentage, rounded to hundredths so that 0.81 is 81.
fn percent(utilization: f64) -> f64 {
    (utilization * 10_000.0).round() / 100.0
}

/// The `plan_usage` rows of a turn's usage samples, one per sample.
fn usage_rows(
    provider: &str,
    samples: &[UsageSample],
    machine: &str,
    now: Timestamp,
) -> Vec<PlanUsage> {
    samples
        .iter()
        .map(|sample| PlanUsage {
            id: 0,
            at: now.to_string(),
            machine: machine.to_owned(),
            status: "ok".to_owned(),
            label: Some(window_label(&sample.window)),
            percent_used: Some(percent(sample.utilization)),
            resets_at: sample.resets_at.clone(),
            note: sample
                .status
                .as_deref()
                .filter(|status| *status != "allowed")
                .map(|status| format!("CLI status {status}")),
            posted: 0,
            provider: provider.to_owned(),
        })
        .collect()
}

/// The model ID a probe of `app` runs on: the cheapest of [`PROBE_TIERS`] that
/// `[pool.models.<app>]` names, else its first tier, else the cheapest tier's own name.
pub fn probe_model(config: &Config, app: &str) -> Option<String> {
    let Some(table) = config.pool.models.get(app) else {
        return PROBE_TIERS.first().map(|tier| (*tier).to_owned());
    };
    PROBE_TIERS
        .iter()
        .find_map(|tier| table.get(*tier))
        .or_else(|| table.values().next())
        .cloned()
}

/// The model a worker uses for an untiered ticket.
pub fn default_model(config: &Config, app: &str) -> Option<String> {
    config
        .pool
        .model_id(app, "sonnet")
        .or_else(|| probe_model(config, app))
}

fn store_error(error: &ignite_store::StoreError) -> Error {
    Error::Io(error.to_string())
}

/// The hold a `cli_holds` row stores. A reason this build does not know still holds, as an API
/// error, so that a newer build's hold is never ignored.
pub(super) fn hold_of(row: &CliHold, fallback_since: Timestamp) -> Hold {
    let reason = HoldReason::parse(&row.reason).unwrap_or(HoldReason::ApiError);
    Hold {
        app: row.app.clone(),
        reason,
        since: row
            .since
            .as_deref()
            .and_then(|at| Timestamp::parse(at).ok())
            .unwrap_or(fallback_since),
        cli_version: row.cli_version.clone(),
        message: row.message.clone().unwrap_or_default(),
        latest_probe_failure: row.probe_reason.as_deref().map(|reason| ProbeFailure {
            reason: HoldReason::parse(reason).unwrap_or(HoldReason::ApiError),
            message: row.probe_message.clone().unwrap_or_default(),
        }),
    }
}

/// Whether a probe that ran after `row`'s hold started has failed. A hold that a failed probe
/// started has `probed_at` equal to `since`, so it waits for the next probe too.
fn probe_confirmed(row: &CliHold) -> bool {
    let at = |field: &Option<String>| field.as_deref().and_then(|t| Timestamp::parse(t).ok());
    matches!((at(&row.since), at(&row.probed_at)), (Some(since), Some(probed)) if probed > since)
}

fn hold_row(
    app: &str,
    reason: HoldReason,
    message: &str,
    cli_version: Option<&str>,
    now: Timestamp,
) -> CliHold {
    CliHold {
        app: app.to_owned(),
        reason: reason.as_str().to_owned(),
        since: Some(now.to_string()),
        cli_version: cli_version.map(str::to_owned),
        message: Some(first_line(message)),
        probed_at: None,
        reported: 0,
        probe_reason: None,
        probe_message: None,
    }
}

/// The first line of `text`, at most 200 characters.
fn first_line(text: &str) -> String {
    text.lines()
        .next()
        .unwrap_or("")
        .chars()
        .take(200)
        .collect()
}

/// How a probe turn went.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Probe {
    /// The turn ended cleanly, or at the plan's limit, which also shows the CLI works.
    Passed,
    /// The turn failed; `end` is how it ended.
    Failed { end: TurnEnd },
}

/// Posts a desktop notification with a title and text.
pub type Notify = Box<dyn Fn(&str, &str) + Send + Sync>;

/// Each app's CLI hold and probe, over the daemon's local database.
pub struct Health {
    pool: Arc<Pool>,
    /// The Ignite home, where probe turns run.
    home: PathBuf,
    /// The machine's hostname, for notifications and reports.
    host: String,
    notify: Notify,
    /// The apps a probe has passed for since the daemon started. Only these are offered.
    probed: Mutex<BTreeSet<String>>,
}

impl Health {
    pub fn new(pool: Arc<Pool>, home: PathBuf, host: String) -> Health {
        Health {
            pool,
            home,
            host,
            notify: Box::new(ignite_os::notify::post),
            probed: Mutex::new(BTreeSet::new()),
        }
    }

    /// Posts notifications through `notify` instead of the system's.
    #[must_use]
    pub fn with_notify(mut self, notify: Notify) -> Health {
        self.notify = notify;
        self
    }

    fn probed(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.probed.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every app's CLI hold. A database that cannot be read reports none, and says so.
    pub fn holds(&self) -> Vec<Hold> {
        match self.pool.with_database(|conn| cli_slots::holds(conn)) {
            Ok(rows) => rows
                .iter()
                .map(|row| hold_of(row, Timestamp::EPOCH))
                .collect(),
            Err(error) => {
                eprintln!("ignite: CLI holds not read: {error}");
                Vec::new()
            }
        }
    }

    /// `app`'s CLI hold, if it is on one.
    pub fn hold(&self, app: &str) -> Option<Hold> {
        self.holds().into_iter().find(|hold| hold.app == app)
    }

    /// Whether a turn may start for `app`: it is on no CLI hold.
    pub fn may_start(&self, app: &str) -> bool {
        self.hold(app).is_none()
    }

    /// Whether `app`'s slots may be offered to the hub: a probe has passed since the daemon
    /// started and the app is on no CLI hold.
    pub fn offers(&self, app: &str) -> bool {
        self.probed().contains(app) && self.may_start(app)
    }

    /// Puts `app` on a CLI hold, unless it is on one already. A new hold posts its
    /// notification at once; its justiciar report goes at the next [`Health::tick`]. Returns
    /// whether the hold is new.
    pub fn start_hold(
        &self,
        app: &str,
        reason: HoldReason,
        message: &str,
        cli_version: Option<&str>,
        now: Timestamp,
    ) -> Result<bool> {
        self.record_hold(&hold_row(app, reason, message, cli_version, now))
    }

    fn record_hold(&self, row: &CliHold) -> Result<bool> {
        let new = {
            let _admission = super::lock(&self.pool.cli_admission);
            self.pool
                .with_database(|conn| cli_slots::record_hold(conn, row))
                .map_err(|e| store_error(&e))?
        };
        if new {
            let hold = hold_of(row, Timestamp::EPOCH);
            let app = &hold.app;
            (self.notify)(
                &format!("Ignite holds {app} CLI workers on {}", self.host),
                &format!(
                    "{}: {}{}. No {app} turn starts until a probe turn passes; one runs every 5 \
                     minutes.",
                    hold.reason.as_str(),
                    hold.message,
                    version_note(hold.cli_version.as_deref())
                ),
            );
        }
        Ok(new)
    }

    /// Ends `app`'s hold and queues the slots it parked. Each goes to `backing_off` with its
    /// resume time at `now`, keeping its park reason so the next turn checks its claim.
    /// A slot whose last turn printed nothing gets that turn's prompt again. A slot with no
    /// prompt waiting is told its last turn failed. Returns how many slots were queued.
    pub fn end_hold(&self, app: &str, now: Timestamp) -> Result<u32> {
        self.probed().insert(app.to_owned());
        self.pool
            .with_database(|conn| -> ignite_store::Result<u32> {
                cli_slots::end_hold(conn, app)?;
                let mut resumed = 0;
                for mut slot in cli_slots::list(conn)? {
                    if slot.app != app
                        || slot.state != SlotState::Parked.as_str()
                        || !parked_by_hold(slot.park_reason.as_deref())
                    {
                        continue;
                    }
                    if slot.next_prompt.is_none() {
                        slot.next_prompt = Some(resume_prompt(conn, &slot)?);
                    }
                    SlotState::BackingOff.as_str().clone_into(&mut slot.state);
                    slot.resume_at = Some(now.to_string());
                    slot.since = Some(now.to_string());
                    cli_slots::put(conn, &slot)?;
                    resumed += 1;
                }
                Ok(resumed)
            })
            .map_err(|e| store_error(&e))
    }

    /// Runs one probe turn of `app` in the Ignite home and waits for it.
    fn probe(&self, ctx: &Context, name: &str, app: &dyn CliApp) -> Probe {
        let failed = |reason, message: String| Probe::Failed {
            end: TurnEnd::Hold { reason, message },
        };
        let Some(program) = ctx.config.cli_program(name) else {
            return failed(
                HoldReason::ProgramMissing,
                format!("ignite.toml has no [pool.cli.{name}] table"),
            );
        };
        let Some(model) = probe_model(&ctx.config, name) else {
            return failed(
                HoldReason::ModelRefused,
                format!("[pool.models.{name}] names no model"),
            );
        };
        let spec = TurnSpec {
            app: name.to_owned(),
            program: program.program.clone(),
            args: program.args.clone(),
            model,
            effort: None,
            transcript: None,
            new_transcript: None,
            prompt: PROBE_PROMPT.to_owned(),
        };
        let cwd = self.home.join("probe");
        if let Err(error) = std::fs::create_dir_all(&cwd) {
            return failed(
                HoldReason::ProgramMissing,
                format!("{} could not be made: {error}", cwd.display()),
            );
        }
        let started = ctx.clock.now();
        let result = ctx.launcher.run(&CommandSpec {
            args: app.command(&spec),
            cwd,
            env: Vec::new(),
            class: Class::Agent,
            cores: 0,
            resources: Vec::new(),
            grows: false,
            tree: None,
            ticket: None,
            hang_limit_s: Some(PROBE_HANG_S),
            reserve: None,
        });
        let output = std::fs::read_to_string(&result.output).unwrap_or_default();
        let events = app.read(&output);
        let end = if result.hung {
            TurnEnd::Interrupted
        } else {
            app.classify(&events, result.exit)
        };
        if matches!(end, TurnEnd::PlanLimit { .. }) && !reports_plan_usage(program.dialect(name)) {
            return Probe::Failed { end };
        }
        let now = ctx.clock.now();
        if let Err(error) = self.record_usage(ctx, name, &events.usage, now) {
            eprintln!("ignite: usage of a {name} probe not stored: {error}");
        }
        self.record_codex_usage(ctx, name, &events, Some(started));
        let agy_samples = self.record_agy_usage(ctx, name);
        let limit_samples = if agy_samples.is_empty() {
            &events.usage
        } else {
            &agy_samples
        };
        match end {
            TurnEnd::Clean => Probe::Passed,
            TurnEnd::PlanLimit { window } => {
                if let Err(error) =
                    self.plan_limit(ctx, name, window.as_deref(), limit_samples, now)
                {
                    eprintln!("ignite: plan-limit sample of a {name} probe not stored: {error}");
                }
                Probe::Passed
            }
            end => Probe::Failed { end },
        }
    }

    /// Probes `app` and acts on the result: a pass ends its hold, if any, and lets its slots be
    /// offered; a failure records the probe's time and cause and starts a hold if needed.
    fn probe_and_apply(&self, ctx: &Context, name: &str, app: &dyn CliApp) {
        let outcome = self.probe(ctx, name, app);
        let now = ctx.clock.now();
        match &outcome {
            Probe::Passed => match self.end_hold(name, now) {
                Ok(resumed) if resumed > 0 => {
                    eprintln!("ignite: a {name} probe passed; {resumed} parked slots queued");
                }
                Ok(_) => {}
                Err(error) => eprintln!("ignite: the {name} CLI hold was not ended: {error}"),
            },
            Probe::Failed { end } => {
                let (reason, message) = hold_cause(end);
                let mut row = hold_row(name, reason, &message, None, now);
                row.probed_at.clone_from(&row.since);
                row.probe_reason = Some(row.reason.clone());
                row.probe_message.clone_from(&row.message);
                if let Err(error) = self.record_hold(&row) {
                    eprintln!("ignite: the {name} CLI hold was not stored: {error}");
                }
            }
        }
    }

    /// One round for the apps in CLI mode, `apps`: probes each app that has not passed a probe
    /// since the daemon started and each held app whose probe is due, then sends each hold's
    /// justiciar report that the hub has not yet taken. A probe runs a whole turn, so a round
    /// can take a minute.
    pub fn tick(&self, ctx: &Context, apps: &[(&str, &dyn CliApp)]) {
        let now = ctx.clock.now();
        let rows = self
            .pool
            .with_database(|conn| cli_slots::holds(conn))
            .unwrap_or_default();
        for &(name, app) in apps {
            let row = rows.iter().find(|row| row.app == name);
            let due = match row {
                None => !self.probed().contains(name),
                Some(row) => {
                    let last = row
                        .probed_at
                        .as_deref()
                        .and_then(|at| Timestamp::parse(at).ok());
                    probe_due(&hold_of(row, Timestamp::EPOCH), last, now)
                }
            };
            if due {
                self.probe_and_apply(ctx, name, app);
            }
        }
        self.report_holds(ctx);
    }

    /// Sends the justiciar report of every hold not yet reported that a later probe confirmed,
    /// and marks each one the hub took.
    ///
    /// A provider's backend often fails for a few minutes and recovers, as when Antigravity
    /// returned an empty conversation ID. Reporting at the first failure filed a justiciar
    /// ticket each time, and the next probe had already ended the hold before anyone read it.
    /// So a hold is reported only once a probe that ran after it started has also failed.
    fn report_holds(&self, ctx: &Context) {
        let rows = self
            .pool
            .with_database(|conn| cli_slots::holds(conn))
            .unwrap_or_default();
        for row in rows
            .iter()
            .filter(|row| row.reported == 0 && probe_confirmed(row))
        {
            let hold = hold_of(row, Timestamp::EPOCH);
            let report = JusticiarReport {
                subject: format!("cli-hold:{}", hold.app),
                title: format!(
                    "{} CLI turns fail on {}: {}",
                    hold.app,
                    self.host,
                    hold.reason.as_str()
                ),
                details: format!(
                    "Since {} no {app} turn starts on {host}, because a turn failed for a reason \
                     that is not its session's: {} ({}){}. The slots it stopped are parked with \
                     their tickets held. The daemon runs a one-word probe turn every 5 minutes \
                     and resumes them when one passes. `ignite pool list` shows the hold; run \
                     the CLI by hand on {host} to see the error.",
                    hold.since,
                    hold.message,
                    hold.reason.as_str(),
                    version_note(hold.cli_version.as_deref()),
                    app = hold.app,
                    host = self.host,
                ),
                evidence: hold.since.to_string(),
                machine: ctx.machine.clone(),
                host: self.host.clone(),
                on_machine: true,
            };
            match ctx.hub.justiciar(&report) {
                Ok(_) => {
                    let _ = self
                        .pool
                        .with_database(|conn| cli_slots::hold_reported(conn, &hold.app));
                }
                Err(error) => eprintln!("ignite: the {} CLI hold not reported: {error}", hold.app),
            }
        }
    }

    /// Stores the usage samples of a turn as `plan_usage` rows and posts every row the hub has
    /// not acknowledged. The reader supplies each newly observed sample, even when its
    /// percentage matches an older local sample: another machine may have sent a higher one.
    pub fn record_usage(
        &self,
        ctx: &Context,
        provider: &str,
        samples: &[UsageSample],
        now: Timestamp,
    ) -> Result<()> {
        let rows = usage_rows(provider, samples, ctx.machine.as_str(), now);
        self.store_samples(ctx, rows, now)
    }

    /// Posts a 100% sample for the Claude window that reached its limit. If the turn names no
    /// window, use its most-used window or default to the five-hour window. Use the reset from
    /// the turn or the latest stored sample if still ahead, otherwise [`UNKNOWN_RESET_S`].
    fn plan_limit(
        &self,
        ctx: &Context,
        provider: &str,
        window: Option<&str>,
        samples: &[UsageSample],
        now: Timestamp,
    ) -> Result<()> {
        let window = window.map_or_else(
            || {
                samples
                    .iter()
                    .max_by(|a, b| a.utilization.total_cmp(&b.utilization))
                    .map_or_else(|| "five_hour".to_owned(), |s| s.window.clone())
            },
            str::to_owned,
        );
        let label = window_label(&window);
        let ahead = |text: &str| parse_resets_at(text).is_some_and(|at| at > now);
        let from_turn = samples
            .iter()
            .rev()
            .filter(|s| s.window == window)
            .find_map(|s| s.resets_at.clone())
            .filter(|text| ahead(text));
        let stored = || {
            self.pool
                .with_database(|conn| plan_usage::latest(conn))
                .unwrap_or_default()
                .into_iter()
                .find(|row| {
                    row.provider == provider && row.label.as_deref() == Some(label.as_str())
                })
                .and_then(|row| row.resets_at)
                .filter(|text| ahead(text))
        };
        let resets_at = from_turn
            .or_else(stored)
            .unwrap_or_else(|| now.plus_seconds(UNKNOWN_RESET_S).to_string());
        let row = PlanUsage {
            id: 0,
            at: now.to_string(),
            machine: ctx.machine.as_str().to_owned(),
            status: "limit".to_owned(),
            label: Some(label),
            percent_used: Some(100.0),
            resets_at: Some(resets_at),
            note: Some("a CLI turn reached the plan's limit".to_owned()),
            posted: 0,
            provider: provider.to_owned(),
        };
        self.store_samples(ctx, vec![row], now)
    }

    fn store_samples(&self, ctx: &Context, rows: Vec<PlanUsage>, now: Timestamp) -> Result<()> {
        // The reader may see several reports of the same window in one batch.
        let mut last: Vec<PlanUsage> = Vec::new();
        for row in rows {
            last.retain(|kept| kept.label != row.label);
            last.push(row);
        }
        if last.is_empty() {
            return Ok(());
        }
        self.pool
            .with_database(|conn| plan_usage::insert(conn, &last, now))
            .map_err(|e| store_error(&e))?;
        crate::dispatchd::post_usage(&self.pool, ctx.hub.as_ref(), &ctx.machine)
    }
}

/// The reason and message of the hold a failed probe starts.
fn hold_cause(end: &TurnEnd) -> (HoldReason, String) {
    match end {
        TurnEnd::Hold { reason, message } => (*reason, message.clone()),
        TurnEnd::Interrupted => (
            HoldReason::NoFirstLine,
            "the probe turn printed no result".to_owned(),
        ),
        TurnEnd::Unknown { why } => (HoldReason::ApiError, format!("probe turn: {why}")),
        TurnEnd::PlanLimit { .. } => (HoldReason::PlanLimit, PLAN_LIMIT_MESSAGE.to_owned()),
        TurnEnd::Clean => (
            HoldReason::ApiError,
            "the probe turn passed but was counted as failed".to_owned(),
        ),
    }
}

/// ", with CLI <version>" when the version is known.
fn version_note(version: Option<&str>) -> String {
    version.map_or_else(String::new, |v| format!(", with CLI {v}"))
}

/// The prompt a slot parked by a hold resumes with when none waits: its last turn's prompt
/// again if that turn printed nothing the transcript kept (the slot has no transcript), else a
/// note that its last turn failed.
fn resume_prompt(
    conn: &rusqlite::Connection,
    slot: &ignite_store::local::rows::CliSlot,
) -> ignite_store::Result<String> {
    if slot.transcript.is_none()
        && let Some(session) = &slot.session
        && let Some(last) = cli_slots::turns(conn, session)?.pop()
    {
        return Ok(last.prompt);
    }
    Ok(
        "Your last turn failed for a reason outside your session (the CLI could not reach its \
        service), and it works again now. Run `ignite status` and continue."
            .to_owned(),
    )
}

#[cfg(test)]
#[path = "clid_health_tests.rs"]
mod clid_health_tests;
