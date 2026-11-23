"""The Antigravity CLI (`agy`) source: per-conversation transcripts under
`~/.gemini/antigravity-cli/brain/<id>/.system_generated/logs/transcript.jsonl`.

Workspace filtering and subagent parent links come from `conversation_summaries.db` (with
`cache/conversation_metadata.json` as a fallback when a conversation has not yet been
indexed). Each conversation is assigned to `agy-gemini` or `agy-claude` from its first
`PLANNER_RESPONSE` model so requests match Ignite's separate quota pools (`Gemini Models`
vs `Claude and GPT models`).

Lines that matter in `transcript.jsonl`:

- `USER_INPUT` (`source == "USER_EXPLICIT"`) starts a turn as a `prompts` row.
- `PLANNER_RESPONSE` carries `step_index`, `created_at`, `model`, `input_tokens`
  (uncached fresh input), `cache_read_tokens`, `output_tokens` and `tool_calls`. Each
  tool call at position `i` produces its `GENERIC` tool-result step at `step_index + 1 + i`,
  so tool calls and results pair statelessly by `str(step_index + 1 + i)`.
- `GENERIC` carries the tool result's `content` (including `Completed At:` timestamps and
  command output) and closes its matching tool call.

Only commands, file paths and sizes are kept: no message text and no tool output.
"""

from __future__ import annotations

import json
import re
import sqlite3
from pathlib import Path
from urllib.parse import unquote, urlparse

from . import cilog, claude, kinds, paths, timing, tokens, windows
from .ingest import LineSource, Row

PROVIDER_GEMINI = "agy-gemini"
PROVIDER_CLAUDE = "agy-claude"
PROVIDERS: tuple[str, ...] = (PROVIDER_GEMINI, PROVIDER_CLAUDE)

# Lines without one of these types carry no requests, tool calls, tool results or prompts.
WANTED: tuple[bytes, ...] = (
    b'"PLANNER_RESPONSE"',
    b'"GENERIC"',
    b'"USER_INPUT"',
)
COMPLETED_AT: re.Pattern[str] = re.compile(r"^Completed At:\s*(\S+)", re.MULTILINE)


def uri_to_path(uri: str) -> str:
    """Converts a `file:///` URI (macOS, Linux or Windows) to a filesystem path string."""
    if not uri.startswith("file://"):
        return uri
    parsed = urlparse(uri)
    text = unquote(parsed.path)
    if len(text) >= 3 and text[0] == "/" and text[2] == ":":
        text = text[1:]
    return text


def provider_for_model(model: str) -> str:
    """The Ignite provider whose quota pool bills `model`."""
    if model.lower().startswith(("claude", "gpt")):
        return PROVIDER_CLAUDE
    return PROVIDER_GEMINI


def iso_time(value: str) -> str:
    """Normalizes an ISO-8601 timestamp (UTC or with offset) to UTC milliseconds."""
    return cilog.iso(cilog.utc(value))


def decode_arg(value: object) -> object:
    """`transcript.jsonl` stores tool call arguments as JSON-encoded strings; decode one."""
    if not isinstance(value, str):
        return value
    try:
        return json.loads(value)
    except ValueError:
        return value


def tool_target(tool: str, args: dict[str, object]) -> str | None:
    """What a tool call acted on: a shell command's first characters, or the path, pattern,
    url or query another tool was given. Never message text or file contents."""
    if tool == "run_command":
        cmd = args.get("CommandLine")
        if isinstance(cmd, str):
            return kinds.without_heredocs(cmd)[: claude.COMMAND_CHARACTERS]
        return None
    for key in ("Path", "path", "file_path", "pattern", "url", "query"):
        value = args.get(key)
        if isinstance(value, str) and value:
            return value[: claude.COMMAND_CHARACTERS]
    return None


def conversation_of(path: Path) -> str:
    """The conversation ID of `brain/<id>/.system_generated/logs/transcript.jsonl`."""
    return path.parents[2].name


def completed_at(content: str, fallback: str) -> str:
    """The tool's completion timestamp from its `Completed At:` header, else `fallback`."""
    found = COMPLETED_AT.search(content)
    if found:
        try:
            return iso_time(found.group(1))
        except ValueError:
            pass
    return iso_time(fallback)


