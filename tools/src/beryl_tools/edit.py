"""`just edit <path>`: edit one file in an Ignite build tree, format it and land it.

The main checkout stays clean so Ignite can fast-forward it, which makes a quick edit to one
file tedious by hand: setting `IGNITE_SESSION`, claiming an ad hoc ticket with `ignite start`,
opening the file in the leased tree, running its formatters and any derived-file generator,
committing with a Conventional Commit subject, and landing (or closing the ticket when nothing
changed). This command runs that whole loop in one step.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import shlex
import shutil
import subprocess
import sys
import tomllib
import uuid
from collections.abc import Callable, Sequence
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from . import paths, system
from .ignite import rules as check_rules

# Exit code Ignite returns when an operation is still running and names a handle to wait on.
IGNITE_RUNNING = 5

# What `ignite land` or `ignite start` prints when the caller should follow up with `ignite wait`.
WAIT_HANDLE: re.Pattern[str] = re.compile(r"\bignite wait ([A-Za-z0-9_.-]+)")

# Derived files whose generators must run when their source data changes (`repository` family).
GENERATORS: dict[str, tuple[str, ...]] = {
    "data/ui-conventions.json": ("conventions",),
    "data/arena-motion.json": ("conventions",),
}

# The subject Ignite's landing formatter commits under, when a landing adds a format commit.
FORMAT_COMMIT_SUBJECT = "style: format"


class EditError(Exception):
    """A failure that stops `just edit` with a plain message for the user."""


@dataclass(frozen=True)
class FormatRule:
    """One entry of `[format].rules` in `ignite.toml`."""

    paths: tuple[str, ...]
    exclude: tuple[str, ...]
    dir: str
    command: str

    def matches(self, path: str) -> bool:
        return any(path_matches(p, path) for p in self.paths) and not any(
            path_matches(p, path) for p in self.exclude
        )

    def words(self, files: Sequence[str]) -> list[str]:
        """The rule's command split on whitespace, with `{files}` expanded relative to `dir`."""
        base: str = self.dir.strip("/")
        climb: str = "../" * len(base.split("/")) if base else ""

        def relative(file: str) -> str:
            if not base:
                return file
            prefix = f"{base}/"
            if file.startswith(prefix):
                return file[len(prefix) :]
            return f"{climb}{file}"

        out: list[str] = []
        for word in self.command.split():
            if word == "{files}":
                out.extend(relative(file) for file in files)
            else:
                out.append(word)
        return out


def _compile_glob(pattern: str) -> re.Pattern[str]:
    """Compiles an Ignite path glob (`*`, `?`, `**`) to a full-path regular expression."""
    out: list[str] = ["^"]
    i = 0
    while i < len(pattern):
        if pattern.startswith("**/", i):
            out.append("(?:.*/)?")
            i += 3
        elif pattern.startswith("/**", i):
            out.append("/.*")
            i += 3
        elif pattern.startswith("**", i):
            out.append(".*")
            i += 2
        elif pattern[i] == "*":
            out.append("[^/]*")
            i += 1
        elif pattern[i] == "?":
            out.append("[^/]")
            i += 1
        else:
            out.append(re.escape(pattern[i]))
            i += 1
    out.append("$")
    return re.compile("".join(out))


def path_matches(pattern: str, path: str) -> bool:
    """Whether `path` (POSIX, relative to checkout root) matches Ignite's glob `pattern`."""
    return _compile_glob(pattern).match(path) is not None


def load_format_rules(tree: Path) -> list[FormatRule]:
    """Reads `[format].rules` from `tree / "ignite.toml"`, or returns none if absent."""
    config = tree / "ignite.toml"
    if not config.is_file():
        return []
    data: dict[str, Any] = tomllib.loads(config.read_text(encoding="utf-8"))
    raw_rules = data.get("format", {}).get("rules", [])
    return [
        FormatRule(
            paths=tuple(str(p) for p in entry.get("paths", ())),
            exclude=tuple(str(p) for p in entry.get("exclude", ())),
            dir=str(entry.get("dir", "")),
            command=str(entry.get("command", "")),
        )
        for entry in raw_rules
        if entry.get("command")
    ]


