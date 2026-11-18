//! The pool: the machine's worker and dispatcher sessions, kept in the local database so a
//! restarted daemon keeps them, and the `ignite pool` verb that lists, adds, removes, pauses and
//! resumes them.
//!
//! A member's stored state is only `idle` or `down`. Whether a worker is reserved, working or
//! stalled is worked out by the [`Dispatcher`] from what it handed off and from which session
//! holds a build tree, so it can never disagree with the trees.

use std::fmt::Write as _;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use ignite_core::ids::{MachineId, SessionId};
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
}

impl Pool {
    pub fn new(conn: Connection) -> Pool {
        Pool {
            conn: Mutex::new(conn),
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

    pub fn paused(&self) -> Result<bool> {
        store::paused(&self.conn()).map_err(|e| store_error(&e))
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        store::set_paused(&self.conn(), paused).map_err(|e| store_error(&e))
    }
}

/// `ignite pool list | add | remove | pause | resume`.
pub struct PoolVerb {
    dispatcher: Arc<Dispatcher>,
}

impl PoolVerb {
    pub fn new(dispatcher: Arc<Dispatcher>) -> PoolVerb {
        PoolVerb { dispatcher }
    }
}

impl Verb for PoolVerb {
    fn name(&self) -> &'static str {
        "pool"
    }

    fn run(&self, ctx: &Context, req: &Request) -> Reply {
        let words: Vec<&str> = req.args.iter().map(String::as_str).collect();
        let result = match words.split_first() {
            Some((&"list", _)) | None => Ok(self.list()),
            Some((&"add", rest)) => self.add(ctx, req, rest),
            Some((&"remove", [session])) => self.remove(session),
            Some((&"pause", rest)) => self.pause(ctx, rest.contains(&"--all"), true),
            Some((&"resume", rest)) => self.pause(ctx, rest.contains(&"--all"), false),
            Some(_) => Err(Error::Invalid(
                "usage: ignite pool list | add <session>... --app <app> | remove <session> \
                 | pause [--all] | resume [--all]"
                    .to_owned(),
            )),
        };
        result.unwrap_or_else(|error| Reply::text(hub_exit(&error), error.to_string()))
    }
}

impl PoolVerb {
    fn list(&self) -> Reply {
        let view = match self.dispatcher.view() {
            Ok(view) => view,
            Err(error) => return Reply::text(hub_exit(&error), error.to_string()),
        };
        let mut lines = Vec::new();
        if self.dispatcher.pool().paused().unwrap_or(false) {
            lines.push("paused   no new hand-offs until `ignite pool resume`".to_owned());
        }
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
        if sessions.is_empty() {
            return Err(Error::Invalid("name at least one session".to_owned()));
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
            })?;
        }
        let names: Vec<&str> = sessions.iter().map(SessionId::as_str).collect();
        let what = if dispatcher { "dispatcher" } else { "workers" };
        let mut text = format!("added    {} as {what} ({app})", names.join(", "));
        if let Some(parent) = &parent {
            let _ = write!(text, ", parent {parent}");
        }
        Ok(Reply::text(Exit::Success, text))
    }

    fn remove(&self, session: &str) -> Result<Reply> {
        let session =
            SessionId::new(session).map_err(|e| Error::Invalid(format!("not a session: {e}")))?;
        self.dispatcher.pool().remove(&session)?;
        self.dispatcher.forget(&session);
        Ok(Reply::text(Exit::Success, format!("removed  {session}")))
    }

    /// Pauses or resumes this machine (kept locally and told to the hub) or, with `all`, every
    /// machine (kept by the hub, whose `/next` then answers nothing).
    fn pause(&self, ctx: &Context, all: bool, paused: bool) -> Result<Reply> {
        let verb = if paused { "paused" } else { "resumed" };
        if all {
            ctx.hub.pause(&Pause {
                machine: None,
                paused,
            })?;
            return Ok(Reply::text(
                Exit::Success,
                format!("{verb}   hand-offs on every machine"),
            ));
        }
        self.dispatcher.pool().set_paused(paused)?;
        let mut text = format!("{verb}   hand-offs on this machine; running workers finish");
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
