//! CLI mode's health: each app's CLI hold and the probe turn that ends it, the plan-limit sample,
//! the usage samples a turn reports, and each turn's cost (see "CLI mode" in the design
//! document).
//!
//! - A CLI hold (a row of `cli_holds`) means the app's turns fail for a reason that is not the
//!   session's own: the CLI is not logged in, refuses the model, is missing, exits before its
//!   first line, or meets an API error. While an app is held, no turn starts for it
//!   ([`Health::may_start`]) and its slots are not offered to the hub ([`Health::offers`]). A
//!   hold never releases a ticket: the slots it stops are parked with their tickets held.
//! - Each hold posts one desktop notification when it starts and sends one justiciar report,
//!   tried again each [`Health::tick`] until the hub takes it.
//! - A probe turn is a one-word turn on the cheapest tier of `[pool.models.<app>]`, run in the
//!   Ignite home and never in the checkout. One runs for every app in CLI mode when the daemon
//!   starts, before the app's slots are first offered, and one every [`PROBE_EVERY_S`] while the
//!   app is held. A probe that succeeds ends the hold so parked slots can start their next
//!   turn after checking that their ticket claims are still current.
//! - A turn that ended at the plan's limit stores a 100% sample of the window, so the usage hold
//!   at the hub stops affected hand-offs for that provider until the window resets.
//! - Claude streams usage samples during a turn. Codex saves them to its session transcript.
//!   The daemon stores both as rows of `plan_usage` and sends them to the hub.
//! - Each turn's cost and token counts go to the hub's metrics under [`TURN_SOURCE`], with its
//!   ticket, through the outbox, so none is lost while the hub is down.
//!
//! [`HealthEffects`] is how the `ignite-cli` loop reaches all of this: it is the loop's
//! [`Effects`], and the daemon calls [`Health::tick`] for the probes and reports.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};

use serde_json::json;

use ignite_core::cli::{
    CliApp, Hold, HoldReason, Slot, SlotState, TurnEnd, TurnEvents, TurnSpec, UsageSample,
};
use ignite_core::config::Config;
use ignite_core::ids::{SessionId, TicketId};
use ignite_core::plan_usage::parse_resets_at;
use ignite_core::protocol::Class;
use ignite_core::protocol::hub::{JusticiarReport, MetricsImport};
use ignite_core::protocol::tickets::Status;
use ignite_core::time::Timestamp;
use ignite_core::traits::{CommandSpec, Error, Result};
use ignite_store::local::rows::{CliHold, PlanUsage};
use ignite_store::local::{cli_slots, plan_usage};

use self::codex_usage::uses_codex_rollout;
use super::Effects;
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

