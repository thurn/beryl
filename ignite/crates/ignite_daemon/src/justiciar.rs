//! The daemon's side of the justiciar (`ignite guide justiciar`): what it reports to the hub as
//! an emergency, and what it does when the hub cannot be reached to hear it.
//!
//! [`Reporter::tick`] runs in the sender loop after each heartbeat:
//!
//! - Each crash record in the Ignite home's `crash` folder is sent as a justiciar report and
//!   then marked reported. A crash is a bug in Ignite's code, which any machine's justiciar can
//!   fix, so the report is not tied to this machine.
//! - Each loop that [`LoopHealth`] calls unhealthy is reported once while it stays unhealthy,
//!   tied to this machine, whose state is what has to be looked at. A loop whose round is
//!   waiting in the scheduler's queue for cores is not reported: work of a higher class holds
//!   them, which `ignite status --admission` shows. On 2026-11-20 cygnus filed a P0 justiciar
//!   ticket because its idle loop waited 10 minutes for 12 cores while a tip round and an
//!   agent's build used all 18, which is how idle work is meant to behave. A scheduler that
//!   grants nothing at all would also stall landings and tip rounds, and the hub files a ticket
//!   when `master` stops moving.
//! - A shared failure, one check failing the same way for several workers here, is reported
//!   once, unless an open regression ticket already names the check (see `dispatchd_shared.rs`).
//! - A request `ignite justiciar` made while the hub was down is sent once it answers again.
//! - When the hub is unreachable for [`HUB_DOWN_S`], the daemon starts a local CLI justiciar.
//!
//! The `justiciar` verb is how a session asks for a justiciar.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use serde::{Deserialize, Serialize};

use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::JusticiarReport;
use ignite_core::time::Timestamp;
use ignite_core::traits::Error;

use crate::crash;
use crate::dispatchd::Dispatcher;
use crate::loop_health::LoopHealth;
use crate::server::{Context, Verb};

/// How long the hub may fail to answer before this machine starts a justiciar of its own.
pub const HUB_DOWN_S: f64 = 15.0 * 60.0;

/// A request made while the hub could not be reached, kept in the Ignite home until it can.
const LOCAL_REQUEST: &str = "justiciar-request.json";

/// The most lines of a backtrace a report carries.
const BACKTRACE_LINES: usize = 40;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct LocalRequest {
    at: Timestamp,
    subject: String,
    reason: String,
    on_machine: bool,
}

#[derive(Debug, Default)]
struct ReporterState {
    hub_down_since: Option<Timestamp>,
    /// Loops already reported while they stay unhealthy.
    reported_loops: BTreeSet<String>,
}

/// Sends the daemon's emergencies to the hub; see the module's documentation.
pub struct Reporter {
    pub home: PathBuf,
    pub host: String,
    pub version: String,
    pub loops: LoopHealth,
    pub dispatcher: Arc<Dispatcher>,
    state: Mutex<ReporterState>,
}

fn report(
    ctx: &Context,
    host: &str,
    subject: String,
    title: String,
    details: String,
) -> JusticiarReport {
    JusticiarReport {
        subject,
        title,
        details,
        evidence: String::new(),
        machine: ctx.machine.clone(),
        host: host.to_owned(),
        on_machine: false,
    }
}

fn crash_report(
    ctx: &Context,
    host: &str,
    path: &Path,
    record: &crash::CrashRecord,
) -> JusticiarReport {
    let backtrace: Vec<&str> = record.backtrace.lines().take(BACKTRACE_LINES).collect();
    let details = format!(
        "Ignite {} panicked on thread `{}` at {} at {}:\n\n{}\n\nBacktrace:\n\n{}\n\nThe record \
         is {} on {host}. Fix the cause with a regression test and deploy it through the \
         release branch.",
        record.commit,
        record.thread,
        record.at,
        record.location,
        record.message,
        backtrace.join("\n"),
        path.display()
    );
    let mut report = report(
        ctx,
        host,
        format!("crash:{}:{}", record.thread, record.location),
        format!(
            "Ignite panicked on {}: {}",
            record.thread,
            first_line(&record.message)
        ),
        details,
    );
    report.evidence = path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    report
}

fn first_line(text: &str) -> &str {
    let line = text.lines().next().unwrap_or("");
    line.char_indices()
        .nth(80)
        .map_or(line, |(at, _)| &line[..at])
}

impl Reporter {
    pub fn new(
        home: PathBuf,
        host: String,
        version: String,
        loops: LoopHealth,
        dispatcher: Arc<Dispatcher>,
    ) -> Self {
        Self {
            home,
            host,
            version,
            loops,
            dispatcher,
            state: Mutex::new(ReporterState::default()),
        }
    }

    fn state(&self) -> std::sync::MutexGuard<'_, ReporterState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// One round, after a heartbeat that reached the hub (`hub_answered`) or not.
    pub fn tick(&self, ctx: &Context, hub_answered: bool) {
        let now = ctx.clock.now();
        if !hub_answered {
            let since = *self.state().hub_down_since.get_or_insert(now);
            if now.seconds_since(since) >= HUB_DOWN_S {
                let prompt = self.hub_down_prompt(ctx, since);
                if let Some(worker) = self.dispatcher.hand_off_justiciar(ctx, &prompt) {
                    eprintln!("ignite: the hub is down; gave {worker} a justiciar prompt");
                }
            }
            return;
        }
        self.state().hub_down_since = None;
        self.send_crashes(ctx);
        self.send_loops(ctx, now);
        self.dispatcher.report_shared(ctx, &self.host);
        self.send_local_request(ctx);
    }

