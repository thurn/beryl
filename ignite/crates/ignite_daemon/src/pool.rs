//! The machine’s CLI workers and the commands that show or pause their work.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignite_core::fleet::MachineState;
use ignite_core::ids::SessionId;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::{Pause, WorkerState};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::{pool as store, rows};
use rusqlite::Connection;

use crate::dispatchd::{HeadlessDispatch, hub_exit};
use crate::server::{Context, Verb};

/// One pool member as the daemon uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub session: SessionId,
    /// The provider name from `[pool.cli.<provider>]`.
    pub app: String,
    /// The app confirmed the session is gone.
    pub down: bool,
    /// The model tiers its app can run.
    pub models: Vec<String>,
    /// The files its last ticket touched.
    pub files: Vec<String>,
    pub added_at: Timestamp,
    /// A worker started for one ticket. It leaves the pool when that ticket ends.
    pub single_use: bool,
}

impl Member {
    fn from_row(row: &rows::PoolMember) -> Result<Member> {
        let invalid = |what: &str| Error::Invalid(format!("pool row {}: {what}", row.session));
        Ok(Member {
            session: SessionId::new(row.session.clone()).map_err(|_| invalid("session"))?,
            app: row.app.clone(),
            down: row.state == "down",
            models: serde_json::from_str(&row.models).map_err(|_| invalid("models"))?,
            files: serde_json::from_str(&row.files).map_err(|_| invalid("files"))?,
            added_at: Timestamp::parse(&row.added_at).map_err(|_| invalid("added_at"))?,
            single_use: row.single_use != 0,
        })
    }

    fn to_row(&self) -> rows::PoolMember {
        rows::PoolMember {
            session: self.session.as_str().to_owned(),
            app: self.app.clone(),
            role: "worker".to_owned(),
            state: if self.down { "down" } else { "idle" }.to_owned(),
            ticket: None,
            models: json_list(&self.models),
            added_at: self.added_at.to_string(),
            parent: None,
            files: json_list(&self.files),
            slots: 0,
            single_use: i64::from(self.single_use),
        }
    }
}

fn json_list(items: &[String]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".to_owned())
}

fn store_error(error: &ignite_store::StoreError) -> Error {
    Error::Io(error.to_string())
}

/// The pool's members and pause, in the daemon's local database.
pub struct Pool {
    conn: Mutex<Connection>,
    /// Orders new hand-offs with providers entering a CLI hold.
    pub(crate) cli_admission: Mutex<()>,
    /// The hub's last heartbeat reply said every machine is paused (`ignite pool pause --all`).
    paused_everywhere: AtomicBool,
}

impl Pool {
    pub fn new(conn: Connection) -> Pool {
        Pool {
            conn: Mutex::new(conn),
            cli_admission: Mutex::new(()),
            paused_everywhere: AtomicBool::new(false),
        }
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every member, ordered by session.
    pub fn members(&self) -> Result<Vec<Member>> {
        let rows = store::all(&self.conn()).map_err(|e| store_error(&e))?;
        rows.iter()
            .filter(|row| row.role == "worker")
            .map(Member::from_row)
            .collect()
    }

    pub fn member(&self, session: &SessionId) -> Result<Option<Member>> {
        Ok(self.members()?.into_iter().find(|m| &m.session == session))
    }

    /// Adds the member, or replaces the one with its session.
    pub fn put(&self, member: &Member) -> Result<()> {
        store::upsert(&self.conn(), &member.to_row()).map_err(|e| store_error(&e))
    }

    pub fn remove(&self, session: &SessionId) -> Result<()> {
        store::remove(&self.conn(), session.as_str()).map_err(|e| store_error(&e))
    }

    /// Changes one member, if it is in the pool.
    pub fn update(&self, session: &SessionId, change: impl FnOnce(&mut Member)) -> Result<()> {
        if let Some(mut member) = self.member(session)? {
            change(&mut member);
            self.put(&member)?;
        }
        Ok(())
    }

    /// The saved dispatch queue, in the order it was written.
    pub(crate) fn with_database<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> T {
        f(&mut self.conn())
    }

    pub fn paused(&self) -> Result<bool> {
        store::paused(&self.conn()).map_err(|e| store_error(&e))
    }

    /// Whether the hub last said every machine is paused.
    pub fn paused_everywhere(&self) -> bool {
        self.paused_everywhere.load(Ordering::Relaxed)
    }

    /// Records what the hub's heartbeat reply said about a pause of every machine.
    pub fn set_paused_everywhere(&self, paused: bool) {
        self.paused_everywhere.store(paused, Ordering::Relaxed);
    }
}

/// `ignite pool list | add | remove | pause | resume`.
pub struct PoolVerb {
    headless_dispatch: Arc<HeadlessDispatch>,
    /// The emergency this machine enforces, which resuming every machine ends here at once
    /// rather than at the next heartbeat.
    emergency: Option<Arc<crate::emergency::Emergencies>>,
}

impl PoolVerb {
    pub fn new(headless_dispatch: Arc<HeadlessDispatch>) -> PoolVerb {
        PoolVerb {
            headless_dispatch,
            emergency: None,
        }
    }

