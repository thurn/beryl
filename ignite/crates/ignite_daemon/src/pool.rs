//! The pool: the machine's worker and dispatcher sessions, kept in the local database so a
//! restarted daemon keeps them, and the `ignite pool` verb that lists, adds, removes, pauses and
//! resumes them, and switches an app between its desktop dispatcher and CLI mode
//! (`ignite pool mode`, see `dispatchd_cli`).
//!
//! A member's stored state is only `idle` or `down`. Whether a worker is reserved, working or
//! stalled is worked out by the [`Dispatcher`] from what it handed off and from which session
//! holds a build tree, so it can never disagree with the trees.

use std::fmt::Write as _;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignite_core::config::PoolMode;
use ignite_core::ids::{MachineId, SessionId};
use ignite_core::protocol::cli::PoolModeSetting;
use ignite_core::protocol::client::{Exit, Reply, Request};
use ignite_core::protocol::hub::{Pause, WorkerRole, WorkerState};
use ignite_core::time::Timestamp;
use ignite_core::traits::{Error, Result};
use ignite_store::local::{pool as store, rows};
use rusqlite::Connection;

use crate::dispatchd::{Dispatcher, hub_exit};
use crate::server::{Context, Verb};

/// The model tiers a Claude worker runs, matched against a ticket's `tier:` label.
pub const CLAUDE_MODELS: [&str; 3] = ["opus", "sonnet", "light"];

/// One pool member as the daemon uses it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Member {
    pub session: SessionId,
    /// `claude` or `codex`.
    pub app: String,
    pub role: WorkerRole,
    /// The app confirmed the session is gone.
    pub down: bool,
    /// The model tiers its app can run.
    pub models: Vec<String>,
    /// The dispatcher session that added it.
    pub parent: Option<SessionId>,
    /// The files its last ticket touched.
    pub files: Vec<String>,
    pub added_at: Timestamp,
    /// For a Codex dispatcher, how many tickets its chats may run at once (see `dispatchd_slots`);
    /// 0 for every other member.
    pub slots: u32,
    /// A Codex chat opened for one ticket. It is never handed a second ticket, and it leaves the
    /// pool when its ticket ends.
    pub single_use: bool,
}

impl Member {
    fn from_row(row: &rows::PoolMember) -> Result<Member> {
        let invalid = |what: &str| Error::Invalid(format!("pool row {}: {what}", row.session));
        Ok(Member {
            session: SessionId::new(row.session.clone()).map_err(|_| invalid("session"))?,
            app: row.app.clone(),
            role: if row.role == "dispatcher" {
                WorkerRole::Dispatcher
            } else {
                WorkerRole::Worker
            },
            down: row.state == "down",
            models: serde_json::from_str(&row.models).map_err(|_| invalid("models"))?,
            parent: row.parent.as_deref().and_then(|p| SessionId::new(p).ok()),
            files: serde_json::from_str(&row.files).map_err(|_| invalid("files"))?,
            added_at: Timestamp::parse(&row.added_at).map_err(|_| invalid("added_at"))?,
            slots: u32::try_from(row.slots).map_err(|_| invalid("slots"))?,
            single_use: row.single_use != 0,
        })
    }

