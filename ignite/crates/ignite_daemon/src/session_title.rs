//! The title of a Claude desktop session, which the hub keeps so the dashboard can name sessions
//! by title instead of by id.
//!
//! No environment variable holds a session's title, only its id (`CLAUDE_CODE_HOST_SESSION_ID`,
//! a `local_<uuid>`). The desktop app keeps each session's metadata, title included, in a JSON
//! file named after that id:
//!
//! ```text
//! <app data>/Claude/claude-code-sessions/<account uuid>/<org uuid>/local_<uuid>.json
//! ```
//!
//! where `<app data>` is `~/Library/Application Support` on macOS and `%APPDATA%` on Windows.
//! The format belongs to the app and is undocumented, so every failure here means "no title", and
//! the caller goes on as it would without one.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use rusqlite::Connection;

use serde_json::Value;

use ignite_core::ids::SessionId;
use ignite_core::protocol::client::Request;
use ignite_core::protocol::hub::SessionTitle;
use ignite_store::local::rows;

use crate::server::{Context, Hook};

/// The directory holding the desktop app's data, from the environment.
pub fn app_data(env: &impl Fn(&str) -> Option<String>) -> Option<PathBuf> {
    if cfg!(windows) {
        env("APPDATA").map(|dir| PathBuf::from(dir).join("Claude"))
    } else {
        env("HOME").map(|home| {
            PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("Claude")
        })
    }
}

/// The title the desktop app shows for `session`, if `session` is a desktop session whose
/// metadata file under `app_data` names one. The account and organization directories come from
/// `CLAUDE_CODE_ACCOUNT_UUID` and `CLAUDE_CODE_ORGANIZATION_UUID` when set; otherwise every
/// pair of directories is searched.
pub fn title(
    app_data: &Path,
    session: &str,
    env: &impl Fn(&str) -> Option<String>,
) -> Option<String> {
    let id = session.strip_prefix("local_")?;
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_hexdigit() || c == '-') {
        return None;
    }
    let sessions = app_data.join("claude-code-sessions");
    let file = format!("{session}.json");
    let path = match (
        env("CLAUDE_CODE_ACCOUNT_UUID"),
        env("CLAUDE_CODE_ORGANIZATION_UUID"),
    ) {
        (Some(account), Some(org)) => Some(sessions.join(account).join(org).join(&file)),
        _ => None,
    }
    .filter(|path| path.is_file())
    .or_else(|| search(&sessions, &file))?;
    let metadata: Value = serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()?;
    let title = metadata.get("title")?.as_str()?.trim();
    (!title.is_empty()).then(|| title.to_owned())
}

/// `<sessions>/*/*/<file>`, if it exists.
fn search(sessions: &Path, file: &str) -> Option<PathBuf> {
    let children = |dir: &Path| {
        std::fs::read_dir(dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| path.is_dir())
            .collect::<Vec<_>>()
    };
    children(sessions)
        .iter()
        .flat_map(|account| children(account))
        .map(|org| org.join(file))
        .find(|path| path.is_file())
}

/// Reports to the hub the title of each session that runs a command from a build tree, whenever
/// it differs from the title last reported for that session. Workers set their title after
/// `ignite start`, and again when they finish, so a title read once at the claim would be stale.
///
/// A session renames itself without running a command: a reused worker claims its next ticket
/// under the old ticket's title, then sets the new one. So the sender loop also calls
/// [`TitleHook::report_holders`] after each heartbeat, and the dashboard never names a ticket's
/// assignee by the previous ticket's title for longer than one heartbeat.
pub struct TitleHook {
    app_data: Option<PathBuf>,
    reported: Mutex<HashMap<SessionId, String>>,
}

impl TitleHook {
    /// The hook, reading titles from the desktop app's data under `app_data`.
    pub fn new(app_data: Option<PathBuf>) -> TitleHook {
        TitleHook {
            app_data,
            reported: Mutex::new(HashMap::new()),
        }
    }

    /// Reports `session`'s title if it changed. Returns whether a report was sent.
    pub fn report(&self, ctx: &Context, session: &SessionId) -> bool {
        let Some(app_data) = &self.app_data else {
            return false;
        };
        let env = |name: &str| std::env::var(name).ok();
        let Some(title) = title(app_data, session.as_str(), &env) else {
            return false;
        };
        let mut reported = self.reported.lock().unwrap_or_else(PoisonError::into_inner);
        if reported.get(session) == Some(&title) {
            return false;
        }
        let sent = ctx.hub.session_title(&SessionTitle {
            machine: ctx.machine.clone(),
            session: session.clone(),
            title: title.clone(),
        });
        // A failed report is tried again at the session's next command.
        if sent.is_ok() {
            reported.insert(session.clone(), title);
        }
        sent.is_ok()
    }

    /// Reports the title of each session that holds a build tree in the local database `db`,
    /// where it changed. A tree the database cannot list is skipped until the next round.
    pub fn report_holders(&self, ctx: &Context, db: &Connection) {
        let Ok(trees) = ignite_store::local::trees::all(db) else {
            return;
        };
        let holders: BTreeSet<SessionId> = trees
            .iter()
            .filter_map(|tree| tree.session.as_deref())
            .filter_map(|session| SessionId::new(session).ok())
            .collect();
        for session in &holders {
            self.report(ctx, session);
        }
    }
}

impl Hook for Arc<TitleHook> {
    fn run(&self, ctx: &Context, req: &Request, tree: &rows::Tree) -> Vec<String> {
        self.as_ref().run(ctx, req, tree)
    }
}

