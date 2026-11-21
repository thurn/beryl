"""Codex logs: per-request tokens, the log's own project filter, tool calls from a script's
shell commands, and ticket windows found from them."""

from __future__ import annotations

import json
from pathlib import Path

import pytest

from beryl_efficiency import cli, codex, ingest, spend, store
from tests.legacy_sources import Beads
from tests.synthetic import CHECKOUT
from tests.synthetic import bead as bead_record
from tests.synthetic import listing

HOST = "jarvis"
THREAD = "01a1dea5-54b3-7821-83db-56ee8875b124"
OTHER = "01a1dea5-54b3-7821-83db-56ee8875b125"
ENACT = "python3 ~/enact/scripts/enact.py"


class Rollout:
    """A synthetic Codex log, written in the shapes the real ones use."""

    def __init__(self, root: Path, thread: str, cwd: str = str(CHECKOUT)) -> None:
        folder = root / "sessions" / "2026" / "11" / "15"
        folder.mkdir(parents=True, exist_ok=True)
        self.path: Path = folder / f"rollout-2026-11-15T10-00-00-{thread}.jsonl"
        self.sums: dict[str, int] = {}
        self.lines: list[str] = []
        self.add("2026-11-15T10:00:00.000Z", "session_meta", {"id": thread, "cwd": cwd})

    def add(self, time: str, kind: str, payload: dict[str, object]) -> Rollout:
        self.lines.append(
            json.dumps({"timestamp": time, "type": kind, "payload": payload})
        )
        return self

    def turn(self, time: str, model: str = "gpt-6.1-sol") -> Rollout:
        return self.add(time, "turn_context", {"model": model})

    def request(
        self, time: str, fresh: int, cached: int, output: int, repeat: bool = False
    ) -> Rollout:
        """A `token_count` for a request of `fresh` uncached and `cached` cached input tokens
        (Codex's input counts both) and `output` tokens, written twice when `repeat`."""
        last = {
            "input_tokens": fresh + cached,
            "cached_input_tokens": cached,
            "cache_write_input_tokens": 0,
            "output_tokens": output,
            "reasoning_output_tokens": output // 2,
            "total_tokens": fresh + cached + output,
        }
        for key, value in last.items():
            self.sums[key] = self.sums.get(key, 0) + value
        info = {"last_token_usage": last, "total_token_usage": dict(self.sums)}
        for _ in range(2 if repeat else 1):
            self.add(time, "event_msg", {"type": "token_count", "info": info})
        return self

    def shell(self, time: str, call: str, command: str, done: str, out: str) -> Rollout:
        """A script that runs `command` through `tools.exec_command`, and its output."""
        code = f"text(await tools.exec_command({{cmd:{json.dumps(command)},workdir:'/x'}}));"
        self.add(
            time,
            "response_item",
            {
                "type": "custom_tool_call",
                "call_id": call,
                "name": "exec",
                "input": code,
            },
        )
        return self.add(
            done,
            "response_item",
            {
                "type": "custom_tool_call_output",
                "call_id": call,
                "output": [{"type": "input_text", "text": out}],
            },
        )

    def finish(self, time: str) -> Rollout:
        return self.add(time, "event_msg", {"type": "task_complete"})

    def quota(self, time: str, percent: int, bucket: str = "codex") -> Rollout:
        return self.add(
            time,
            "event_msg",
            {
                "type": "token_count",
                "rate_limits": {
                    "limit_id": bucket,
                    "primary": {"used_percent": 90, "window_minutes": 300},
                    "secondary": {
                        "used_percent": percent,
                        "window_minutes": 10080,
                        "resets_at": 1794873600,
                    },
                },
            },
        )

    def write(self) -> Path:
        self.path.write_text("\n".join(self.lines) + "\n")
        return self.path


def sources(root: Path, beads: bytes = b"[]") -> list[ingest.Source]:
    return [Beads(lambda: beads), codex.Rollouts([root / "sessions"], CHECKOUT, HOST)]


def run(root: Path, beads: bytes = b"[]") -> store.sqlite3.Connection:
    db = store.open_store(root / "store.sqlite")
    ingest.ingest(db, sources(root, beads), HOST)
    return db


