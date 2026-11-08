//! Bug reports: what the player saw, what they say went wrong, and everything needed to put
//! the game back where it was. The UI's bug button sends a [`BugReport`]; [`write`] makes a
//! folder for it under [`dir`]:
//!
//! - `report.md`: front matter (`status: open`, the game's seed, turn and decision, the build)
//!   and the description, the screenshot, and how to reproduce it;
//! - `screenshot.png`, the region the player chose, and `window.png`, the whole window;
//! - `replay.json`, the game's replay file as of its latest decision, which reproduces it bit
//!   for bit (`just review replay.json --turn N`);
//! - `view.json`, the latest view the session emitted;
//! - `fixture.json`, the latest views the UI received as a UI fixture, which replays them on
//!   the game screen (`/?fixture=<name>#/game`);
//! - `ui.json`, the page and the UI's own log: its commands, events, errors and console;
//! - `journal.jsonl`, the latest records of the process's [`journal`].
//!
//! `by bugs` (`tools/src/beryl_tools/bugs.py`) lists the open reports, files a bead for each and
//! marks them done; the `bugfix` skill (`.agents/bugfix`) works them.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::SystemTime;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::ipc::{CHOOSE, Fixture, FixtureMessage, GAME_UPDATE, SET_STOPS};
use crate::journal::{self, Level};
use crate::protocol::Status;
use crate::{SessionError, Sessions};

/// How many journal records a report copies.
const JOURNAL_RECORDS: usize = 2000;

/// The UI's report.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BugReport {
    /// What the player says went wrong.
    pub description: String,
    /// The app's snapshot of the window as the player froze it (`capture_window`), which the
    /// app turns into the report's images ([`Images`]); none in a browser.
    pub snapshot: Option<u32>,
    /// Where the region lies in the window, in CSS pixels.
    pub region: Option<Region>,
    pub page: PageInfo,
    /// The UI's log ([`ui/src/diagnostics.ts`]): commands, events, errors, console.
    #[ts(type = "unknown")]
    pub ui_log: Value,
    /// The latest game updates the UI received, oldest first.
    #[ts(type = "unknown[]")]
    pub updates: Vec<Value>,
}

/// A rectangle of the window, in CSS pixels.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// The page the report came from.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct PageInfo {
    pub url: String,
    /// The window's size in CSS pixels, and device pixels to one.
    pub width: f64,
    pub height: f64,
    pub scale: f64,
}

/// A report's images, as PNGs: the region the player chose, and the whole window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Images {
    pub screenshot: Option<Vec<u8>>,
    pub window: Option<Vec<u8>>,
}

/// Where a report was written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct BugSaved {
    pub id: String,
    /// Its folder.
    pub path: String,
}

/// The UI's records for the journal ([`crate::ipc::UI_LOG`]).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UiLogRequest {
    pub entries: Vec<UiLogEntry>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
pub struct UiLogEntry {
    pub level: Level,
    pub message: String,
    #[ts(type = "unknown")]
    pub data: Value,
}

/// Journals the UI's records under the source `ui`.
pub fn ui_log(request: UiLogRequest) {
    for entry in request.entries {
        journal::record(entry.level, "ui", entry.message, entry.data);
    }
}

/// Where reports go: `$BERYL_BUGS`, else the app's data directory's `bugs`
/// (`~/Library/Application Support/ca.thurn.beryl/bugs`), wherever the report came from.
pub fn dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("BERYL_BUGS") {
        return PathBuf::from(dir);
    }
    let home = std::env::var_os("HOME").map_or_else(|| PathBuf::from("."), PathBuf::from);
    home.join("Library/Application Support/ca.thurn.beryl/bugs")
}