    fn to_row(&self) -> rows::PoolMember {
        rows::PoolMember {
            session: self.session.as_str().to_owned(),
            app: self.app.clone(),
            role: match self.role {
                WorkerRole::Worker => "worker",
                WorkerRole::Dispatcher => "dispatcher",
            }
            .to_owned(),
            state: if self.down { "down" } else { "idle" }.to_owned(),
            ticket: None,
            models: json_list(&self.models),
            added_at: self.added_at.to_string(),
            parent: self.parent.as_ref().map(|p| p.as_str().to_owned()),
            files: json_list(&self.files),
            slots: i64::from(self.slots),
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
    /// The hub's last heartbeat reply said every machine is paused (`ignite pool pause --all`).
    paused_everywhere: AtomicBool,
}

impl Pool {
    pub fn new(conn: Connection) -> Pool {
        Pool {
            conn: Mutex::new(conn),
            paused_everywhere: AtomicBool::new(false),
        }
    }

    fn conn(&self) -> MutexGuard<'_, Connection> {
        self.conn.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Every member, ordered by session.
    pub fn members(&self) -> Result<Vec<Member>> {
        let rows = store::all(&self.conn()).map_err(|e| store_error(&e))?;
        rows.iter().map(Member::from_row).collect()
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

    /// The dispatcher's saved queue, in the order it was written.
    pub fn load_queue(&self) -> Result<Vec<rows::DispatchEntry>> {
        ignite_store::local::dispatch_queue::all(&self.conn()).map_err(|e| store_error(&e))
    }

    /// Runs `f` on the local database, for the samples of the plan's usage (see
    /// `dispatchd_usage`).
    pub(crate) fn with_database<T>(&self, f: impl FnOnce(&mut Connection) -> T) -> T {
        f(&mut self.conn())
    }

    /// Replaces the dispatcher's saved queue with `entries`.
    pub fn save_queue(&self, entries: &[rows::DispatchEntry]) -> Result<()> {
        ignite_store::local::dispatch_queue::replace_all(&mut self.conn(), entries)
            .map_err(|e| store_error(&e))
    }

    pub fn paused(&self) -> Result<bool> {
        store::paused(&self.conn()).map_err(|e| store_error(&e))
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        store::set_paused(&self.conn(), paused).map_err(|e| store_error(&e))
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
    dispatcher: Arc<Dispatcher>,
    /// The emergency this machine enforces, which resuming every machine ends here at once
    /// rather than at the next heartbeat.
    emergency: Option<Arc<crate::emergency::Emergencies>>,
}

impl PoolVerb {
    pub fn new(dispatcher: Arc<Dispatcher>) -> PoolVerb {
        PoolVerb {
            dispatcher,
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
            Some((&"add", rest)) => self.add(ctx, req, rest),
            Some((&"remove", [session])) => self.remove(session),
            Some((&"pause", rest)) => self.pause(ctx, rest.contains(&"--all"), true),
            Some((&"resume", rest)) => self.pause(ctx, rest.contains(&"--all"), false),
            Some((&"mode", [app, setting])) => self.mode(app, setting),
            Some((&"watch", rest)) => self.watch(rest),
            Some((&"usage", [])) => Ok(crate::dispatchd::usage_report(
                self.dispatcher.pool(),
                ctx.clock.now(),
                ctx.config.pool.usage_hold_percent,
            )),
            Some(_) => Err(Error::Invalid(
                "usage: ignite pool list | add <session>... --app <app> [--dispatcher [--slots <n>]] \
                 | remove <session> \
                 | pause [--all] | resume [--all] | usage | mode <app> cli|desktop|config \
                 | watch <ticket | session> [--after <n>]"
                    .to_owned(),
            )),
        };
        result.unwrap_or_else(|error| Reply::text(hub_exit(&error), error.to_string()))
    }
}

impl PoolVerb {
    fn list(&self, ctx: &Context) -> Reply {
        let view = match self.dispatcher.view() {
            Ok(view) => view,
            Err(error) => return Reply::text(hub_exit(&error), error.to_string()),
        };
        let mut lines = Vec::new();
        let pool = self.dispatcher.pool();
        if pool.paused_everywhere() {
            lines.push(
                "paused   every machine: no hand-offs, not even of tickets that hold master back, \
                 until `ignite pool resume --all`"
                    .to_owned(),
            );
        } else if pool.paused().unwrap_or(false) {
            lines.push(
                "paused   only tickets that hold master back are handed off, one at a time, \
                 until `ignite pool resume`"
                    .to_owned(),
            );
        }
        lines.extend(
            crate::dispatchd::usage_holds(
                pool,
                ctx.config.pool.usage_hold_percent,
                ctx.clock.now(),
            )
            .iter()
            .map(crate::dispatchd::hold_line),
        );
        if let Some(shared) = self.dispatcher.shared_failure() {
            lines.push(format!("shared   {shared}"));
        }
        for app in crate::dispatchd::APPS {
            let (mode, overridden) = self.dispatcher.mode(app);
            if mode == PoolMode::Cli || overridden {
                let forced = if overridden {
                    " (set by `ignite pool mode`)"
                } else {
                    ""
                };
                lines.push(format!("mode     {app} {mode}{forced}"));
            }
        }
        lines.extend(self.dispatcher.cli_lines(ctx.clock.now()));
        if self.dispatcher.is_down() {
            lines.push("down     the dispatcher left three instructions unacknowledged".to_owned());
        }
        for worker in &view {
            lines.push(format_worker(worker));
        }
        if view.is_empty() {
            lines.push("the pool is empty: `ignite pool add <session>... --app claude`".to_owned());
        }
        Reply {
            exit: Exit::Success,
            text: lines.join("\n"),
            json: serde_json::to_value(&view).ok(),
            handle: None,
        }
    }

    fn add(&self, ctx: &Context, req: &Request, words: &[&str]) -> Result<Reply> {
        let mut sessions = Vec::new();
        let mut app = None;
        let mut models = None;
        let mut dispatcher = false;
        let mut slots = None;
        let mut rest = words.iter();
        while let Some(word) = rest.next() {
            match *word {
                "--app" => app = rest.next().map(|a| (*a).to_owned()),
                "--models" => {
                    models = rest
                        .next()
                        .map(|m| m.split(',').map(str::to_owned).collect::<Vec<_>>());
                }
                "--dispatcher" => dispatcher = true,
                "--slots" => slots = Some(slot_count(rest.next().copied())?),
                session => sessions.push(
                    SessionId::new(session)
                        .map_err(|e| Error::Invalid(format!("not a session: {e}")))?,
                ),
            }
        }
        let app = app.ok_or_else(|| Error::Invalid("--app claude or --app codex".to_owned()))?;
        if app != "claude" && app != "codex" {
            return Err(Error::Invalid(format!("unknown app {app}")));
        }
        if sessions.is_empty()
            && dispatcher
            && let Some(caller) = &req.session
        {
            sessions.push(caller.clone());
        }
        if sessions.is_empty() {
            return Err(Error::Invalid("name at least one session".to_owned()));
        }
        if dispatcher && sessions.len() > 1 {
            return Err(Error::Invalid("name one dispatcher".to_owned()));
        }
        if slots.is_some() && !(dispatcher && app == "codex") {
            return Err(Error::Invalid(
                "--slots is only for a Codex dispatcher: --app codex --dispatcher".to_owned(),
            ));
        }
        let models = models.unwrap_or_else(|| match app.as_str() {
            "claude" => CLAUDE_MODELS.iter().map(|m| (*m).to_owned()).collect(),
            _ => Vec::new(),
        });
        let role = if dispatcher {
            WorkerRole::Dispatcher
        } else {
            WorkerRole::Worker
        };
        // `ignite pool add` runs in the dispatcher's own session, which becomes the parent of
        // the workers it adds.
        let parent = if dispatcher {
            None
        } else {
            req.session.clone()
        };
        let pool = self.dispatcher.pool();
        let slots = if dispatcher && app == "codex" {
            self.codex_slots(&sessions[0], slots.unwrap_or(0))?
        } else {
            0
        };
        let replaced = if dispatcher {
            self.replace_dispatcher(&app, &sessions[0])?
        } else {
            Vec::new()
        };
        for session in &sessions {
            pool.put(&Member {
                session: session.clone(),
                app: app.clone(),
                role,
                down: false,
                models: models.clone(),
                parent: parent.clone(),
                files: Vec::new(),
                added_at: ctx.clock.now(),
                slots,
                single_use: false,
            })?;
        }
        // A dispatcher starts its workers between registering itself and registering them.
        self.dispatcher.pool_setup(dispatcher, ctx.clock.now());
        let names: Vec<&str> = sessions.iter().map(SessionId::as_str).collect();
        let what = if dispatcher { "dispatcher" } else { "workers" };
        let mut text = format!("added    {} as {what} ({app})", names.join(", "));
        if let Some(parent) = &parent {
            let _ = write!(text, ", parent {parent}");
        }
        if dispatcher && app == "codex" {
            let _ = write!(text, ", {slots} slots");
        }
        if !replaced.is_empty() {
            let names: Vec<&str> = replaced.iter().map(SessionId::as_str).collect();
            let _ = write!(text, "\nremoved  {}", names.join(", "));
        }
        Ok(Reply::text(Exit::Success, text))
    }

    /// The slot count a Codex dispatcher registers with, given the `--slots` it `asked` for.
    ///
    /// A count above 0 is the new count. 0 keeps the count the machine's Codex dispatcher already
    /// had: a Codex dispatcher whose context has grown opens a new chat whose prompt is
    /// `/ignite pool 0`, and that new chat takes over the old one's slots. Its chats still
    /// holding tickets are moved to the new dispatcher, so the replacement below keeps them.
    /// A new dispatcher with a count of its own starts afresh, and the old one's chats go with it.
    fn codex_slots(&self, new: &SessionId, asked: u32) -> Result<u32> {
        let pool = self.dispatcher.pool();
        let members = pool.members()?;
        let Some(old) = members
            .iter()
            .filter(|m| m.role == WorkerRole::Dispatcher && m.app == "codex")
            .max_by_key(|m| &m.session == new)
        else {
            return Ok(asked);
        };
        if asked > 0 {
            return Ok(asked);
        }
        if &old.session != new {
            for chat in members
                .iter()
                .filter(|m| m.parent.as_ref() == Some(&old.session))
            {
                pool.update(&chat.session, |m| m.parent = Some(new.clone()))?;
            }
        }
        Ok(old.slots)
    }

    /// Removes every other dispatcher of `app` on this machine and the workers each one started,
    /// since only the session that started a worker can clear and prompt it. Returns the removed
    /// sessions. Registering the same dispatcher again removes nothing.
    fn replace_dispatcher(&self, app: &str, new: &SessionId) -> Result<Vec<SessionId>> {
        let pool = self.dispatcher.pool();
        let members = pool.members()?;
        let old: Vec<&SessionId> = members
            .iter()
            .filter(|m| m.role == WorkerRole::Dispatcher && m.app == app && &m.session != new)
            .map(|m| &m.session)
            .collect();
        let mut removed = Vec::new();
        for member in &members {
            let orphan = member.parent.as_ref().is_some_and(|p| old.contains(&p));
            if old.contains(&&member.session) || orphan {
                pool.remove(&member.session)?;
                self.dispatcher.forget(&member.session);
                self.dispatcher.check_holder(&member.session);
                removed.push(member.session.clone());
            }
        }
        Ok(removed)
    }

    /// `ignite pool mode <app> cli|desktop|config`. The mode is read on every step, so the
    /// switch needs no restart. A ticket in progress is never touched: workers of the old mode
    /// finish it, and only new hand-offs follow the new mode.
    fn mode(&self, app: &str, setting: &str) -> Result<Reply> {
        let setting = PoolModeSetting::parse(setting).ok_or_else(|| {
            Error::Invalid(format!(
                "not a mode: {setting:?}; use cli, desktop or config"
            ))
        })?;
        let reply = self.dispatcher.set_mode(app, setting)?;
        let source = if reply.overridden {
            "until `ignite pool mode <app> config`"
        } else {
            "as ignite.toml says"
        };
        let mut text = format!(
            "mode     {} runs in {} mode on this machine, {source}; tickets in progress finish as \
             they started",
            reply.app, reply.mode
        );
        if setting == PoolModeSetting::Cli
            && reply.mode != PoolMode::Cli
            && let Some(why) = self.dispatcher.cli_unavailable(app)
        {
            let _ = write!(text, "\nnot cli  {why}");
        }
        Ok(Reply {
            exit: Exit::Success,
            text,
            json: serde_json::to_value(&reply).ok(),
            handle: None,
        })
    }

    /// `ignite pool watch <ticket | session> [--after <n>]`: the digest lines of a CLI
    /// session's transcript after the first `n`, and in the JSON the number of lines there are
    /// now, which the client sends as `--after` to get only what is new. The client does the
    /// following, so the daemon answers at once.
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
        let Some((session, lines)) = self.dispatcher.cli_digest(target)? else {
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

    fn remove(&self, session: &str) -> Result<Reply> {
        let session =
            SessionId::new(session).map_err(|e| Error::Invalid(format!("not a session: {e}")))?;
        self.dispatcher.pool().remove(&session)?;
        self.dispatcher.forget(&session);
        Ok(Reply::text(Exit::Success, format!("removed  {session}")))
    }

    /// Pauses or resumes this machine (kept locally and told to the hub) or, with `all`, every
    /// machine (kept by the hub, whose `/next` then answers nothing). Resuming every machine also
    /// ends the emergency, if one is in force.
    fn pause(&self, ctx: &Context, all: bool, paused: bool) -> Result<Reply> {
        let verb = if paused { "paused" } else { "resumed" };
        if all {
            ctx.hub.pause(&Pause {
                machine: None,
                paused,
            })?;
            // Known here at once rather than at the next heartbeat.
            self.dispatcher.pool().set_paused_everywhere(paused);
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
        self.dispatcher.pool().set_paused(paused)?;
        let mut text = format!(
            "{verb}   hand-offs on this machine; running workers finish, and tickets that hold \
             master back still go to one worker at a time (`--all` stops those too)"
        );
        if !paused {
            text = format!("{verb}  hand-offs on this machine");
        }
        if let Err(error) = ctx.hub.pause(&Pause {
            machine: Some(MachineId::clone(&ctx.machine)),
            paused,
        }) {
            let _ = write!(
                text,
                "\nhub      not told ({error}); this machine is {verb}"
            );
        }
        Ok(Reply::text(Exit::Success, text))
    }
}

/// The labels of the tickets that hold `master` back: a regression (a check that tip
/// verification found failing) and a justiciar ticket (a fault in Ignite itself).
const HOLDING_MASTER: [&str; 2] = ["regression", ignite_core::dispatch::JUSTICIAR];

/// `ignite status`'s `pool` lines, shown only while hand-offs are paused or on a usage hold: how
/// far the pause reaches, which window of the plan holds, and the tickets that hold `master`
/// back, which a person who paused the pool may not know are waiting.
pub struct PauseSection {
    pub dispatcher: Arc<Dispatcher>,
    pub hub: Arc<dyn ignite_core::traits::Hub>,
    pub clock: Arc<dyn ignite_core::traits::Clock>,
    /// `[pool] usage_hold_percent` from `ignite.toml`.
    pub usage_hold_percent: u32,
}

impl crate::doctor::StatusSection for PauseSection {
    fn title(&self) -> &'static str {
        "pool"
    }

    fn lines(&self) -> Vec<String> {
        let pool = self.dispatcher.pool();
        let holds: Vec<String> =
            crate::dispatchd::usage_holds(pool, self.usage_hold_percent, self.clock.now())
                .iter()
                .map(crate::dispatchd::hold_line)
                .collect();
        let pause = if pool.paused_everywhere() {
            Some(
                "paused on every machine: nothing is handed off, not even the tickets that hold \
                 master back",
            )
        } else if pool.paused().unwrap_or(false) {
            Some(
                "paused on this machine: only tickets that hold master back are handed off, one \
                 at a time",
            )
        } else {
            None
        };
        let shared = self.dispatcher.shared_failure();
        if pause.is_none() && holds.is_empty() && shared.is_none() {
            // Whether more workers would help: only when the reason is about workers, never
            // when it is about tickets or cores.
            return self
                .dispatcher
                .idle_reason()
                .map(|(idle, why)| {
                    let workers = if idle == 1 { "worker" } else { "workers" };
                    vec![format!("{idle} idle {workers}: {why}")]
                })
                .unwrap_or_default();
        }
        let mut lines: Vec<String> = pause.map(str::to_owned).into_iter().collect();
        lines.extend(shared);
        lines.extend(holds);
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
        lines
    }
}

fn format_worker(worker: &WorkerState) -> String {
    let role = match worker.role {
        WorkerRole::Worker => "worker",
        WorkerRole::Dispatcher => "dispatcher",
    };
    let ticket = worker.ticket.as_ref().map_or("-", |t| t.as_str());
    let mut line = format!(
        "{:<16} {:<6} {:<10} {:<8} {ticket}",
        worker.session.as_str(),
        worker.app,
        role,
        worker.state
    );
    if let (Some(command), Some(since)) = (&worker.command, worker.command_since) {
        let _ = write!(line, "  `{command}` since {since}");
    }
    line
}

/// The value of `--slots`: a count of 0 or more.
fn slot_count(word: Option<&str>) -> Result<u32> {
    let word = word.unwrap_or_default();
    word.parse::<u32>()
        .map_err(|_| Error::Invalid(format!("not a slot count: {word:?}")))
}