def load_metadata(root: Path) -> dict[str, tuple[list[str], str | None]]:
    """Each conversation's workspace paths and optional parent conversation ID, from
    `conversation_summaries.db` and `cache/conversation_metadata.json`."""
    found: dict[str, tuple[list[str], str | None]] = {}
    db_path = root / "conversation_summaries.db"
    if db_path.exists():
        try:
            conn = sqlite3.connect(f"{db_path.resolve().as_uri()}?mode=ro", uri=True)
            try:
                for cid, raw_uris, parent in conn.execute(
                    "SELECT conversation_id, workspace_uris, parent_conversation_id "
                    "FROM conversation_summaries"
                ):
                    uris: list[str] = []
                    if isinstance(raw_uris, str) and raw_uris:
                        try:
                            parsed = json.loads(raw_uris)
                            if isinstance(parsed, list):
                                uris = [uri_to_path(str(u)) for u in parsed if u]
                        except ValueError:
                            pass
                    parent_id = str(parent) if parent else None
                    found[str(cid)] = (uris, parent_id)
            finally:
                conn.close()
        except sqlite3.Error:
            pass
    meta_path = root / "cache" / "conversation_metadata.json"
    if meta_path.exists():
        try:
            data = json.loads(meta_path.read_text(encoding="utf-8"))
        except (OSError, ValueError):
            data = None
        conversations = data.get("conversations") if isinstance(data, dict) else None
        if isinstance(conversations, dict):
            for cid, entry in conversations.items():
                if str(cid) in found or not isinstance(entry, dict):
                    continue
                summary = entry.get("summary")
                if not isinstance(summary, dict):
                    continue
                raw = summary.get("WorkspaceURIs")
                uris = (
                    [uri_to_path(str(u)) for u in raw if u]
                    if isinstance(raw, list)
                    else []
                )
                found[str(cid)] = (uris, None)
    return found


def first_provider(path: Path) -> str | None:
    """The provider of `path`, from its first `PLANNER_RESPONSE` model; None when the file
    has no model request yet."""
    try:
        with open(path, "rb") as handle:
            for line in handle:
                if b'"PLANNER_RESPONSE"' not in line or b'"model"' not in line:
                    continue
                try:
                    record = json.loads(line)
                except ValueError:
                    continue
                if (
                    isinstance(record, dict)
                    and record.get("type") == "PLANNER_RESPONSE"
                ):
                    model = record.get("model")
                    if isinstance(model, str) and model:
                        return provider_for_model(model)
    except OSError:
        return None
    return None