/// Writes `report` into a new folder under `dir`, with its `images` and what `sessions` knows
/// of the game.
pub fn write(
    dir: &Path,
    report: &BugReport,
    images: &Images,
    sessions: &Sessions,
) -> Result<BugSaved, SessionError> {
    let now = SystemTime::now();
    let title = title(&report.description);
    let id = format!("{}-{}", stamp(now), slug(&title));
    let folder = dir.join(&id);
    std::fs::create_dir_all(&folder).map_err(SessionError::io(&folder))?;
    let put = |name: &str, bytes: &[u8]| {
        let path = folder.join(name);
        std::fs::write(&path, bytes).map_err(SessionError::io(&path))
    };

    let mut files: Vec<(&str, String)> = Vec::new();
    if let Some(png) = &images.screenshot {
        put("screenshot.png", png)?;
        files.push(("screenshot.png", region_note(report)));
    }
    if let Some(png) = &images.window {
        put("window.png", png)?;
        files.push(("window.png", "the whole window, frozen".to_owned()));
    }
    let replay = sessions.current_replay();
    if let Some(replay) = &replay {
        put("replay.json", pretty(replay).as_bytes())?;
        files.push((
            "replay.json",
            format!(
                "seed {}, {} against `{}`, {} log entries: reproduces the game",
                replay.seed,
                if replay.human == 0 { "P0" } else { "P1" },
                replay.opponent,
                replay.log.len()
            ),
        ));
    }
    let current = sessions.current();
    if let Some(update) = &current {
        put("view.json", pretty(update).as_bytes())?;
        files.push((
            "view.json",
            "the latest view the session emitted".to_owned(),
        ));
    }
    if !report.updates.is_empty() {
        put("fixture.json", pretty(&fixture(&report.updates)).as_bytes())?;
        files.push((
            "fixture.json",
            format!(
                "the UI's latest {} views, as a UI fixture",
                report.updates.len()
            ),
        ));
    }
    put(
        "ui.json",
        pretty(&json!({ "page": report.page, "region": report.region, "log": report.ui_log }))
            .as_bytes(),
    )?;
    files.push((
        "ui.json",
        "the page, and the UI's commands, events, errors and console".to_owned(),
    ));
    journal::info(
        "bugs",
        "report written",
        json!({ "id": id, "path": folder, "title": title }),
    );
    let mut lines = String::new();
    for record in journal::recent(JOURNAL_RECORDS) {
        lines.push_str(&serde_json::to_string(&record).expect("a record serializes"));
        lines.push('\n');
    }
    put("journal.jsonl", lines.as_bytes())?;
    files.push((
        "journal.jsonl",
        match journal::path() {
            Some(path) => format!(
                "the journal's latest records; all of it is in `{}`",
                path.display()
            ),
            None => "the journal's latest records".to_owned(),
        },
    ));

    let game = Game::of(
        current.as_ref().map(|u| &u.view.status),
        current.as_ref(),
        replay.as_ref(),
    );
    let markdown = markdown(&Markdown {
        id: &id,
        title: &title,
        created: &journal::utc(now),
        report,
        screenshot: images.screenshot.is_some(),
        game: &game,
        files: &files,
        folder: &folder,
    });
    put("report.md", markdown.as_bytes())?;
    Ok(BugSaved {
        id,
        path: folder.display().to_string(),
    })
}

/// Where the game stood.
struct Game {
    session: Option<u32>,
    seed: Option<u64>,
    turn: Option<u16>,
    step: Option<String>,
    decision: Option<u32>,
    status: Option<&'static str>,
}

impl Game {
    fn of(
        status: Option<&Status>,
        update: Option<&crate::GameUpdate>,
        replay: Option<&crate::ReplayFile>,
    ) -> Game {
        Game {
            session: update.map(|u| u.session.0),
            seed: replay.map(|r| r.seed),
            turn: update.map(|u| u.view.turn.number),
            step: update.map(|u| {
                serde_json::to_value(u.view.turn.step)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_owned))
                    .unwrap_or_default()
            }),
            decision: match status {
                Some(Status::Deciding { decision }) => Some(decision.id.seq),
                _ => None,
            },
            status: status.map(|s| match s {
                Status::Deciding { .. } => "deciding",
                Status::Thinking => "thinking",
                Status::Playing => "playing",
                Status::Stopped => "stopped",
                Status::Over { .. } => "over",
            }),
        }
    }
}

struct Markdown<'a> {
    id: &'a str,
    title: &'a str,
    created: &'a str,
    report: &'a BugReport,
    screenshot: bool,
    game: &'a Game,
    files: &'a [(&'a str, String)],
    folder: &'a Path,
}

