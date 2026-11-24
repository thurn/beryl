"""Antigravity CLI (`agy`) transcripts: Gemini vs Claude provider splitting, workspace
filtering (macOS and Windows `file:///` URIs), subagent attribution, tool pairing, and
per-provider plan estimates."""

from __future__ import annotations

import json
import sqlite3
from pathlib import Path

import pytest

from beryl_efficiency import agy, ingest, spend, store
from tests.legacy_sources import Beads
from tests.synthetic import CHECKOUT
from tests.synthetic import bead as bead_record
from tests.synthetic import listing

HOST = "jarvis"
GEMINI_CONV = "11111111-2222-3333-4444-555555555551"
CLAUDE_CONV = "11111111-2222-3333-4444-555555555552"
SUB_CONV = "11111111-2222-3333-4444-555555555553"
OTHER_CONV = "11111111-2222-3333-4444-555555555554"
WEEKLY = "Weekly · all models"
RESETS = "2026-11-16T23:00:00.000Z"

SUMMARIES_SCHEMA = """
CREATE TABLE IF NOT EXISTS conversation_summaries (
    conversation_id TEXT PRIMARY KEY,
    workspace_uris TEXT NOT NULL,
    parent_conversation_id TEXT NOT NULL DEFAULT ''
);
"""


def register_summary(root: Path, cid: str, uris: list[str], parent: str = "") -> None:
    root.mkdir(parents=True, exist_ok=True)
    conn = sqlite3.connect(root / "conversation_summaries.db")
    conn.executescript(SUMMARIES_SCHEMA)
    conn.execute(
        "INSERT OR REPLACE INTO conversation_summaries VALUES (?, ?, ?)",
        (cid, json.dumps(uris), parent),
    )
    conn.commit()
    conn.close()


class AgyTranscript:
    """A synthetic `agy` `transcript.jsonl` written in the real step format."""

    def __init__(self, root: Path, cid: str) -> None:
        folder = root / "brain" / cid / ".system_generated" / "logs"
        folder.mkdir(parents=True, exist_ok=True)
        self.path: Path = folder / "transcript.jsonl"
        self.lines: list[str] = []
        self.step = 0

    def _push(self, record: dict[str, object]) -> int:
        idx = self.step
        self.step += 1
        self.lines.append(json.dumps({"step_index": idx, **record}))
        return idx

    def user(self, created: str, content: str = "start") -> AgyTranscript:
        self._push(
            {
                "source": "USER_EXPLICIT",
                "type": "USER_INPUT",
                "status": "DONE",
                "created_at": created,
                "content": content,
            }
        )
        return self

    def planner(
        self,
        created: str,
        model: str,
        fresh: int,
        cached: int,
        output: int,
        tools: list[tuple[str, dict[str, object]]] | None = None,
    ) -> AgyTranscript:
        entry: dict[str, object] = {
            "source": "MODEL",
            "type": "PLANNER_RESPONSE",
            "status": "DONE",
            "created_at": created,
            "model": model,
            "input_tokens": fresh,
            "cache_read_tokens": cached,
            "output_tokens": output,
        }
        if tools:
            entry["tool_calls"] = [
                {
                    "name": name,
                    "args": {k: json.dumps(v) for k, v in args.items()},
                }
                for name, args in tools
            ]
        self._push(entry)
        return self

    def generic(
        self, created: str, completed: str, output: str = "ok"
    ) -> AgyTranscript:
        content = (
            f"Created At: {created}\nCompleted At: {completed}\n"
            f"Command exited with code 0.\n\nCommand output:\n{output}"
        )
        self._push(
            {
                "source": "MODEL",
                "type": "GENERIC",
                "status": "DONE",
                "created_at": created,
                "content": content,
            }
        )
        return self

    def write(self) -> Path:
        self.path.write_text("\n".join(self.lines) + "\n", encoding="utf-8")
        return self.path


def run(
    root: Path, checkout: Path = CHECKOUT, beads: bytes = b"[]"
) -> sqlite3.Connection:
    db = store.open_store(root / "store.sqlite")
    sources: list[ingest.Source] = [
        Beads(lambda: beads),
        agy.Transcripts(root / "agy", checkout, HOST),
    ]
    ingest.ingest(db, sources, HOST)
    return db


