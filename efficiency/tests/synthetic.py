"""Small synthetic sources for the tests."""

from __future__ import annotations

import json
from collections.abc import Mapping
from pathlib import Path

from beryl_efficiency.ingest import LineSource, Row


def bead(id: str, **fields: object) -> dict[str, object]:
    """A bead record as `bd list --json` prints one, with `fields` over the defaults."""
    return {
        "id": id,
        "title": f"Bead {id}",
        "status": "open",
        "priority": 2,
        "issue_type": "task",
        "created_at": "2026-11-01T10:00:00Z",
        **fields,
    }


def listing(*beads: Mapping[str, object]) -> bytes:
    return json.dumps(list(beads)).encode()


class Lines(LineSource):
    """A line source over `folder`'s `*.jsonl` files: each line is an object with `id` and
    `n`, written to the `beads` table as a bead `id` with priority `n`."""

    name = "lines"

    def __init__(self, folder: Path) -> None:
        self.folder = folder

    def files(self) -> list[Path]:
        return sorted(self.folder.glob("*.jsonl"))

    def parse(self, path: Path, line: bytes) -> list[Row]:
        record = json.loads(line)
        return [("beads", {"id": str(record["id"]), "priority": int(record["n"])})]


# The main checkout the Claude tests pretend to work in, and its transcript folder's name.
CHECKOUT = Path("/work/beryl")
PROJECT = "-work-beryl"


class Transcript:
    """A synthetic Claude Code transcript, written as the real ones are: an assistant message
    with several content blocks takes one line per block, each with the same id and usage.
    """

    def __init__(self, path: Path) -> None:
        self.path = path
        self.lines: list[str] = []

    def request(
        self,
        time: str,
        message: str,
        usage: tuple[int, int, int, int] = (1, 10, 100, 5),
        tools: tuple[tuple[str, str, Mapping[str, object]], ...] = (),
        stop: str | None = None,
        model: str = "claude-test",
        hour: int = 0,
    ) -> Transcript:
        """A request at `time` with `usage` as (input, cache write, cache read, output), a
        text block and a `tool_use` block for each of `tools`, given as (id, name, input).
        It stops for its tools, or with `stop`, else ends the turn. `hour` of its cache
        writes went to the 1-hour cache.
        """
        counts: dict[str, object] = dict(
            zip(
                [
                    "input_tokens",
                    "cache_creation_input_tokens",
                    "cache_read_input_tokens",
                    "output_tokens",
                ],
                usage,
            )
        )
        counts["cache_creation"] = {
            "ephemeral_1h_input_tokens": hour,
            "ephemeral_5m_input_tokens": usage[1] - hour,
        }
        blocks: list[dict[str, object]] = [{"type": "text", "text": "thinking aloud"}]
        for call, name, given in tools:
            use: dict[str, object] = {"type": "tool_use", "id": call, "name": name}
            use["input"] = dict(given)
            blocks.append(use)
        for block in blocks:
            self.lines.append(
                json.dumps(
                    {
                        "type": "assistant",
                        "timestamp": time,
                        "message": {
                            "id": message,
                            "model": model,
                            "usage": counts,
                            "content": [block],
                            "stop_reason": stop
                            or ("tool_use" if tools else "end_turn"),
                        },
                    }
                )
            )
        return self

    def bash(self, time: str, message: str, call: str, command: str) -> Transcript:
        return self.request(
            time, message, tools=((call, "Bash", {"command": command}),)
        )

    def prompt(
        self,
        time: str,
        text: str = "please go on",
        origin: Mapping[str, object] | None = None,
    ) -> Transcript:
        """A message that starts a turn: typed by a person unless `origin` says otherwise
        (such as `{"kind": "task-notification"}`)."""
        record: dict[str, object] = {
            "type": "user",
            "timestamp": time,
            "message": {"role": "user", "content": text},
            "origin": dict(origin or {"kind": "human"}),
        }
        self.lines.append(json.dumps(record))
        return self

    def result(
        self, time: str, call: str, stdout: str = "ok", stderr: str = ""
    ) -> Transcript:
        self.lines.append(
            json.dumps(
                {
                    "type": "user",
                    "timestamp": time,
                    "message": {
                        "content": [
                            {
                                "type": "tool_result",
                                "tool_use_id": call,
                                "content": stdout,
                            }
                        ]
                    },
                    "toolUseResult": {"stdout": stdout, "stderr": stderr},
                }
            )
        )
        return self

    def save(self) -> Path:
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.path.write_text("".join(line + "\n" for line in self.lines))
        return self.path