fn markdown(m: &Markdown<'_>) -> String {
    let quote = |s: &str| serde_json::to_string(s).expect("a string serializes");
    let opt = |v: Option<String>| v.unwrap_or_default();
    let (commit, dirty) = commit();
    let mut out = String::new();
    let g = m.game;
    // JSON strings are YAML strings: the front matter parses as YAML whatever the title holds.
    writeln!(
        out,
        "---\n\
         id: {}\n\
         title: {}\n\
         status: open\n\
         bead:\n\
         created: {}\n\
         version: {}\n\
         commit: {}{}\n\
         url: {}\n\
         session: {}\n\
         seed: {}\n\
         turn: {}\n\
         step: {}\n\
         game: {}\n\
         decision: {}\n\
         ---\n",
        m.id,
        quote(m.title),
        m.created,
        env!("CARGO_PKG_VERSION"),
        commit,
        if dirty { " (uncommitted changes)" } else { "" },
        quote(&m.report.page.url),
        opt(g.session.map(|s| s.to_string())),
        opt(g.seed.map(|s| s.to_string())),
        opt(g.turn.map(|t| t.to_string())),
        opt(g.step.clone()),
        g.status.unwrap_or(""),
        opt(g.decision.map(|d| d.to_string())),
    )
    .expect("a String takes writes");
    writeln!(
        out,
        "# {}\n\n{}\n",
        m.title,
        body(m.title, &m.report.description)
    )
    .expect("writes");
    if m.screenshot {
        out.push_str("![What the player saw](screenshot.png)\n\n");
    }
    out.push_str("## Reproduce\n\n");
    let folder = m.folder.display();
    if g.seed.is_some() {
        let turn = g.turn.map(|t| format!(" --turn {t}")).unwrap_or_default();
        writeln!(
            out,
            "- The game, as a transcript to the reported turn:\n  \
             `just review \"{folder}/replay.json\"{turn}`"
        )
        .expect("writes");
    }
    if m.report.updates.is_empty() {
        writeln!(out, "- The page: `{}`", m.report.page.url).expect("writes");
    } else {
        writeln!(
            out,
            "- The screen, its latest views replayed: copy `fixture.json` to \
             `ui/public/fixtures/bug-{id}.json` and open `/?fixture=bug-{id}#/game` \
             (`&at=-1` stops a view short, and the next answer plays the last)",
            id = m.id
        )
        .expect("writes");
    }
    out.push_str("\n## Files\n\n");
    for (name, about) in m.files {
        writeln!(out, "- [{name}]({name}): {about}").expect("writes");
    }
    out
}

fn region_note(report: &BugReport) -> String {
    match report.region {
        Some(r) => format!(
            "the region chosen: {:.0}x{:.0} at ({:.0}, {:.0}) of a {:.0}x{:.0} window",
            r.width, r.height, r.x, r.y, report.page.width, report.page.height
        ),
        None => "the region chosen".to_owned(),
    }
}

/// The views as a UI fixture: each a `game-update` event, with the answers the fixture
/// transport plays on with.
fn fixture(updates: &[Value]) -> Fixture {
    let mut messages = vec![
        FixtureMessage::response(CHOOSE, &()),
        FixtureMessage::response(SET_STOPS, &()),
    ];
    messages.extend(updates.iter().map(|payload| FixtureMessage::Event {
        event: GAME_UPDATE.to_owned(),
        payload: payload.clone(),
    }));
    Fixture { messages }
}

/// The report's title: its description's first line, cut to 80 characters.
fn title(description: &str) -> String {
    let line = description.trim().lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return "Untitled bug".to_owned();
    }
    let mut title: String = line.chars().take(80).collect();
    if line.chars().count() > 80 {
        title.push('…');
    }
    title
}

/// The description under its title: without its first line where that is the title whole.
fn body<'a>(title: &str, description: &'a str) -> &'a str {
    let description = description.trim();
    match description.split_once('\n') {
        Some((first, rest)) if first.trim() == title => rest.trim(),
        None if description == title => "",
        _ => description,
    }
}