#[path = "clid_codex_usage.rs"]
mod codex_usage;

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
fn hold_of(row: &CliHold) -> Hold {
    let reason = HoldReason::parse(&row.reason).unwrap_or(HoldReason::ApiError);
    Hold {
        app: row.app.clone(),
        reason,
        since: row
            .since
            .as_deref()
            .and_then(|at| Timestamp::parse(at).ok())
            .unwrap_or_else(|| Timestamp::from_millis(0)),
        cli_version: row.cli_version.clone(),
        message: row.message.clone().unwrap_or_default(),
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
            Ok(rows) => rows.iter().map(hold_of).collect(),
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
        let row = CliHold {
            app: app.to_owned(),
            reason: reason.as_str().to_owned(),
            since: Some(now.to_string()),
            cli_version: cli_version.map(str::to_owned),
            message: Some(first_line(message)),
            probed_at: None,
            reported: 0,
        };
        let new = {
            let _admission = super::lock(&self.pool.cli_admission);
            self.pool
                .with_database(|conn| cli_slots::start_hold(conn, &row))
                .map_err(|e| store_error(&e))?
        };
        if new {
            let hold = hold_of(&row);
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
        let mut events = app.read(&output);
        let now = ctx.clock.now();
        if let Err(error) = self.record_usage(ctx, name, &events.usage, now) {
            eprintln!("ignite: usage of a {name} probe not stored: {error}");
        }
        if let Some(samples) = self.record_codex_usage(ctx, name, &events, Some(started)) {
            events.usage = samples;
        }
        let end = if result.hung {
            TurnEnd::Interrupted
        } else {
            app.classify(&events, result.exit)
        };
        match end {
            TurnEnd::Clean => Probe::Passed,
            TurnEnd::PlanLimit { window } => {
                if let Err(error) =
                    self.plan_limit(ctx, name, window.as_deref(), &events.usage, now)
                {
                    eprintln!("ignite: plan-limit sample of a {name} probe not stored: {error}");
                }
                Probe::Passed
            }
            end => Probe::Failed { end },
        }
    }

    /// Probes `app` and acts on the result: a pass ends its hold, if any, and lets its slots be
    /// offered; a failure starts a hold, or records the probe's time on the one it is on.
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
                if let Err(error) = self.start_hold(name, reason, &message, None, now) {
                    eprintln!("ignite: the {name} CLI hold was not stored: {error}");
                }
                let at = now.to_string();
                let _ = self
                    .pool
                    .with_database(|conn| cli_slots::hold_probed(conn, name, &at));
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
                    probe_due(&hold_of(row), last, now)
                }
            };
            if due {
                self.probe_and_apply(ctx, name, app);
            }
        }
        self.report_holds(ctx);
    }

    /// Sends the justiciar report of every hold not yet reported, and marks each one the hub
    /// took.
    fn report_holds(&self, ctx: &Context) {
        let rows = self
            .pool
            .with_database(|conn| cli_slots::holds(conn))
            .unwrap_or_default();
        for row in rows.iter().filter(|row| row.reported == 0) {
            let hold = hold_of(row);
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

    /// Stores a 100% sample of `window`, the window a turn reached the plan's limit in, so the
    /// usage hold starts, and posts it. The reset is the one the turn's own samples gave for the
    /// window, else the window's newest stored row's if it is still ahead, else
    /// [`UNKNOWN_RESET_S`] from `now`. A limit that named no window is stored for the window
    /// the turn's samples show most used, else as "5-hour", which holds every hand-off.
    /// For Codex, an unnamed limit only uses a sampled window when the reading reached 100%.
    fn plan_limit(
        &self,
        ctx: &Context,
        provider: &str,
        window: Option<&str>,
        samples: &[UsageSample],
        now: Timestamp,
    ) -> Result<()> {
        let uses_rollout = uses_codex_rollout(&ctx.config, provider);
        let window = window.map_or_else(
            || {
                samples
                    .iter()
                    .filter(|sample| !uses_rollout || sample.utilization >= 1.0)
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

/// Queues turn `turn` of `session`'s cost and token counts for the hub's metrics under
/// [`TURN_SOURCE`], with its ticket. A turn with no result has no cost and queues nothing.
pub fn record_turn(
    pool: &Pool,
    ctx: &Context,
    app: &str,
    session: &SessionId,
    ticket: Option<&TicketId>,
    turn: u32,
    events: &TurnEvents,
) -> Result<()> {
    if events.cost_usd.is_none() && events.tokens.is_none() {
        return Ok(());
    }
    let now = ctx.clock.now();
    let tokens = events.tokens.unwrap_or_default();
    let import = MetricsImport {
        machine: ctx.machine.clone(),
        source: TURN_SOURCE.to_owned(),
        rows: vec![json!({
            "at": now.to_string(),
            "app": app,
            "session": session.as_str(),
            "ticket": ticket.map(TicketId::as_str),
            "turn": turn,
            "model": events.model,
            "cli_version": events.cli_version,
            "is_error": events.result_error,
            "cost_usd": events.cost_usd,
            "input_tokens": tokens.input,
            "output_tokens": tokens.output,
            "cache_read_tokens": tokens.cache_read,
            "cache_write_tokens": tokens.cache_write,
        })],
    };
    pool.with_database(|conn| crate::outbox::push_metrics(conn, &import, now))
}

/// The `ignite-cli` loop's [`Effects`]: usage samples, each ended turn's cost and plan-limit
/// sample, and CLI holds go to [`Health`], and a slot is offered only once a probe has passed.
pub struct HealthEffects {
    pub health: Arc<Health>,
    /// The daemon's context, for the hub, the clock, the machine and `[pool]`.
    pub ctx: Context,
}

impl Effects for HealthEffects {
    fn usage(&self, slot: &Slot, samples: &[UsageSample]) {
        let now = self.ctx.clock.now();
        if let Err(error) = self.health.record_usage(&self.ctx, &slot.app, samples, now) {
            eprintln!("ignite: CLI usage samples not stored: {error}");
        }
    }

    fn ended(&self, slot: &Slot, end: &TurnEnd, events: &TurnEvents) {
        let now = self.ctx.clock.now();
        let codex = self
            .health
            .record_codex_usage(&self.ctx, &slot.app, events, slot.since);
        if let TurnEnd::PlanLimit { window } = end {
            let samples = codex.as_deref().unwrap_or(&events.usage);
            if let Err(error) =
                self.health
                    .plan_limit(&self.ctx, &slot.app, window.as_deref(), samples, now)
            {
                eprintln!("ignite: the plan-limit sample was not stored: {error}");
            }
        }
        if let Some(session) = &slot.session
            && let Err(error) = record_turn(
                &self.health.pool,
                &self.ctx,
                &slot.app,
                session,
                slot.ticket.as_ref(),
                slot.turn,
                events,
            )
        {
            eprintln!("ignite: the cost of {session}'s turn was not queued: {error}");
        }
    }

    fn handoff(&self, slot: &Slot, note: &str, resumed: bool) {
        use ignite_core::protocol::tickets::{BatchRequest, CULPRIT_HANDOFF_KEY, Operation};

        let (Some(ticket), Some(session)) = (&slot.ticket, &slot.session) else {
            return;
        };
        let request = BatchRequest {
            preconditions: Vec::new(),
            ops: vec![Operation::Update {
                id: ticket.to_string(),
                set: None,
                add_labels: Vec::new(),
                remove_labels: Vec::new(),
                set_meta: std::collections::BTreeMap::from([(
                    CULPRIT_HANDOFF_KEY.to_owned(),
                    serde_json::json!({"session": session, "note": note, "resumed": resumed}),
                )]),
                unset_meta: Vec::new(),
            }],
        };
        if let Err(error) = self.ctx.hub.batch(&request) {
            eprintln!("ignite: the hand-off of {ticket} was not recorded: {error}");
        }
    }

    fn hold(&self, app: &str, reason: HoldReason, message: &str, cli_version: Option<&str>) {
        let now = self.ctx.clock.now();
        if let Err(error) = self
            .health
            .start_hold(app, reason, message, cli_version, now)
        {
            eprintln!("ignite: the {app} CLI hold was not stored: {error}");
        }
    }

    fn probed(&self, app: &str) -> bool {
        self.health.probed().contains(app)
    }

    fn claim_is_current(&self, session: &SessionId, ticket: &TicketId) -> Result<bool> {
        match self.ctx.hub.ticket(ticket) {
            Ok(detail) => Ok(detail.ticket.status == Status::InProgress
                && detail.ticket.assignee.as_deref() == Some(session.as_str())
                && detail.ticket.session.as_ref() == Some(session)
                && detail.ticket.machine.as_ref() == Some(&self.ctx.machine)),
            Err(Error::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
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
        TurnEnd::Clean | TurnEnd::PlanLimit { .. } => (
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
