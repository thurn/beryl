//! The title of the Claude desktop session filing a ticket, saved on the ticket so the dashboard
//! can say which session filed it.
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
//! The format belongs to the app and is undocumented, so every failure here means "no title",
//! and the ticket is filed exactly as it would be without one.

use std::path::{Path, PathBuf};

use serde_json::{Value, json};

/// The metadata key a ticket's filing session's title is saved under.
pub const CREATED_BY_TITLE: &str = "created_by_title";

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

/// Adds `title` to the metadata of a `create` operation.
pub fn attach(op: &mut Value, title: Option<String>) {
    let Some(title) = title else { return };
    op["fields"]["metadata"] = json!({ CREATED_BY_TITLE: title });
}

#[cfg(test)]
mod tests {
    use super::*;

    const SESSION: &str = "local_8c29f0b2-fddb-4e96-9da4-b1872d57316a";

    fn app_data(name: &str, contents: Option<&str>) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ignite-cli-session-title-{name}-{}",
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

    #[test]
    fn attach_adds_the_title_to_the_create_metadata() {
        let mut op = json!({"op": "create", "fields": {"title": "x"}});
        attach(&mut op, None);
        assert_eq!(op, json!({"op": "create", "fields": {"title": "x"}}));
        attach(&mut op, Some("Status check".into()));
        assert_eq!(
            op["fields"]["metadata"],
            json!({"created_by_title": "Status check"})
        );
    }
}