    /// The verb, ending `emergencies`' emergency when it resumes every machine.
    #[must_use]
    pub fn ending(self, emergencies: Arc<crate::emergency::Emergencies>) -> PoolVerb {
        PoolVerb {
            emergency: Some(emergencies),
            ..self
        }
    }
}

impl Verb for PoolVerb {
    fn name(&self) -> &'static str {
        "pool"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let words: Vec<&str> = req.args.iter().map(String::as_str).collect();
        let result = match words.split_first() {
            Some((&"list", _)) | None => Ok(self.list(ctx)),
            Some((&("add" | "remove" | "mode"), _)) => Err(Error::Invalid(
                "desktop worker commands have been retired".to_owned(),
            )),
            Some((&"pause", rest)) => self.pause(ctx, req, rest, true),
            Some((&"resume", rest)) => self.pause(ctx, req, rest, false),
            Some((&"watch", rest)) => self.watch(rest),
            Some((&"shed", rest)) => self.shed(ctx, req, rest),
            Some((&"usage", [])) => Ok(crate::dispatchd::usage_report(
                self.headless_dispatch.pool(),
                ctx.clock.now(),
            )),
            Some(_) => Err(Error::Invalid(
                "usage: ignite pool list | pause [--all] | resume [--all] | usage \
                 | watch <ticket | session> [--after <n>] | shed [--machine <name>] [--to <n>]"
                    .to_owned(),
            )),
        };
        result.unwrap_or_else(|error| Reply::text(hub_exit(&error), error.to_string()))
    }
}

impl PoolVerb {
    fn list(&self, ctx: &Context) -> Reply {
        let view = match self.headless_dispatch.view() {
            Ok(view) => view,
            Err(error) => return Reply::text(hub_exit(&error), error.to_string()),
        };
        let mut lines = Vec::new();
        let pool = self.headless_dispatch.pool();
        let state = crate::fleet_notice::current(pool);
        if let Some(notice) = &state {
            lines.push(crate::fleet_notice::state_line(notice));
        }
        if pool.paused_everywhere() {
            lines.push(
                "paused   every machine: no hand-offs, not even of tickets that hold master back, \
                 until `ignite pool resume --all`"
                    .to_owned(),
            );
        } else if state.is_none() && pool.paused().unwrap_or(false) {
            lines.push(
                "paused   only tickets that hold master back are handed off, one at a time, \
                 until `ignite pool resume`"
                    .to_owned(),
            );
        }
        let holds = crate::dispatchd::usage_holds(pool, ctx.clock.now());
        lines.extend(crate::dispatchd::hold_lines(&holds));
        if let Some(shared) = self.headless_dispatch.shared_failure() {
            lines.push(format!("shared   {shared}"));
        }
        lines.extend(
            self.headless_dispatch
                .build_line()
                .map(|line| format!("builds   {line}")),
        );
        lines.extend(self.headless_dispatch.cli_lines(ctx.clock.now()));
        for worker in &view {
            let mut line = format_worker(worker);
            if let Some(note) = self.headless_dispatch.handoff_note(&worker.session) {
                let _ = write!(line, "  {note}");
            }
            lines.push(line);
        }
        if view.is_empty() {
            lines.push("the pool is empty".to_owned());
        }
        Reply {
            exit: Exit::Success,
            text: lines.join("\n"),
            json: serde_json::to_value(&view).ok(),
            handle: None,
        }
    }

