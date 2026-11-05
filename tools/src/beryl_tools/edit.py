"""`by edit FILE`: change one file and deliver it without leaving the terminal. It cuts an
Enact worktree from staging, opens the file there in $EDITOR, formats it the way `just fmt`
would, commits it with a generic message and runs `enact push`.

The worktree is named after the file (`edit-docs-benchmarking-md`). When a push fails its
checks, the worktree stays, and running `by edit` on the same file again reopens it there.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path

from . import checks, paths, warm

ENACT: list[str] = ["python3", str(Path.home() / "enact" / "scripts" / "enact.py")]


def git(cwd: Path, *args: str) -> str:
    run = subprocess.run(
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=False
    )
    return run.stdout.strip() if run.returncode == 0 else ""


def enact(cwd: Path, *args: str) -> tuple[int, dict[str, object]]:
    """Runs an Enact command: its logs go to stderr as they happen, its JSON comes back."""
    run = subprocess.run(
        [*ENACT, *args], cwd=cwd, stdout=subprocess.PIPE, text=True, check=False
    )
    try:
        data = json.loads(run.stdout)
    except ValueError:
        data = {}
    return run.returncode, data if isinstance(data, dict) else {}


def locate(file: Path) -> tuple[Path, str]:
    """The checkout `file` is in (the main one or a worktree) and its path there. The file
    need not exist yet."""
    path = (Path.cwd() / file).resolve()
    if path.is_dir():
        raise SystemExit(f"edit: {file} is a directory")
    probe = path.parent
    while not probe.is_dir():
        probe = probe.parent
    top = git(probe, "rev-parse", "--show-toplevel")
    common = git(probe, "rev-parse", "--path-format=absolute", "--git-common-dir")
    if not top or Path(common).parent != paths.main_checkout():
        raise SystemExit(f"edit: {file} is not in this repository")
    return Path(common).parent, path.relative_to(top).as_posix()


def worktree_name(rel: str) -> str:
    return "edit-" + re.sub(r"[^a-z0-9]+", "-", rel.lower()).strip("-")[:40].strip("-")


def subject(rel: str, new: bool) -> str:
    kind = "docs" if rel.endswith(".md") else "chore"
    return f"{kind}: {'add' if new else 'update'} {Path(rel).name}"[: checks.COMMIT_MAX]


def existing_worktree(main: Path, branch: str) -> Path | None:
    tree = None
    for line in git(main, "worktree", "list", "--porcelain").splitlines():
        if line.startswith("worktree "):
            tree = Path(line.removeprefix("worktree "))
        elif line == f"branch refs/heads/{branch}":
            return tree
    return None


def formatter(tree: Path, rel: str) -> tuple[list[str], Path] | None:
    """The command, and where to run it, that formats `rel` as the checks expect."""
    if rel.startswith((".beads/", ".agents/")):
        return None
    if rel.endswith(".md"):
        return [sys.executable, "-m", "mdformat", rel], tree
    if rel.startswith("tools/") and rel.endswith(".py"):
        return [sys.executable, "-m", "black", "--quiet", rel], tree
    if rel.startswith("rust/") and rel.endswith(".rs"):
        return ["cargo", "fmt", "--all"], tree / "rust"
    if rel == "justfile":
        return ["just", "--fmt", "--unstable"], tree
    if rel.startswith("ui/"):
        prettier = tree / "ui" / "node_modules" / ".bin" / "prettier"
        return [
            str(prettier),
            *("--write", "--ignore-unknown", "--log-level", "warn"),
            rel.removeprefix("ui/"),
        ], tree / "ui"
    return None


def open_editor(target: Path) -> int:
    editor = os.environ.get("EDITOR") or os.environ.get("VISUAL") or "vi"
    # Through the shell, like git, so that EDITOR="code --wait" works.
    return subprocess.run(
        ["sh", "-c", f'{editor} "$@"', editor, str(target)], check=False
    ).returncode


def edit(args: argparse.Namespace) -> int:
    main, rel = locate(Path(args.file))
    name = worktree_name(rel)
    tree = existing_worktree(main, f"enact/{name}")
    if tree:
        print(f"edit: reopening {tree}", file=sys.stderr)
    else:
        code, made = enact(main, "worktree", "create", name)
        if code != 0:
            return code
        tree = Path(str(made["path"]))
    target = tree / rel
    target.parent.mkdir(parents=True, exist_ok=True)
    new = not target.exists()
    if (code := open_editor(target)) != 0:
        print(f"edit: the editor exited with {code}; {tree} is kept", file=sys.stderr)
        return code
    if git(tree, "status", "--porcelain", "--", rel) and (fmt := formatter(tree, rel)):
        if rel.startswith("ui/"):
            warm.ensure(tree)
        subprocess.run(fmt[0], cwd=fmt[1], check=False)
    if git(tree, "status", "--porcelain", "--", rel):
        git(tree, "add", "--", rel)
        commit = ["git", "commit", "-q", "-m", subject(rel, new)]
        if subprocess.run(commit, cwd=tree, check=False).returncode != 0:
            print(f"edit: could not commit {rel}; {tree} is kept", file=sys.stderr)
            return 1
    elif git(tree, "rev-list", "--count", "origin/staging..HEAD") == "0":
        code, _ = enact(main, "worktree", "remove", name)
        print(f"edit: {rel} is unchanged; nothing to push", file=sys.stderr)
        return code
    code, pushed = enact(tree, "push")
    if code == 0 and pushed.get("pushed"):
        sha = str(pushed.get("staging", ""))
        ci = f"; slow CI is running (`enact ci --wait {sha}` to watch)"
        print(f"edit: pushed {sha[:8]} to staging{ci if 'ci' in pushed else ''}")
    elif code == 0:
        print(f"edit: {pushed.get('reason', 'nothing pushed')}")
    elif code == 2:
        print(
            f"edit: {tree} is kept: fix it there, or run `just edit {args.file}` again",
            file=sys.stderr,
        )
    return code


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "edit", help="edit one file in $EDITOR, then commit and `enact push` it"
    )
    p.add_argument("file", help="relative to the current directory; may be new")
    p.set_defaults(func=edit)