    fn hub_down_prompt(&self, ctx: &Context, since: Timestamp) -> String {
        format!(
            "You are this machine's justiciar ({}). The Ignite hub at {} has not answered since \
             {since}, so no tickets can be claimed, landed or filed anywhere. Read `ignite guide \
             justiciar`, find out why (the hub runs inside the daemon on its machine; look at that \
             daemon's log, `ignite doctor`, the network) and repair it. You hold no ticket: once \
             the hub answers, record what you did with `ignite ticket create` and close it.",
            self.host, ctx.config.hub.url
        )
    }

    fn send_crashes(&self, ctx: &Context) {
        for (path, record) in crash::pending(&self.home) {
            match ctx
                .hub
                .justiciar(&crash_report(ctx, &self.host, &path, &record))
            {
                Ok(_) => {
                    if let Err(error) = crash::mark_reported(&path) {
                        eprintln!(
                            "ignite: crash record {} not marked: {error}",
                            path.display()
                        );
                    }
                }
                Err(error) => {
                    eprintln!(
                        "ignite: crash record {} not reported: {error}",
                        path.display()
                    );
                    return;
                }
            }
        }
    }

    fn send_loops(&self, ctx: &Context, now: Timestamp) {
        for loop_report in self.loops.reports() {
            let Some(problem) = loop_report.problem(now) else {
                self.state().reported_loops.remove(&loop_report.name);
                continue;
            };
            if loop_report.waiting_for_cores()
                || self.state().reported_loops.contains(&loop_report.name)
            {
                continue;
            }
            let mut report = report(
                ctx,
                &self.host,
                format!("loop:{}", loop_report.name),
                format!(
                    "Ignite's {} loop is stuck on {}",
                    loop_report.name, self.host
                ),
                format!(
                    "{problem}.\n\nIgnite {} on {}. `ignite status` shows every loop; the daemon \
                     log has each failed round.",
                    self.version, self.host
                ),
            );
            report.on_machine = true;
            report.evidence = format!("{}:{}", self.version, loop_report.registered_at);
            match ctx.hub.justiciar(&report) {
                Ok(_) => {
                    self.state().reported_loops.insert(loop_report.name.clone());
                }
                Err(error) => eprintln!("ignite: stuck loop not reported: {error}"),
            }
        }
    }

    fn send_local_request(&self, ctx: &Context) {
        let path = self.home.join(LOCAL_REQUEST);
        let Some(request) = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<LocalRequest>(&bytes).ok())
        else {
            return;
        };
        let mut report = report(
            ctx,
            &self.host,
            request.subject,
            format!("Justiciar asked for: {}", first_line(&request.reason)),
            format!(
                "Asked at {} while the hub could not be reached:\n\n{}",
                request.at, request.reason
            ),
        );
        report.on_machine = request.on_machine;
        report.evidence = request.at.to_string();
        if ctx.hub.justiciar(&report).is_ok() {
            let _ = std::fs::remove_file(&path);
        }
    }
}

/// `ignite justiciar [--here] "<reason>"`: asks for a justiciar for an emergency that stops
/// work and that no ordinary ticket will fix.
pub struct JusticiarVerb {
    pub home: PathBuf,
    pub host: String,
    pub dispatcher: Arc<Dispatcher>,
}

impl Verb for JusticiarVerb {
    fn name(&self) -> &'static str {
        "justiciar"
    }

    fn needs_the_tree_lease(&self) -> bool {
        false
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let on_machine = req.args.iter().any(|a| a == "--here");
        let reason = req
            .args
            .iter()
            .filter(|a| *a != "--here")
            .cloned()
            .collect::<Vec<_>>()
            .join(" ");
        let reason = reason.trim();
        if reason.is_empty() {
            return Reply::text(
                Exit::Usage,
                "usage    ignite justiciar [--here] \"<what is broken, and the evidence>\"",
            );
        }
        let asker = ctx
            .session
            .as_ref()
            .or(req.session.as_ref())
            .map_or_else(|| self.host.clone(), ToString::to_string);
        let subject = format!("request:{asker}");
        let mut report = report(
            ctx,
            &self.host,
            subject.clone(),
            format!("Justiciar asked for: {}", first_line(reason)),
            format!("Asked by {asker}:\n\n{reason}"),
        );
        report.on_machine = on_machine;
        report.evidence = ctx.clock.now().to_string();
        match ctx.hub.justiciar(&report) {
            Ok(filed) => Reply::text(
                Exit::Success,
                format!(
                    "filed    {} (justiciar, P0), offered before every other ticket\nnext     do \
                     what you can without it, or release your ticket with a note; never wait",
                    filed.ticket
                ),
            ),
            Err(Error::Unreachable(error)) => {
                let request = LocalRequest {
                    at: ctx.clock.now(),
                    subject,
                    reason: reason.to_owned(),
                    on_machine,
                };
                let _ = std::fs::write(
                    self.home.join(LOCAL_REQUEST),
                    serde_json::to_vec(&request).unwrap_or_default(),
                );
                let prompt = format!(
                    "You are this machine's justiciar. The hub cannot be reached ({error}), and \
                     a session asked for a justiciar: {reason}\n\nRead `ignite guide justiciar` \
                     and end the emergency."
                );
                let given = self.dispatcher.hand_off_justiciar(ctx, &prompt);
                Reply::text(
                    Exit::Unreachable,
                    format!(
                        "failed   the hub cannot be reached: {error}\nkept     the request, sent \
                         when the hub answers{}\nnext     do what you can without it, or release \
                         your ticket with a note; never wait",
                        given.map_or_else(String::new, |w| format!("; {w} is the justiciar now"))
                    ),
                )
            }
            Err(error) => Reply::text(Exit::Usage, format!("failed   {error}")),
        }
    }
}