    fn watch(&self, words: &[&str]) -> Result<Reply> {
        let mut target = None;
        let mut after = 0usize;
        let mut rest = words.iter();
        while let Some(word) = rest.next() {
            match *word {
                "--after" => {
                    let number = rest.next().copied().unwrap_or_default();
                    after = number
                        .parse()
                        .map_err(|_| Error::Invalid(format!("not a line count: {number:?}")))?;
                }
                "--no-follow" => {}
                other => target = Some(other),
            }
        }
        let target = target
            .ok_or_else(|| Error::Invalid("usage: ignite pool watch <ticket | session>".into()))?;
        let Some((session, lines)) = self.headless_dispatch.cli_digest(target)? else {
            return Err(Error::NotFound(format!(
                "no CLI session works or worked {target}; `ignite pool list` shows the slots"
            )));
        };
        Ok(Reply {
            exit: Exit::Success,
            text: crate::dispatchd::digest_text(&lines, after),
            json: Some(serde_json::json!({"session": session, "next": lines.len()})),
            handle: None,
        })
    }

    /// `ignite pool shed [--machine <name>] [--to <n>]`: sheds this machine's build tickets
    /// down to `n` at once, or orders the named machine to at its next heartbeat. Only the
    /// operator, an incident commander or a justiciar may take work away from other sessions.
    fn shed(&self, ctx: &Context, req: &Request, words: &[&str]) -> Result<Reply> {
        let usage =
            || Error::Invalid("usage: ignite pool shed [--machine <name>] [--to <n>]".into());
        let (mut machine, mut to) = (None, None);
        let mut rest = words.iter();
        while let Some(word) = rest.next() {
            match *word {
                "--machine" => machine = Some((*rest.next().ok_or_else(usage)?).to_owned()),
                "--to" => {
                    let number = rest.next().ok_or_else(usage)?;
                    to = Some(number.parse::<u32>().map_err(|_| {
                        Error::Invalid(format!("not a number of build tickets: {number:?}"))
                    })?);
                }
                _ => return Err(usage()),
            }
        }
        if let Err(reason) = crate::stop::emergency_or_operator(ctx) {
            return Ok(Reply::text(
                Exit::Conflict,
                format!(
                    "refused  `ignite pool shed` is for incident commanders, justiciars and the \
                     operator: {reason}"
                ),
            ));
        }
        let by = ctx
            .session
            .clone()
            .or_else(|| req.session.clone())
            .map_or_else(|| "the operator".to_owned(), |session| session.to_string());
        if let Some(machine) = machine {
            let order = ignite_core::protocol::hub::shed::ShedOrder { machine, to, by };
            ctx.hub.shed(&order)?;
            let target = to.map_or_else(|| "its limit".to_owned(), |n| n.to_string());
            return Ok(Reply::text(
                Exit::Success,
                format!(
                    "ordered  {} to shed down to {target} build tickets at its next heartbeat \
                     with a sample, within about a minute\nnext     ignite fleet shows the shed \
                     in its recent changes",
                    order.machine
                ),
            ));
        }
        let shed = self.headless_dispatch.shed(ctx, to, &by)?;
        Ok(Reply::text(Exit::Success, shed.lines().join("\n")))
    }

    /// Changes this machine's state, or pauses or resumes all hand-offs. Resuming all
    /// hand-offs also ends the emergency, if one is in force.
    fn pause(&self, ctx: &Context, req: &Request, words: &[&str], paused: bool) -> Result<Reply> {
        let verb = if paused { "paused" } else { "resumed" };
        if words.contains(&"--all") {
            let emergency = crate::emergency::named_emergency(words).map_err(Error::Invalid)?;
            // The hub refuses to end an emergency that another session's ticket holds.
            ctx.hub.pause(&Pause {
                machine: None,
                paused,
                session: ctx.session.clone().or_else(|| req.session.clone()),
                emergency,
            })?;
            // Known here at once rather than at the next heartbeat.
            self.headless_dispatch.pool().set_paused_everywhere(paused);
            let mut text = format!("{verb}   hand-offs on every machine");
            if paused {
                text.push_str(", even of tickets that hold master back");
            }
            if !paused && let Some(emergencies) = &self.emergency {
                if let Some(ended) = emergencies.in_force() {
                    let _ = write!(text, "\nended    emergency {}", ended.ticket);
                }
                emergencies.heard(None);
            }
            return Ok(Reply::text(Exit::Success, text));
        }
        let state = if paused {
            MachineState::ChecksOnly
        } else {
            MachineState::Active
        };
        ctx.hub.set_fleet_machine_state(
            &ctx.machine,
            state,
            req.session.as_ref().map(SessionId::as_str),
        )?;
        Ok(Reply::text(
            Exit::Success,
            format!("set this machine to {state}"),
        ))
    }
}

