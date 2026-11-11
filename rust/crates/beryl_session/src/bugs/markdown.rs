//! The markdown of a bug report, `report.md`: its front matter and the sections after it.

use std::fmt::Write as _;

use crate::ai::AgentMove;
use crate::protocol::Status;

use super::{BugReport, SpecMoment, body, commit};

/// Where the game stood.
pub(super) struct Game {
    pub(super) session: Option<u32>,
    pub(super) seed: Option<u64>,
    pub(super) turn: Option<u16>,
    pub(super) step: Option<String>,
    pub(super) decision: Option<u32>,
    pub(super) status: Option<&'static str>,
}

impl Game {
    pub(super) fn of(
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

pub(super) struct Markdown<'a> {
    pub(super) id: &'a str,
    pub(super) title: &'a str,
    pub(super) created: &'a str,
    pub(super) report: &'a BugReport,
    pub(super) moves: &'a [AgentMove],
    pub(super) screenshot: bool,
    pub(super) game: &'a Game,
    pub(super) files: &'a [(&'a str, String)],
}

pub(super) fn front_matter(out: &mut String, m: &Markdown<'_>, viewport: &str) {
    let quote = |s: &str| serde_json::to_string(s).expect("a string serializes");
    let opt = |v: Option<String>| v.unwrap_or_default();
    let (commit, dirty) = commit();
    let g = m.game;
    let kind = if m.report.spec_viewer {
        "spec-viewer"
    } else if m.report.ai {
        "ai"
    } else {
        "bug"
    };
    // JSON strings are YAML strings: the front matter parses as YAML whatever the title holds.
    writeln!(
        out,
        "---\n\
         id: {}\n\
         title: {}\n\
         kind: {}\n\
         status: open\n\
         bead:\n\
         created: {}\n\
         version: {}\n\
         commit: {}{}\n\
         url: {}\n\
         viewport: {viewport}\n\
         spec: {}\n\
         session: {}\n\
         seed: {}\n\
         turn: {}\n\
         step: {}\n\
         game: {}\n\
         decision: {}\n\
         ---\n",
        m.id,
        quote(m.title),
        kind,
        m.created,
        env!("CARGO_PKG_VERSION"),
        commit,
        if dirty { " (uncommitted changes)" } else { "" },
        quote(&m.report.page.url),
        opt(m.report.spec.as_ref().map(ToString::to_string)),
        opt(g.session.map(|s| s.to_string())),
        opt(g.seed.map(|s| s.to_string())),
        opt(g.turn.map(|t| t.to_string())),
        opt(g.step.clone()),
        g.status.unwrap_or(""),
        opt(g.decision.map(|d| d.to_string())),
    )
    .expect("a String takes writes");
}

pub(super) fn markdown(m: &Markdown<'_>) -> String {
    let mut out = String::new();
    let g = m.game;
    let viewport = if m.report.spec_viewer {
        let page = &m.report.page;
        format!(
            "{}×{} CSS px at {}× scale",
            page.width, page.height, page.scale
        )
    } else {
        String::new()
    };
    front_matter(&mut out, m, &viewport);
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
    if m.report.ai {
        ai_section(&mut out, m.moves);
    }
    out.push_str("## Reproduce\n\n");
    if m.report.spec_viewer {
        writeln!(
            out,
            "- Spec viewer page: `{}` ({viewport})",
            m.report.page.url
        )
        .expect("writes");
    }
    if let Some(spec) = &m.report.spec {
        spec_lines(&mut out, spec);
    }
    if g.seed.is_some() {
        let turn = g.turn.map(|t| format!(" --turn {t}")).unwrap_or_default();
        writeln!(
            out,
            "- The game, as a transcript to the reported turn:\n  \
             `just review \"<report folder>/replay.json\"{turn}`"
        )
        .expect("writes");
    }
    if m.report.ai && g.seed.is_some() {
        let turn = g.turn.map(|t| format!(" --turn {t}")).unwrap_or_default();
        writeln!(
            out,
            "- The agent's searches again, a fresh sample (slow: at low priority):\n  \
             `just review \"<report folder>/replay.json\" --agent recorded{turn}`"
        )
        .expect("writes");
    }
    if m.report.updates.is_empty() {
        if m.report.spec.is_none() {
            writeln!(out, "- The page: `{}`", m.report.page.url).expect("writes");
        }
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

/// How many of the agent's latest decisions an AI report's digest shows.
pub(super) const DIGEST: usize = 12;

/// An AI report's digest of the agent's latest decisions, newest last: `searches.json` has
/// them all.
pub(super) fn ai_section(out: &mut String, moves: &[AgentMove]) {
    out.push_str("## The AI's latest decisions\n\n");
    if moves.is_empty() {
        out.push_str("None recorded: no game, or the agent has not decided yet.\n\n");
        return;
    }
    out.push_str(
        "Newest last. Root visits and mean reward per candidate, the chosen one starred; \
         `searches.json` has the last 200.\n\n```\n",
    );
    for m in &moves[moves.len().saturating_sub(DIGEST)..] {
        out.push_str(&m.describe());
    }
    out.push_str("```\n\n");
}

/// The Reproduce lines for a note on a spec moment: the commands that show it again.
pub(super) fn spec_lines(out: &mut String, spec: &SpecMoment) {
    writeln!(
        out,
        "- The spec moment, as text from both seats:\n  \
         `just specs {card} --describe --moment {moment}`\n\
         - Its page, both seats side by side: `/specs/{card}/{moment}` (`just specs {card}`); \
         this seat full size: `/specs/{spec}`",
        card = spec.card,
        moment = spec.moment,
    )
    .expect("writes");
}

pub(super) fn region_note(report: &BugReport) -> String {
    match report.region {
        Some(r) => format!(
            "the region chosen: {:.0}x{:.0} at ({:.0}, {:.0}) of a {:.0}x{:.0} window",
            r.width, r.height, r.x, r.y, report.page.width, report.page.height
        ),
        None => "the region chosen".to_owned(),
    }
}
