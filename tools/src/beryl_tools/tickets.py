"""Use Enact's active backend and its installed native ticket client."""

from __future__ import annotations

import argparse
import json
import subprocess
import sys
import tomllib
from pathlib import Path
from typing import Any

from . import paths


class TicketError(RuntimeError):
    pass


def detect(repo: Path) -> str:
    config = (paths.main_checkout(repo) or repo) / ".enact.toml"
    if not config.is_file():
        return ""
    with config.open("rb") as file:
        settings = tomllib.load(file).get("tickets", {})
    if not isinstance(settings, dict):
        raise TicketError("expected a [tickets] table")
    if not settings.get("project") and settings.get("backend") != "server":
        return ""

    # Enact owns worktree config precedence, cutover waits and the client cache.
    helper = Path.home() / "enact" / "scripts" / "enact.py"
    result = subprocess.run(
        [sys.executable, str(helper), "backend"],
        cwd=repo,
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode:
        raise TicketError((result.stderr or result.stdout).strip())
    data = json.loads(result.stdout)
    if data.get("backend") == "beads":
        return ""
    if data.get("backend") == "server" and isinstance(data.get("tk"), str):
        return data["tk"]
    raise TicketError("Enact returned an invalid backend")


def client() -> str:
    return detect(paths.REPO)


def invoke(
    executable: str,
    *args: str,
    input_text: str | None = None,
) -> str:
    result = subprocess.run(
        [executable, "--json", *args],
        cwd=paths.REPO,
        input=input_text,
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
    )
    if result.returncode:
        raise TicketError(f"tk {args[0]}: {(result.stderr or result.stdout).strip()}")
    return result.stdout


def list_issues(executable: str, *filters: str) -> list[dict[str, Any]]:
    # With no --limit all matching tickets are returned. tk's --limit 0 returns none.
    return json.loads(invoke(executable, "list", *filters))


def create_issue(
    executable: str,
    fields: dict[str, object],
    *,
    parent: str = "",
    request_key: str = "",
) -> str:
    # tk create does not expose metadata or closed status.
    operation: dict[str, object] = {"op": "create", "fields": fields}
    if parent:
        operation["parent"] = parent
    if request_key:
        operation["request_key"] = request_key
    response = json.loads(
        invoke(executable, "batch", input_text=json.dumps({"ops": [operation]}))
    )
    return str(response["results"][0]["id"])


def backend_cmd(_: argparse.Namespace) -> int:
    try:
        executable = client()
        print(
            json.dumps(
                {"backend": "server", "tk": executable}
                if executable
                else {"backend": "beads"}
            )
        )
    except (RuntimeError, OSError, ValueError) as error:
        print(f"ticket-backend: {error}", file=sys.stderr)
        return 1
    return 0


TOP_LEVEL_EXCLUDED = (
    "bug-report",
    "flaky-check",
    "ci-cut",
    "check-budget",
    "sweep-failure",
    "nightly-failure",
    "role:steward",
)


def top_level(_: argparse.Namespace) -> int:
    try:
        executable = client()
        if executable:
            issues = [
                issue
                for issue in list_issues(executable)
                if not issue.get("parent")
                and not set(TOP_LEVEL_EXCLUDED).intersection(issue.get("labels") or [])
            ]
        else:
            result = subprocess.run(
                [
                    "bd",
                    "list",
                    "--no-parent",
                    "--exclude-label",
                    ",".join(TOP_LEVEL_EXCLUDED),
                    "--flat",
                    "-n",
                    "0",
                    "--json",
                ],
                cwd=paths.REPO,
                stdout=subprocess.PIPE,
                text=True,
                check=False,
            )
            if result.returncode:
                return result.returncode
            issues = json.loads(result.stdout)
        for issue in issues:
            print(issue["id"], issue["title"])
    except (RuntimeError, OSError, ValueError) as error:
        print(f"beads: {error}", file=sys.stderr)
        return 1
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    sub.add_parser(
        "ticket-backend", help="show the active tracker and native client path"
    ).set_defaults(func=backend_cmd)
    sub.add_parser(
        "beads", help="list top-level work in the active tracker"
    ).set_defaults(func=top_level)