def test_requests_come_from_last_usage_and_sum_to_the_final_total(
    tmp_path: Path,
) -> None:
    log = (
        Rollout(tmp_path, THREAD)
        .turn("2026-11-15T10:00:01.000Z")
        .request("2026-11-15T10:00:10.000Z", 1000, 0, 50)
        .request("2026-11-15T10:00:20.000Z", 200, 1000, 80, repeat=True)
        .request("2026-11-15T10:00:30.000Z", 300, 1200, 20)
        .finish("2026-11-15T10:00:31.000Z")
    )
    log.write()
    db = run(tmp_path)
    rows = db.execute(
        "SELECT input_tokens, cache_write_tokens, cache_read_tokens, output_tokens, "
        "context_tokens, model, stop_reason FROM requests ORDER BY time"
    ).fetchall()
    # A repeated `token_count` line is one request. Input excludes the cached part.
    assert rows == [
        (1000, 0, 0, 50, 1000, "gpt-6.1-sol", "tool_use"),
        (200, 0, 1000, 80, 1200, "gpt-6.1-sol", "tool_use"),
        (300, 0, 1200, 20, 1500, "gpt-6.1-sol", "end_turn"),
    ]
    sums = [sum(int(r[i]) for r in rows) for i in (0, 2, 3)]
    final = log.sums
    assert sums[0] + sums[1] == final["input_tokens"]
    assert sums[1] == final["cached_input_tokens"]
    assert sums[2] == final["output_tokens"]
    # The prices table knows the model, so the requests have dollars.
    assert db.execute(
        "SELECT count(*) FROM requests WHERE dollars IS NULL"
    ).fetchone() == (0,)


def test_a_log_of_another_project_is_ignored(tmp_path: Path) -> None:
    Rollout(tmp_path, THREAD).request("2026-11-15T10:00:10.000Z", 10, 0, 1).write()
    Rollout(tmp_path, OTHER, cwd="/work/cognate").request(
        "2026-11-15T10:00:10.000Z", 10, 0, 1
    ).write()
    Rollout(
        tmp_path, "01a1dea5-54b3-7821-83db-56ee8875b126", cwd=f"{CHECKOUT}/.worktrees/x"
    ).request("2026-11-15T10:00:10.000Z", 10, 0, 1).write()
    db = run(tmp_path)
    found = {r[0] for r in db.execute("SELECT DISTINCT transcript FROM requests")}
    assert found == {THREAD, "01a1dea5-54b3-7821-83db-56ee8875b126"}


def test_ticket_spend_reprices_codex_and_estimates_its_weekly_share(
    tmp_path: Path,
) -> None:
    log = (
        Rollout(tmp_path, THREAD)
        .turn("2026-11-15T10:00:01.000Z", "gpt-6.2-sol")
        .quota("2026-11-15T10:00:02.000Z", 20)
        .shell(
            "2026-11-15T10:01:00.000Z",
            "c1",
            f"{ENACT} start by-a",
            "2026-11-15T10:01:02.000Z",
            "{}",
        )
        .request("2026-11-15T10:01:05.000Z", 100_000, 100_000, 1000)
        .quota("2026-11-15T10:01:10.000Z", 20)
        .quota("2026-11-15T10:01:11.000Z", 19)
        .shell(
            "2026-11-15T10:02:00.000Z",
            "c2",
            f"{ENACT} close by-a --reason done",
            "2026-11-15T10:02:02.000Z",
            "{}",
        )
        .request("2026-11-15T10:04:05.000Z", 100_000, 100_000, 1000)
        .quota("2026-11-15T10:04:10.000Z", 21)
        .quota("2026-11-15T10:05:00.000Z", 40, bucket="another_limit")
    )
    log.write()
    beads = listing(bead_record("by-a"))
    db = run(tmp_path, beads)
    [before] = spend.for_hub(db, HOST, None)
    # One tick of the account counter is too little to calibrate a ticket.
    assert "plan_percent" not in before
    dollars = before["dollars"]
    assert isinstance(dollars, list)
    assert sum(s["d"] for s in dollars) == pytest.approx(0.22)

    log.quota("2026-11-15T10:06:00.000Z", 22).write()
    # An upgrade must backfill samples from already ingested lines and preserve a
    # request whose log is gone.
    db.execute("DROP TABLE codex_plan_usage")
    db.execute(f"PRAGMA user_version = {store.version() - 1}")
    db.execute(
        """INSERT INTO requests (host, provider, transcript, message, time)
        VALUES (?, 'codex', ?, 'old', '2026-11-01T00:00:00.000Z')""",
        (HOST, OTHER),
    )
    db.commit()
    db.close()
    db = run(tmp_path, beads)
    assert db.execute(
        "SELECT count(*) FROM requests WHERE transcript = ?", (OTHER,)
    ).fetchone() == (1,)
    # The ticket's $0.22 was half of the host's spend during a two-point quota rise.
    [after] = spend.for_hub(db, HOST, "2026-11-15T10:05:00.000Z")
    assert after["plan_window"] == "Codex weekly"
    assert after["plan_percent"] == pytest.approx(1.0)
    assert after["plan_borrowed"] is False