def resolve_path(raw: str, repo: Path, cwd: Path) -> str:
    """Resolves `raw` (relative to `cwd`, or absolute inside `repo`) to a repo-relative POSIX
    path, rejecting directories and git/worktree internals."""
    if not raw.strip():
        raise EditError("path cannot be empty")
    repo_root = repo.resolve()
    target = (cwd / raw).resolve()
    try:
        rel = target.relative_to(repo_root)
    except ValueError as error:
        raise EditError(f"{raw!r} is outside the repository ({repo_root})") from error
    parts = rel.parts
    if not parts:
        raise EditError("path names the repository root, not a file")
    if parts[0] in (".git", ".worktrees"):
        raise EditError(f"cannot edit internal path {rel.as_posix()!r}")
    if target.is_dir():
        raise EditError(f"{rel.as_posix()} is a directory")
    return rel.as_posix()


def file_exists_in_repo(repo: Path, rel: str) -> bool:
    """Whether `rel` exists on disk in `repo` or in Ignite's newest fetched refs."""
    if (repo / rel).is_file():
        return True
    for ref in ("origin/ignite/green", "origin/ignite/staging"):
        probe = subprocess.run(
            ["git", "-C", str(repo), "cat-file", "-e", f"{ref}:{rel}"],
            capture_output=True,
            check=False,
        )
        if probe.returncode == 0:
            return True
    return False


def resolve_editor(
    explicit: str | None,
    repo: Path,
    env: dict[str, str] | None = None,
) -> list[str]:
    """Picks the editor command: `--editor`, `$VISUAL`, `$EDITOR`, git `core.editor`, else
    `notepad` on Windows or `vi` on POSIX."""
    environ = os.environ if env is None else env
    raw = explicit or environ.get("VISUAL") or environ.get("EDITOR")
    if not raw:
        probe = subprocess.run(
            ["git", "-C", str(repo), "config", "--get", "core.editor"],
            capture_output=True,
            text=True,
            check=False,
        )
        if probe.returncode == 0 and probe.stdout.strip():
            raw = probe.stdout.strip()
    if not raw:
        raw = "notepad" if system.WINDOWS else "vi"
    words = shlex.split(raw, posix=not system.WINDOWS)
    if not words:
        raise EditError("editor command is empty")
    program = words[0]
    if not (Path(program).is_file() or shutil.which(program)):
        raise EditError(f"editor not found: {program}")
    return words


def default_prefix(rel: str) -> str:
    """A Conventional Commit type prefix suited to `rel`."""
    if rel.endswith(".md"):
        return "docs"
    name = Path(rel).name
    if (
        "/tests/" in f"/{rel}"
        or rel.endswith(("_tests.rs", ".test.ts", ".test.tsx"))
        or name.startswith("test_")
    ):
        return "test"
    return "chore"


def default_subject(rel: str) -> str:
    """A valid Conventional Commit subject (<= 60 chars) for editing `rel`."""
    prefix = default_prefix(rel)
    candidate = f"{prefix}: edit {rel}"
    if len(candidate) <= check_rules.COMMIT_MAX:
        return candidate
    short = f"{prefix}: edit {Path(rel).name}"
    return short[: check_rules.COMMIT_MAX]


def normalize_subject(raw: str, rel: str) -> str:
    """Turns a user-provided commit message into a valid Conventional Commit subject of at most
    60 characters, prepending `default_prefix(rel)` when the user omitted a type prefix.
    """
    text = " ".join(raw.strip().splitlines()).strip()
    if not text:
        return default_subject(rel)
    if not check_rules.COMMIT.match(text):
        text = f"{default_prefix(rel)}: {text}"
    if not check_rules.COMMIT.match(text):
        raise EditError(f"commit subject {text!r} is not a Conventional Commit")
    if len(text) > check_rules.COMMIT_MAX:
        raise EditError(
            f"commit subject is {len(text)} characters (max {check_rules.COMMIT_MAX}): {text!r}"
        )
    return text


def git_changed_files(tree: Path) -> list[str]:
    """Every modified, staged, deleted or untracked file in `tree`, relative to `tree`."""
    out = subprocess.run(
        [
            "git",
            "-C",
            str(tree),
            "status",
            "--porcelain",
            "-z",
            "--untracked-files=all",
        ],
        capture_output=True,
        check=True,
    ).stdout
    entries = out.decode("utf-8", errors="surrogateescape").split("\0")
    files: list[str] = []
    i = 0
    while i < len(entries):
        entry = entries[i]
        i += 1
        if len(entry) < 4:
            continue
        status = entry[:2]
        files.append(entry[3:])
        if status[0] in ("R", "C") or status[1] in ("R", "C"):
            i += 1
    return files