/// The labels of the tickets that hold `master` back: a regression (a check that tip
/// verification found failing) and a justiciar ticket (a fault in Ignite itself).
const HOLDING_MASTER: [&str; 2] = ["regression", ignite_core::dispatch::JUSTICIAR];

/// The machine's fleet state, any hand-off pauses and usage holds, and the tickets holding
/// `master` back while work is paused.
pub struct PauseSection {
    pub headless_dispatch: Arc<HeadlessDispatch>,
    pub hub: Arc<dyn ignite_core::traits::Hub>,
    pub clock: Arc<dyn ignite_core::traits::Clock>,
}

impl crate::doctor::StatusSection for PauseSection {
    fn title(&self) -> &'static str {
        "pool"
    }

    fn lines(&self) -> Vec<String> {
        let pool = self.headless_dispatch.pool();
        let holds = crate::dispatchd::usage_holds(pool, self.clock.now());
        let holds: Vec<String> = crate::dispatchd::hold_lines(&holds).collect();
        let notice = crate::fleet_notice::current(pool);
        let state = notice.as_ref().map(crate::fleet_notice::state_line);
        let pause = if pool.paused_everywhere() {
            Some(
                "paused on every machine: nothing is handed off, not even the tickets that hold \
                 master back",
            )
        } else if notice.is_none() && pool.paused().unwrap_or(false) {
            Some(
                "paused on this machine: only tickets that hold master back are handed off, one \
                 at a time",
            )
        } else {
            None
        };
        let shared = self.headless_dispatch.shared_failure();
        let restricted = notice.is_some_and(|notice| notice.state != MachineState::Active);
        let mut lines: Vec<String> = pause.map(str::to_owned).into_iter().chain(state).collect();
        let ordinary = !restricted && pause.is_none() && holds.is_empty() && shared.is_none();
        lines.extend(shared);
        lines.extend(holds);
        lines.extend(self.headless_dispatch.cli_hold_lines(self.clock.now()));
        lines.extend(self.headless_dispatch.build_line());
        if ordinary {
            lines.extend(self.headless_dispatch.worker_room_line());
            if let Some((_, why)) = self.headless_dispatch.idle_reason() {
                lines.push(why);
            }
            return lines;
        }
        let query = ignite_core::protocol::tickets::TicketQuery {
            label_any: HOLDING_MASTER.iter().map(|l| (*l).to_owned()).collect(),
            ..Default::default()
        };
        match self.hub.tickets(&query) {
            Err(error) => lines.push(format!("the hub did not answer: {error}")),
            Ok(tickets) => {
                let holding: Vec<_> = tickets
                    .iter()
                    .filter(|t| t.status != ignite_core::protocol::tickets::Status::Closed)
                    .collect();
                if holding.is_empty() {
                    lines.push("nothing holds master back".to_owned());
                }
                for ticket in holding {
                    let working = match ticket.status {
                        ignite_core::protocol::tickets::Status::InProgress => ", being worked",
                        _ => "",
                    };
                    lines.push(format!(
                        "master held back by {}{working}: {}",
                        ticket.id, ticket.title
                    ));
                }
            }
        }
        lines.extend(self.headless_dispatch.worker_room_line());
        lines
    }
}

fn format_worker(worker: &WorkerState) -> String {
    let ticket = worker.ticket.as_ref().map_or("-", |t| t.as_str());
    let mut line = format!(
        "{:<16} {:<6} worker     {:<8} {ticket}",
        worker.session.as_str(),
        worker.app,
        worker.state
    );
    if let (Some(command), Some(since)) = (&worker.command, worker.command_since) {
        let _ = write!(line, "  `{command}` since {since}");
    }
    line
}