def test_windows_are_found_from_the_scripts_shell_commands(tmp_path: Path) -> None:
    (
        Rollout(tmp_path, THREAD)
        .turn("2026-11-15T10:00:01.000Z")
        .request("2026-11-15T10:00:05.000Z", 10, 0, 1)
        .shell(
            "2026-11-15T10:01:00.000Z",
            "c1",
            f"cd /work/beryl && {ENACT} start by-a",
            "2026-11-15T10:01:02.000Z",
            "{}",
        )
        .request("2026-11-15T10:01:03.000Z", 10, 0, 1)
        .shell(
            "2026-11-15T10:10:00.000Z",
            "c2",
            f"{ENACT} close by-a --reason done",
            "2026-11-15T10:10:03.000Z",
            "Finished `test` profile [unoptimized] target(s) in 1m 02s",
        )
        .request("2026-11-15T10:10:04.000Z", 10, 0, 1)
        .add(
            "2026-11-15T10:10:05.000Z",
            "response_item",
            {"type": "function_call_output", "name": "send_message", "output": "hi"},
        )
        .finish("2026-11-15T10:10:06.000Z")
        .write()
    )
    db = run(tmp_path)
    found = db.execute(
        "SELECT start, end, bead, kind FROM windows ORDER BY start"
    ).fetchall()
    # The first window reaches back to the first request; the rest of the log is no ticket.
    assert [(w[2], w[3]) for w in found] == [("by-a", "bead"), (None, "none")]
    assert found[0][:2] == ("2026-11-15T10:00:05.000Z", "2026-11-15T10:10:03.000Z")
    calls = db.execute(
        "SELECT call, tool, bead, end, compile_seconds FROM tool_calls ORDER BY start"
    ).fetchall()
    assert calls[0][:3] == ("c1", "Bash", "by-a")
    assert calls[1][4] == 62.0
    placed = db.execute("SELECT bead, count(*) FROM requests GROUP BY bead").fetchall()
    assert dict(placed) == {None: 1, "by-a": 2}
    dollars = db.execute(
        "SELECT sum(dollars) FROM window_dollars WHERE split = 'class'"
    ).fetchone()[0]
    total = db.execute("SELECT sum(dollars) FROM requests").fetchone()[0]
    assert abs(dollars - total) < 1e-12


def test_shell_commands_are_read_from_string_literals() -> None:
    code = (
        "const a = await tools.exec_command({cmd:'echo \\'hi\\' && ls',workdir:'/x'});\n"
        'const b = await tools.exec_command({"cmd": "git status\\n", yield_time_ms: 1});\n'
        "const c = await tools.exec_command({cmd: base + '/x'});"
    )
    assert codex.shell_commands(code) == ["echo 'hi' && ls", "git status\n"]


def test_a_bead_that_ran_on_codex_here_has_data(tmp_path: Path) -> None:
    (Rollout(tmp_path, THREAD).request("2026-11-15T10:00:05.000Z", 10, 0, 1).write())
    beads = listing(
        bead_record(
            "by-a",
            metadata={
                "enact_spawn_host": HOST,
                "enact_spawn_provider": "codex",
                "enact_session": THREAD,
            },
        ),
        bead_record(
            "by-b",
            metadata={"enact_spawn_host": HOST, "enact_spawn_provider": "claude"},
        ),
    )
    db = run(tmp_path, beads)
    # Claude has no transcript here, so only the Claude ticket has no data.
    assert cli.gaps(db)["beads_no_data"] == {HOST: 1}