def test_gemini_and_claude_transcripts_split_providers_and_price_tokens(
    tmp_path: Path,
) -> None:
    agy_root = tmp_path / "agy"
    checkout = tmp_path / "checkout"
    readme = str(checkout / "README.md")
    register_summary(agy_root, GEMINI_CONV, [checkout.as_uri()])
    register_summary(agy_root, CLAUDE_CONV, [checkout.as_uri()])
    register_summary(agy_root, SUB_CONV, ["file:///tmp/sub"], parent=GEMINI_CONV)
    register_summary(agy_root, OTHER_CONV, ["file:///work/unrelated"])

    (
        AgyTranscript(agy_root, GEMINI_CONV)
        .user("2026-11-15T10:00:00Z")
        .planner(
            "2026-11-15T10:00:01Z",
            "gemini-4-neon-high",
            100_000,
            0,
            1_000,
            tools=[
                ("run_command", {"CommandLine": "ignite start by-a"}),
                ("read_file", {"Path": readme}),
            ],
        )
        .generic("2026-11-15T10:00:02Z", "2026-11-15T10:00:05Z", "started")
        .generic("2026-11-15T10:00:02Z", "2026-11-15T10:00:03Z", "readme")
        .planner(
            "2026-11-15T10:00:10Z",
            "gemini-4-neon-high",
            10_000,
            100_000,
            500,
            tools=[("run_command", {"CommandLine": "cd rust && cargo test"})],
        )
        .generic(
            "2026-11-15T10:00:11Z",
            "2026-11-15T10:00:30Z",
            "    Finished `test` profile [unoptimized] target(s) in 6.50s\n",
        )
        .planner("2026-11-15T10:00:35Z", "gemini-4-neon-high", 2_000, 110_000, 200)
        .write()
    )
    # Subagent linked by parent_conversation_id is attributed to the parent's ticket.
    (
        AgyTranscript(agy_root, SUB_CONV)
        .user("2026-11-15T10:00:15Z")
        .planner("2026-11-15T10:00:16Z", "gemini-3.8-flash-low", 10_000, 0, 100)
        .write()
    )
    # Claude conversation under agy goes to agy-claude.
    (
        AgyTranscript(agy_root, CLAUDE_CONV)
        .user("2026-11-15T10:05:00Z")
        .planner(
            "2026-11-15T10:05:01Z",
            "claude-opus-5-5-high",
            50_000,
            0,
            1_000,
            tools=[("run_command", {"CommandLine": "ignite start by-b"})],
        )
        .generic("2026-11-15T10:05:02Z", "2026-11-15T10:05:04Z", "started")
        .planner("2026-11-15T10:05:10Z", "claude-opus-5-5-high", 1_000, 50_000, 200)
        .write()
    )
    # Unrelated workspace must be ignored.
    (
        AgyTranscript(agy_root, OTHER_CONV)
        .user("2026-11-15T10:00:00Z")
        .planner("2026-11-15T10:00:01Z", "gemini-4-neon-high", 1_000, 0, 10)
        .write()
    )

    beads = listing(bead_record("by-a"), bead_record("by-b"))
    db = run(tmp_path, checkout, beads=beads)

    providers = dict(
        db.execute(
            "SELECT transcript, provider FROM transcripts ORDER BY transcript"
        ).fetchall()
    )
    assert providers == {
        GEMINI_CONV: agy.PROVIDER_GEMINI,
        CLAUDE_CONV: agy.PROVIDER_CLAUDE,
        SUB_CONV: agy.PROVIDER_GEMINI,
    }
    assert OTHER_CONV not in providers

    # All requests are priced and attributed to their tickets (including the subagent).
    unpriced = db.execute(
        "SELECT count(*) FROM requests WHERE dollars IS NULL"
    ).fetchone()[0]
    assert unpriced == 0
    by_bead = dict(
        db.execute("SELECT bead, count(*) FROM requests GROUP BY bead").fetchall()
    )
    assert by_bead == {"by-a": 4, "by-b": 2}

    # Tool calls pair cleanly with GENERIC results and Completed At timestamps.
    calls = db.execute(
        "SELECT tool, command, end, compile_seconds FROM tool_calls "
        "WHERE transcript = ? ORDER BY cast(call AS INTEGER)",
        (GEMINI_CONV,),
    ).fetchall()
    assert calls == [
        ("Bash", "ignite start by-a", "2026-11-15T10:00:05.000Z", None),
        ("read_file", readme, "2026-11-15T10:00:03.000Z", None),
        ("Bash", "cd rust && cargo test", "2026-11-15T10:00:30.000Z", 6.5),
    ]

    # Per-provider plan usage estimates attach to agy-gemini and agy-claude separately.
    gemini_dollars = float(
        db.execute(
            "SELECT sum(dollars) FROM requests WHERE provider = ?",
            (agy.PROVIDER_GEMINI,),
        ).fetchone()[0]
    )
    claude_dollars = float(
        db.execute(
            "SELECT sum(dollars) FROM requests WHERE provider = ?",
            (agy.PROVIDER_CLAUDE,),
        ).fetchone()[0]
    )
    db.executemany(
        "INSERT INTO plan_usage (host, machine, provider, at, window, percent_used, "
        "resets_at) VALUES (?, 'm-1', ?, ?, ?, ?, ?)",
        [
            (
                HOST,
                agy.PROVIDER_GEMINI,
                "2026-11-15T09:59:00.000Z",
                WEEKLY,
                10.0,
                RESETS,
            ),
            (
                HOST,
                agy.PROVIDER_GEMINI,
                "2026-11-15T10:01:00.000Z",
                WEEKLY,
                10.0 + 2.0 * gemini_dollars,
                RESETS,
            ),
            (
                HOST,
                agy.PROVIDER_CLAUDE,
                "2026-11-15T10:04:00.000Z",
                WEEKLY,
                30.0,
                RESETS,
            ),
            (
                HOST,
                agy.PROVIDER_CLAUDE,
                "2026-11-15T10:06:00.000Z",
                WEEKLY,
                30.0 + 5.0 * claude_dollars,
                RESETS,
            ),
        ],
    )
    db.commit()

    found = {str(w["provider"]): w for w in spend.for_hub(db, HOST, None)}
    assert set(found) == {agy.PROVIDER_GEMINI, agy.PROVIDER_CLAUDE}
    top_gemini_dollars = float(
        db.execute(
            "SELECT sum(dollars) FROM requests WHERE transcript = ?", (GEMINI_CONV,)
        ).fetchone()[0]
    )
    assert found[agy.PROVIDER_GEMINI]["plan_percent"] == pytest.approx(
        2.0 * top_gemini_dollars, abs=1e-4
    )
    assert found[agy.PROVIDER_CLAUDE]["plan_percent"] == pytest.approx(
        5.0 * claude_dollars, abs=1e-4
    )


def test_windows_file_uris_and_metadata_json_fallback(tmp_path: Path) -> None:
    agy_root = tmp_path / "agy"
    cache = agy_root / "cache"
    cache.mkdir(parents=True, exist_ok=True)
    (cache / "conversation_metadata.json").write_text(
        json.dumps(
            {
                "conversations": {
                    GEMINI_CONV: {
                        "summary": {
                            "WorkspaceURIs": ["file:///D:/work/beryl/.worktrees/tree-1"]
                        }
                    }
                }
            }
        ),
        encoding="utf-8",
    )
    (
        AgyTranscript(agy_root, GEMINI_CONV)
        .user("2026-11-15T10:00:00Z")
        .planner("2026-11-15T10:00:01Z", "gemini-4-hydrogen-medium", 5_000, 0, 100)
        .write()
    )
    assert agy.uri_to_path("file:///D:/work/beryl") == "D:/work/beryl"
    assert agy.uri_to_path("file:///Users/a/beryl") == "/Users/a/beryl"

    db = run(tmp_path, checkout=Path("D:/work/beryl"))
    rows = db.execute("SELECT provider, model FROM requests").fetchall()
    assert rows == [(agy.PROVIDER_GEMINI, "gemini-4-hydrogen-medium")]