class Transcripts(LineSource):
    """Every Antigravity CLI transcript whose workspace (or parent's workspace) is the
    main checkout or below it."""

    name = "agy"

    def __init__(self, root: Path, checkout: Path, host: str) -> None:
        self.root = root
        self.checkout = str(checkout)
        self.host = host
        self._meta: dict[str, tuple[list[str], str | None]] = {}
        self._providers: dict[Path, str] = {}

    def _belongs(self, cid: str, seen: set[str] | None = None) -> bool:
        entry = self._meta.get(cid)
        if entry is None:
            return False
        uris, parent = entry
        if any(paths.inside(uri, self.checkout) for uri in uris):
            return True
        if parent:
            visited = set() if seen is None else seen
            if parent not in visited:
                visited.add(cid)
                return self._belongs(parent, visited)
        return False

    def _parent_of(self, path: Path) -> str | None:
        entry = self._meta.get(conversation_of(path))
        return entry[1] if entry is not None else None

    def _provider_of(self, path: Path) -> str | None:
        cached = self._providers.get(path)
        if cached is not None:
            return cached
        found = first_provider(path)
        if found is not None:
            self._providers[path] = found
        return found

    def files(self) -> list[Path]:
        self._meta = load_metadata(self.root)
        brain = self.root / "brain"
        if not brain.exists():
            return []
        try:
            candidates = sorted(brain.glob("*/.system_generated/logs/transcript.jsonl"))
        except OSError:
            return []
        kept: list[Path] = []
        for path in candidates:
            if not self._belongs(conversation_of(path)):
                continue
            if self._provider_of(path) is None:
                continue
            kept.append(path)
        return kept

    def parse(self, path: Path, line: bytes) -> list[Row]:
        if not any(marker in line for marker in WANTED):
            return []
        record = json.loads(line)
        if not isinstance(record, dict):
            raise TypeError("not an object")
        provider = self._provider_of(path)
        if provider is None:
            return []
        kind = record.get("type")
        step_raw = record.get("step_index")
        if not isinstance(step_raw, int):
            raise TypeError("step_index is not an int")
        created = claude.text_field(record, "created_at")
        key: dict[str, object] = {
            "host": self.host,
            "provider": provider,
            "transcript": conversation_of(path),
        }
        if kind == "USER_INPUT":
            if record.get("source") != "USER_EXPLICIT":
                return []
            return [
                (
                    "prompts",
                    {
                        **key,
                        "time": iso_time(created),
                        "kind": "person",
                        "detail": None,
                    },
                )
            ]
        if kind == "PLANNER_RESPONSE":
            return self._planner(key, step_raw, iso_time(created), record)
        if kind == "GENERIC":
            return self._generic(key, step_raw, created, record)
        return []

    def _planner(
        self,
        key: dict[str, object],
        step_index: int,
        time: str,
        record: dict[str, object],
    ) -> list[Row]:
        input_tokens = claude.number(record, "input_tokens")
        cache_read = claude.number(record, "cache_read_tokens")
        output_tokens = claude.number(record, "output_tokens")
        model = record.get("model")
        raw_calls = record.get("tool_calls")
        tool_calls = (
            [tc for tc in raw_calls if isinstance(tc, dict)]
            if isinstance(raw_calls, list)
            else []
        )
        rows: list[Row] = [
            (
                "requests",
                {
                    **key,
                    "message": str(step_index),
                    "time": time,
                    "model": model if isinstance(model, str) else None,
                    "stop_reason": "tool_use" if tool_calls else "end_turn",
                    "input_tokens": input_tokens,
                    "cache_write_tokens": 0,
                    "cache_write_1h_tokens": 0,
                    "cache_read_tokens": cache_read,
                    "output_tokens": output_tokens,
                    "context_tokens": input_tokens + cache_read,
                },
            )
        ]
        for offset, tc in enumerate(tool_calls):
            name = str(tc.get("name") or "")
            raw_args = tc.get("args")
            decoded: dict[str, object] = (
                {str(k): decode_arg(v) for k, v in raw_args.items()}
                if isinstance(raw_args, dict)
                else {}
            )
            call = str(step_index + 1 + offset)
            command = tool_target(name, decoded)
            rows.append(
                (
                    "tool_calls",
                    {
                        **key,
                        "call": call,
                        "start": time,
                        "tool": "Bash" if name == "run_command" else name,
                        "command": command,
                    },
                )
            )
            raw_cmd = decoded.get("CommandLine") if name == "run_command" else None
            if isinstance(raw_cmd, str):
                rows += [
                    (
                        "enact_events",
                        {**key, "call": call, "verb": verb, "bead": bead, "time": time},
                    )
                    for verb, bead in claude.enact_commands(raw_cmd)
                ]
        return rows

    def _generic(
        self,
        key: dict[str, object],
        step_index: int,
        created: str,
        record: dict[str, object],
    ) -> list[Row]:
        content = record.get("content")
        text = content if isinstance(content, str) else ""
        call = str(step_index)
        end = completed_at(text, created)
        rows: list[Row] = [
            (
                "tool_results",
                {
                    **key,
                    "call": call,
                    "end": end,
                    "result_bytes": len(text.encode("utf-8")),
                    "compile_seconds": claude.compile_seconds({"stdout": text}),
                    "deferred": claude.deferred({"stdout": text}),
                },
            )
        ]
        found = claude.NEXT.search(text)
        if found:
            rows.append(
                (
                    "enact_events",
                    {
                        **key,
                        "call": call,
                        "verb": "next",
                        "bead": found[1],
                        "time": end,
                    },
                )
            )
        return rows

    def finish(self, db: windows.Connection, changed: list[Path]) -> int:
        if not self._meta:
            self._meta = load_metadata(self.root)
        by_provider: dict[str, list[tuple[str, str | None]]] = {
            provider: [] for provider in PROVIDERS
        }
        for path in changed:
            provider = self._provider_of(path)
            if provider is None:
                continue
            by_provider.setdefault(provider, []).append(
                (conversation_of(path), self._parent_of(path))
            )
        written = 0
        for provider, found in by_provider.items():
            if found:
                written += windows.derive(db, self.host, provider, found)
            tops = sorted({parent or transcript for transcript, parent in found})
            written += timing.refresh(db, self.host, provider, tops)
            written += tokens.refresh(db, self.host, provider, tops)
        return written