def tool_env(base_env: dict[str, str] | None = None) -> dict[str, str]:
    """Environment for running `uv run --project tools` inside a leased build tree: drops
    `VIRTUAL_ENV` so `uv` uses the tree's own environment instead of the main checkout's.
    """
    env = dict(os.environ if base_env is None else base_env)
    env.pop("VIRTUAL_ENV", None)
    return env


def run_generators(tree: Path, edited: str) -> list[str]:
    """Runs any derived-file generator registered for `edited`, returning failure messages."""
    commands = GENERATORS.get(edited, ())
    errors: list[str] = []
    env = tool_env()
    for subcommand in commands:
        cmd = ["uv", "run", "--quiet", "--project", "tools", "by", subcommand]
        print(f"Running generator: by {subcommand}")
        result = subprocess.run(cmd, cwd=tree, env=env, check=False)
        if result.returncode != 0:
            errors.append(f"`by {subcommand}` exited {result.returncode}")
    return errors


def run_formatters(tree: Path, changed: Sequence[str]) -> list[str]:
    """Runs every `[format]` rule in `tree / "ignite.toml"` that matches any existing file in
    `changed`. Returns failure messages (empty when all succeeded)."""
    rules = load_format_rules(tree)
    errors: list[str] = []
    env = tool_env()
    for rule in rules:
        matched = [f for f in changed if rule.matches(f) and (tree / f).is_file()]
        if not matched:
            continue
        workdir = tree / rule.dir if rule.dir else tree
        words = rule.words(matched)
        if not words:
            continue
        if system.WINDOWS:
            words[0] = system.which(words[0])
        print(f"Formatting: {' '.join(words)}")
        try:
            result = subprocess.run(words, cwd=workdir, env=env, check=False)
        except OSError as error:
            errors.append(f"`{rule.command}` could not start: {error}")
            continue
        if result.returncode != 0:
            errors.append(f"`{rule.command}` exited {result.returncode}")
    return errors


def prepare_changes(tree: Path, edited: str) -> list[str]:
    """Runs derived-file generators for `edited` and then all matching formatters over every
    changed file in `tree`. Returns failure messages."""
    errors = run_generators(tree, edited)
    if errors:
        return errors
    changed = git_changed_files(tree)
    if not changed:
        return []
    return run_formatters(tree, changed)