impl Hook for TitleHook {
    fn run(&self, ctx: &Context, _req: &Request, _tree: &rows::Tree) -> Vec<String> {
        if let Some(session) = &ctx.session {
            self.report(ctx, session);
        }
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "local_8c29f0b2-fddb-4e96-9da4-b1872d57316a";

    fn app_data(name: &str, contents: Option<&str>) -> PathBuf {
        let dir = ignite_core::test_files::temp_dir().join(format!(
            "ignite-daemon-session-title-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        let org = dir.join("claude-code-sessions").join("acct").join("org");
        std::fs::create_dir_all(&org).unwrap();
        if let Some(contents) = contents {
            std::fs::write(org.join(format!("{SESSION}.json")), contents).unwrap();
        }
        dir
    }

    fn no_env(_: &str) -> Option<String> {
        None
    }

    #[test]
    fn reads_the_title_from_the_sessions_metadata_file() {
        let dir = app_data(
            "found",
            Some(r#"{"sessionId":"x","title":" Status check "}"#),
        );
        assert_eq!(
            title(&dir, SESSION, &no_env).as_deref(),
            Some("Status check")
        );
        let env = |name: &str| match name {
            "CLAUDE_CODE_ACCOUNT_UUID" => Some("acct".to_owned()),
            "CLAUDE_CODE_ORGANIZATION_UUID" => Some("org".to_owned()),
            _ => None,
        };
        assert_eq!(title(&dir, SESSION, &env).as_deref(), Some("Status check"));
    }

    fn context() -> (Context, Arc<ignite_core::fakes::FakeHub>) {
        use ignite_core::config::Config;
        use ignite_core::fakes::{FakeBuilds, FakeClock, FakeGit, FakeHub, FakeLauncher};
        use ignite_core::ids::MachineId;
        use ignite_core::time::Timestamp;

        let hub = Arc::new(FakeHub::default());
        let text = "[repository]\nremote = \"origin\"\nstaging = \"staging\"\n\
                    master = \"master\"\ntrees = \"trees\"\nchecks = \"checks\"\n\
                    [hub]\nurl = \"http://localhost:1\"\nproject = \"test\"\n\
                    [gate]\nthreshold = 0.01\ncap = 10\n\
                    [machine.local]\nticket_trees = 1\nlanding_trees = 1\n";
        let ctx = Context {
            config: Config::parse("ignite.toml", text).unwrap(),
            repo: PathBuf::from("/repo"),
            machine: MachineId::new("m-1").unwrap(),
            clock: Arc::new(FakeClock::new(Timestamp::EPOCH)),
            hub: hub.clone(),
            git: Arc::new(FakeGit::default()),
            launcher: Arc::new(FakeLauncher::default()),
            builds: Arc::new(FakeBuilds::default()),
            session: None,
            tree: None,
        };
        (ctx, hub)
    }

    #[test]
    fn the_hook_reports_a_title_only_when_it_changes() {
        let dir = app_data("hook", Some(r#"{"title":"Before"}"#));
        let (ctx, hub) = context();
        let session = SessionId::new(SESSION).unwrap();
        let hook = TitleHook::new(Some(dir.clone()));
        assert!(hook.report(&ctx, &session));
        assert!(!hook.report(&ctx, &session));
        let org = dir.join("claude-code-sessions").join("acct").join("org");
        std::fs::write(org.join(format!("{SESSION}.json")), r#"{"title":"After"}"#).unwrap();
        assert!(hook.report(&ctx, &session));
        let titles: Vec<String> = hub.session_titles().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Before", "After"]);
        assert!(!TitleHook::new(None).report(&ctx, &session));
    }

    #[test]
    fn reports_the_titles_of_sessions_holding_trees() {
        let dir = app_data("holders", Some(r#"{"title":"Holder"}"#));
        let (ctx, hub) = context();
        let db = ignite_store::local::open_in_memory().unwrap();
        for (id, session) in [("t-1", Some(SESSION)), ("t-2", None)] {
            ignite_store::local::trees::upsert(
                &db,
                &rows::Tree {
                    id: id.to_owned(),
                    path: format!("/trees/{id}"),
                    kind: "ticket".to_owned(),
                    ticket: None,
                    session: session.map(str::to_owned),
                    branch: None,
                    built_commit: None,
                    leased_at: None,
                    last_used: None,
                    state: "leased".to_owned(),
                    owner: None,
                },
            )
            .unwrap();
        }
        let hook = TitleHook::new(Some(dir));
        hook.report_holders(&ctx, &db);
        hook.report_holders(&ctx, &db);
        let titles: Vec<String> = hub.session_titles().into_iter().map(|t| t.title).collect();
        assert_eq!(titles, ["Holder"]);
    }

    #[test]
    fn reads_titles_that_start_with_a_status_emoji() {
        for status in ["⛔ [T.1] Stuck", "⚒️ [T.1] Working", "🔍 [T.1] Asking"] {
            let dir = app_data("emoji", Some(&format!(r#"{{"title":"{status}"}}"#)));
            assert_eq!(title(&dir, SESSION, &no_env).as_deref(), Some(status));
        }
    }

    #[test]
    fn finds_no_title_without_a_readable_titled_file() {
        assert_eq!(title(&app_data("missing", None), SESSION, &no_env), None);
        let untitled = app_data("untitled", Some(r#"{"title":""}"#));
        assert_eq!(title(&untitled, SESSION, &no_env), None);
        let broken = app_data("broken", Some("not json"));
        assert_eq!(title(&broken, SESSION, &no_env), None);
        let dir = app_data("ids", Some(r#"{"title":"T"}"#));
        assert_eq!(title(&dir, "ignite", &no_env), None);
        assert_eq!(title(&dir, "local_../../x", &no_env), None);
    }
}