/// A few words of `title`, lowercase and hyphenated, for a folder name.
fn slug(title: &str) -> String {
    let mut slug = String::new();
    for word in title
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| !w.is_empty())
    {
        if slug.len() + word.len() > 40 {
            break;
        }
        if !slug.is_empty() {
            slug.push('-');
        }
        slug.push_str(&word.to_ascii_lowercase());
    }
    if slug.is_empty() {
        "bug".to_owned()
    } else {
        slug
    }
}

/// `t` as UTC `YYYYMMDD-HHMMSS`, which sorts.
fn stamp(t: SystemTime) -> String {
    let secs = t
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (date, s) = journal::civil(secs);
    format!(
        "{}-{:02}{:02}{:02}",
        date.replace('-', ""),
        s / 3600,
        s / 60 % 60,
        s % 60
    )
}

/// The commit the app was built from, as its build script found it ([`set_build_commit`]).
static BUILD_COMMIT: OnceLock<String> = OnceLock::new();

/// Records the commit the running program was built from, for a report run from no checkout
/// (a bundled app).
pub fn set_build_commit(commit: &str) {
    let _ = BUILD_COMMIT.set(commit.to_owned());
}

/// The commit this runs from, and whether its checkout had uncommitted changes. Under
/// `cargo run` (`just dev`, the CLI), git says, of the checkout that built it; else the
/// commit the build recorded, whose changes are unknown.
fn commit() -> (String, bool) {
    let built = || {
        let commit = BUILD_COMMIT.get().map_or("unknown", String::as_str);
        (commit.to_owned(), false)
    };
    let Some(dir) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return built();
    };
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(&dir)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let Some(head) = git(&["rev-parse", "HEAD"]) else {
        return built();
    };
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    (head, dirty)
}

fn pretty(value: &impl Serialize) -> String {
    serde_json::to_string_pretty(value).expect("a report's parts serialize")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_and_slugs() {
        assert_eq!(
            title("\n  Arrow jumps: on block\nmore"),
            "Arrow jumps: on block"
        );
        assert_eq!(slug("Arrow jumps: on block"), "arrow-jumps-on-block");
        assert_eq!(slug("!!!"), "bug");
        assert_eq!(title(""), "Untitled bug");
        assert_eq!(body("Arrow jumps", "Arrow jumps\non block"), "on block");
        assert_eq!(body("Arrow jumps", "Arrow jumps"), "");
        let long = "x".repeat(90);
        assert_eq!(body(&title(&long), &long), long);
    }

    #[test]
    fn writes_a_report_without_a_game() {
        let dir = std::env::temp_dir().join(format!("beryl-bugs-{}", std::process::id()));
        let sessions = Sessions::new(None, |_| {});
        let report = BugReport {
            description: "The button: it vanished\nafter I clicked".to_owned(),
            snapshot: None,
            region: Some(Region {
                x: 1.0,
                y: 2.0,
                width: 30.0,
                height: 40.0,
            }),
            page: PageInfo {
                url: "http://localhost/#/game".to_owned(),
                width: 1440.0,
                height: 900.0,
                scale: 2.0,
            },
            ui_log: json!([]),
            updates: vec![json!({ "session": 1 })],
        };
        let images = Images {
            screenshot: Some(b"hi".to_vec()),
            window: None,
        };
        let saved = write(&dir, &report, &images, &sessions).unwrap();
        let folder = PathBuf::from(&saved.path);
        assert!(saved.id.ends_with("-the-button-it-vanished"));
        let md = std::fs::read_to_string(folder.join("report.md")).unwrap();
        assert!(md.starts_with("---\nid: "));
        assert!(md.contains("title: \"The button: it vanished\""));
        assert!(md.contains("status: open\n"));
        assert!(md.contains("![What the player saw](screenshot.png)"));
        assert_eq!(std::fs::read(folder.join("screenshot.png")).unwrap(), b"hi");
        let fixture: Fixture =
            serde_json::from_str(&std::fs::read_to_string(folder.join("fixture.json")).unwrap())
                .unwrap();
        assert_eq!(fixture.messages.len(), 3);
        assert!(folder.join("journal.jsonl").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}