def _stream_command(cmd: list[str], cwd: Path, env: dict[str, str]) -> tuple[int, str]:
    """Runs `cmd`, streaming its combined output live while also returning the full text."""
    proc = subprocess.Popen(
        cmd,
        cwd=cwd,
        env=env,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    chunks: list[str] = []
    stdout = proc.stdout
    assert stdout is not None
    for line in stdout:
        sys.stdout.write(line)
        sys.stdout.flush()
        chunks.append(line)
    return proc.wait(), "".join(chunks)


def _ignite_wait(handle: str, cwd: Path, env: dict[str, str]) -> tuple[int, str]:
    """Waits on a running Ignite handle, streaming steps until it exits with a final code."""
    current = handle
    while True:
        code, output = _stream_command(["ignite", "wait", current], cwd, env)
        if code != IGNITE_RUNNING:
            return code, output
        match = WAIT_HANDLE.search(output)
        if not match:
            return code, output
        current = match.group(1)


def start_tree(repo: Path, rel: str, env: dict[str, str]) -> tuple[str, Path]:
    """Claims an ad hoc Ignite ticket for editing `rel` and leases a build tree, waiting if all
    trees are busy."""
    summary = f"edit {rel}"
    while True:
        try:
            run = subprocess.run(
                ["ignite", "start", "--json", summary],
                cwd=repo,
                env=env,
                capture_output=True,
                text=True,
                check=False,
            )
        except FileNotFoundError as error:
            raise EditError("ignite is not on PATH") from error
        raw = run.stdout.strip()
        data: dict[str, Any] = {}
        if raw:
            try:
                parsed = json.loads(raw)
                if isinstance(parsed, dict):
                    data = parsed
            except json.JSONDecodeError:
                pass
        if run.returncode == 0 and "path" in data:
            ticket = str(data.get("ticket", {}).get("id", ""))
            return ticket, Path(str(data["path"]))
        if run.returncode == IGNITE_RUNNING and isinstance(data.get("handle"), str):
            handle = data["handle"]
            print(f"All build trees are busy; waiting on {handle}...")
            wait_code, _ = _ignite_wait(handle, repo, env)
            if wait_code != 0:
                raise EditError(f"waiting for a build tree failed (exit {wait_code})")
            continue
        detail = (
            data.get("text") or raw or run.stderr.strip() or f"exit {run.returncode}"
        )
        raise EditError(f"ignite start failed:\n{detail}")


def close_tree(tree: Path, env: dict[str, str], reason: str) -> None:
    """Closes the leased ticket and frees `tree` without landing."""
    run = subprocess.run(
        ["ignite", "close", "-m", reason],
        cwd=tree,
        env=env,
        capture_output=True,
        text=True,
        check=False,
    )
    if run.returncode != 0:
        detail = (run.stdout + run.stderr).strip()
        print(f"warning: ignite close failed: {detail}", file=sys.stderr)


def land_tree(tree: Path, env: dict[str, str], note: str) -> int:
    """Runs `ignite land -m <note>` in `tree`, streaming steps and following `ignite wait` if
    the landing outlives the client's wait window."""
    code, output = _stream_command(["ignite", "land", "-m", note], tree, env)
    if code == IGNITE_RUNNING:
        match = WAIT_HANDLE.search(output)
        if match:
            code, _ = _ignite_wait(match.group(1), tree, env)
    return code


def commit_changes(tree: Path, subject: str, *, amend: bool) -> bool:
    """Stages all changes in `tree` and commits them as `subject`, amending cleanly on a retry
    (first folding back any `style: format` commit an earlier failed landing added). Returns
    False when amending leaves no net diff from the commit before ours."""
    if amend:
        head_subject = subprocess.run(
            ["git", "-C", str(tree), "log", "-1", "--format=%s"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout.strip()
        if head_subject == FORMAT_COMMIT_SUBJECT:
            subprocess.run(
                ["git", "-C", str(tree), "reset", "--soft", "HEAD~1"],
                check=True,
            )
            head_subject = subprocess.run(
                ["git", "-C", str(tree), "log", "-1", "--format=%s"],
                capture_output=True,
                text=True,
                check=True,
            ).stdout.strip()
        should_amend = head_subject == subject
    else:
        should_amend = False

    changed = git_changed_files(tree)
    if changed:
        subprocess.run(
            ["git", "-C", str(tree), "add", "-A", "--", *changed], check=True
        )
    if should_amend:
        empty = subprocess.run(
            ["git", "-C", str(tree), "diff", "--cached", "--quiet", "HEAD~1"],
            check=False,
        )
        if empty.returncode == 0:
            return False
        subprocess.run(
            ["git", "-C", str(tree), "commit", "--amend", "--no-edit"], check=True
        )
    else:
        subprocess.run(["git", "-C", str(tree), "commit", "-m", subject], check=True)
    return True


def show_diff_summary(tree: Path) -> None:
    """Prints a compact summary of pending changes in `tree`."""
    status = subprocess.run(
        ["git", "-C", str(tree), "status", "--short"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout.strip()
    if status:
        print(status)
    diff = subprocess.run(
        ["git", "-C", str(tree), "diff", "--stat"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout.strip()
    if diff:
        print(diff)


def prompt_subject(rel: str, ask: Callable[[str], str] = input) -> str | None:
    """Asks the user for a commit message; returns None if the user aborts."""
    fallback = default_subject(rel)
    while True:
        try:
            raw = ask(f"Commit message [{fallback}] ('q' to abort): ")
        except EOFError:
            return None
        trimmed = raw.strip()
        if trimmed.lower() in ("q", "quit", "abort"):
            return None
        try:
            return normalize_subject(trimmed, rel)
        except EditError as error:
            print(f"edit: {error}", file=sys.stderr)


def prompt_choice(
    prompt: str,
    valid: Sequence[str],
    default: str,
    ask: Callable[[str], str] = input,
) -> str:
    """Asks a single-letter interactive question, returning `default` on blank input or `'a'` on
    EOF."""
    if not sys.stdin.isatty() and ask is input:
        return "a"
    while True:
        try:
            raw = ask(prompt).strip().lower()
        except EOFError:
            return "a"
        choice = raw or default
        if choice in valid:
            return choice
        print(f"Choose one of: {', '.join(valid)}", file=sys.stderr)


def edit_file(
    raw_path: str,
    *,
    repo: Path = paths.REPO,
    cwd: Path | None = None,
    message: str | None = None,
    editor: str | None = None,
    allow_new: bool = False,
    ask: Callable[[str], str] = input,
) -> int:
    """Runs the full `just edit` workflow and returns a process exit code."""
    rel = resolve_path(raw_path, repo, Path.cwd() if cwd is None else cwd)
    if not allow_new and not file_exists_in_repo(repo, rel):
        raise EditError(f"{rel} does not exist (pass --new to create it)")
    editor_cmd = resolve_editor(editor, repo)
    preset_subject = normalize_subject(message, rel) if message is not None else None

    session = f"edit-{uuid.uuid4().hex[:12]}"
    env = {**os.environ, "IGNITE_SESSION": session}
    ticket, tree = start_tree(repo, rel, env)
    print(f"Ticket {ticket} in {tree}")

    subject = preset_subject
    committed = False
    open_editor = True

    try:
        while True:
            if open_editor:
                target = tree / rel
                if allow_new:
                    target.parent.mkdir(parents=True, exist_ok=True)
                edit_run = subprocess.run([*editor_cmd, str(target)], cwd=tree)
                if edit_run.returncode != 0:
                    print(
                        f"warning: editor exited with status {edit_run.returncode}",
                        file=sys.stderr,
                    )

                changed = git_changed_files(tree)
                if not changed and not committed:
                    print("No changes; closing ticket and freeing build tree.")
                    close_tree(tree, env, "no changes")
                    return 0

                if changed:
                    fmt_errors = prepare_changes(tree, rel)
                    if fmt_errors:
                        for err in fmt_errors:
                            print(f"edit: {err}", file=sys.stderr)
                        action = prompt_choice(
                            "Formatting failed. [e]dit again, [c]ontinue anyway, or [a]bort? [e]: ",
                            ("e", "c", "a"),
                            "e",
                            ask=ask,
                        )
                        if action == "e":
                            continue
                        if action == "a":
                            close_tree(tree, env, "aborted after formatting error")
                            return 1

                    show_diff_summary(tree)
                    if subject is None:
                        subject = prompt_subject(rel, ask=ask)
                        if subject is None:
                            print("Aborted; closing ticket and freeing build tree.")
                            close_tree(tree, env, "aborted")
                            return 1

                    if not commit_changes(tree, subject, amend=committed):
                        print(
                            "No net changes left; closing ticket and freeing build tree."
                        )
                        close_tree(tree, env, "no changes")
                        return 0
                    committed = True

            assert subject is not None
            land_code = land_tree(tree, env, subject)
            if land_code == 0:
                return 0

            action = prompt_choice(
                "Landing failed. [e]dit again, [r]etry land, [k]eep tree, or [a]bort? [e]: ",
                ("e", "r", "k", "a"),
                "e",
                ask=ask,
            )
            if action == "e":
                open_editor = True
                continue
            if action == "r":
                open_editor = False
                continue
            if action == "k":
                print(
                    f"Keeping ticket {ticket} in {tree}.\n"
                    f"Resume with: cd {shlex.quote(str(tree))} && "
                    f"IGNITE_SESSION={session} ignite land"
                )
                return land_code
            close_tree(tree, env, "aborted after failed landing")
            return land_code
    except KeyboardInterrupt:
        print("\nInterrupted; closing ticket and freeing build tree.", file=sys.stderr)
        close_tree(tree, env, "interrupted")
        return 130
    return 1


def run(args: argparse.Namespace) -> int:
    try:
        return edit_file(
            args.path,
            message=args.message,
            editor=args.editor,
            allow_new=args.new,
        )
    except EditError as error:
        print(f"edit: {error}", file=sys.stderr)
        return 1


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "edit",
        help="Edit one file in an Ignite build tree, format it and land it",
    )
    p.add_argument("path", help="Relative path of the file to edit")
    p.add_argument(
        "-m",
        "--message",
        default=None,
        help="Commit subject (Conventional Commit, max 60 chars; default prefix added if omitted)",
    )
    p.add_argument(
        "-e",
        "--editor",
        default=None,
        help="Editor command (default: $VISUAL, $EDITOR, git core.editor, or vi)",
    )
    p.add_argument(
        "--new",
        action="store_true",
        help="Allow creating a file that does not exist yet",
    )
    p.set_defaults(func=run)
