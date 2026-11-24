"""Tests for `just edit` (`beryl_tools.edit`): path checks, formatters, commit rules and the
Ignite start/edit/format/land/close loop."""

from __future__ import annotations

import json
import subprocess
import sys
from collections.abc import Iterator
from pathlib import Path
from typing import Any

import pytest

from beryl_tools import cli, edit


def init_repo(root: Path, files: dict[str, str]) -> Path:
    """Creates a committed git repository at `root` with `files`."""
    root.mkdir(parents=True, exist_ok=True)
    for rel, text in files.items():
        path = root / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")
    subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
    subprocess.run(["git", "config", "user.name", "Test"], cwd=root, check=True)
    subprocess.run(
        ["git", "config", "user.email", "test@example.com"], cwd=root, check=True
    )
    subprocess.run(["git", "add", "-A"], cwd=root, check=True)
    subprocess.run(
        ["git", "commit", "-q", "-m", "chore: initial"], cwd=root, check=True
    )
    return root


def git_log_subjects(repo: Path) -> list[str]:
    return (
        subprocess.run(
            ["git", "-C", str(repo), "log", "--format=%s"],
            capture_output=True,
            text=True,
            check=True,
        )
        .stdout.strip()
        .splitlines()
    )


def python_script_cmd(script: Path) -> str:
    """An `--editor` command string that runs `script` with the active Python interpreter."""
    return (
        f'"{sys.executable}" "{script}"'
        if sys.platform == "win32"
        else f"{sys.executable} {script}"
    )


class FakeIgnite:
    """Intercepts `ignite` subprocess calls while forwarding `git`, editors and formatters."""

    def __init__(
        self,
        tree: Path,
        *,
        ticket: str = "TEDIT1",
        start_waits: int = 0,
        land_codes: list[int] | None = None,
        land_outputs: list[str] | None = None,
        wait_codes: list[int] | None = None,
        on_land: Any = None,
    ) -> None:
        self.tree: Path = tree
        self.ticket: str = ticket
        self.start_waits: int = start_waits
        self.land_codes: list[int] = list(land_codes if land_codes is not None else [0])
        self.land_outputs: list[str] = list(land_outputs or [])
        self.wait_codes: list[int] = list(wait_codes or [0])
        self.on_land: Any = on_land
        self.calls: list[tuple[list[str], str | None]] = []
        self.closed_reasons: list[str] = []

    def install(self, monkeypatch: pytest.MonkeyPatch) -> None:
        real_run: Any = subprocess.run
        real_stream: Any = edit._stream_command

        def fake_run(cmd: Any, *args: Any, **kwargs: Any) -> Any:
            if isinstance(cmd, list) and cmd and cmd[0] == "ignite":
                return self._handle_run(cmd, kwargs.get("env") or {})
            return real_run(cmd, *args, **kwargs)

        def fake_stream(
            cmd: list[str], cwd: Path, env: dict[str, str]
        ) -> tuple[int, str]:
            if cmd and cmd[0] == "ignite":
                return self._handle_stream(cmd, env)
            return real_stream(cmd, cwd, env)

        monkeypatch.setattr(subprocess, "run", fake_run)
        monkeypatch.setattr(edit, "_stream_command", fake_stream)

    def _handle_run(
        self, cmd: list[str], env: dict[str, str]
    ) -> subprocess.CompletedProcess[str]:
        session = env.get("IGNITE_SESSION")
        self.calls.append((cmd, session))
        verb = cmd[1]
        if verb == "start":
            if self.start_waits > 0:
                self.start_waits -= 1
                payload = json.dumps({"waiting": "tree", "handle": "S-1"})
                return subprocess.CompletedProcess(
                    cmd, edit.IGNITE_RUNNING, payload, ""
                )
            payload = json.dumps(
                {"ticket": {"id": self.ticket}, "path": str(self.tree)}
            )
            return subprocess.CompletedProcess(cmd, 0, payload, "")
        if verb == "close":
            reason = cmd[cmd.index("-m") + 1] if "-m" in cmd else ""
            self.closed_reasons.append(reason)
            return subprocess.CompletedProcess(cmd, 0, f"closed   {self.ticket}\n", "")
        raise AssertionError(f"unexpected ignite run command: {cmd}")

    def _handle_stream(self, cmd: list[str], env: dict[str, str]) -> tuple[int, str]:
        session = env.get("IGNITE_SESSION")
        self.calls.append((cmd, session))
        verb = cmd[1]
        if verb == "land":
            if self.on_land is not None:
                self.on_land(self.tree)
            code = self.land_codes.pop(0) if self.land_codes else 0
            out = self.land_outputs.pop(0) if self.land_outputs else "landed   ok\n"
            return code, out
        if verb == "wait":
            code = self.wait_codes.pop(0) if self.wait_codes else 0
            return code, "done\n"
        raise AssertionError(f"unexpected ignite stream command: {cmd}")


def test_edit_finds_a_file_landed_since_the_checkout_last_updated(
    tmp_path: Path,
) -> None:
    repo = init_repo(tmp_path / "repo", {"existing.txt": "existing"})
    (repo / "landed.txt").write_text("landed", encoding="utf-8")
    subprocess.run(["git", "add", "landed.txt"], cwd=repo, check=True)
    subprocess.run(
        ["git", "commit", "-qm", "feat: add landed file"], cwd=repo, check=True
    )
    subprocess.run(
        ["git", "update-ref", "refs/remotes/origin/staging", "HEAD"],
        cwd=repo,
        check=True,
    )
    subprocess.run(["git", "switch", "--detach", "HEAD^"], cwd=repo, check=True)

    assert not (repo / "landed.txt").exists()
    assert edit.file_exists_in_repo(repo, "landed.txt")


def test_resolve_path_handles_subdirectories_and_rejects_bad_targets(
    tmp_path: Path,
) -> None:
    repo = tmp_path / "repo"
    (repo / "docs").mkdir(parents=True)
    (repo / "docs" / "guide.md").write_text("hi\n")
    (repo / ".git").mkdir()

    assert edit.resolve_path("docs/guide.md", repo, repo) == "docs/guide.md"
    assert edit.resolve_path("guide.md", repo, repo / "docs") == "docs/guide.md"
    assert (
        edit.resolve_path(str(repo / "docs" / "guide.md"), repo, tmp_path)
        == "docs/guide.md"
    )

    with pytest.raises(edit.EditError, match="outside"):
        edit.resolve_path("../outside.txt", repo, repo)
    with pytest.raises(edit.EditError, match="directory"):
        edit.resolve_path("docs", repo, repo)
    with pytest.raises(edit.EditError, match="internal"):
        edit.resolve_path(".git/config", repo, repo)
    with pytest.raises(edit.EditError, match="internal"):
        edit.resolve_path(".worktrees/tree-01/a.rs", repo, repo)


def test_format_rule_matches_and_expands_files() -> None:
    md = edit.FormatRule(
        paths=("**/*.md",),
        exclude=(".agents/**",),
        dir="",
        command="mdformat {files}",
    )
    assert md.matches("AGENTS.md")
    assert md.matches("docs/ignite.md")
    assert not md.matches(".agents/skills/client/SKILL.md")
    assert not md.matches("docs/ignite.txt")

    ui = edit.FormatRule(
        paths=("ui/**",),
        exclude=(),
        dir="ui",
        command="prettier --write {files}",
    )
    assert ui.matches("ui/src/App.tsx")
    assert not ui.matches("ui-other/App.tsx")
    assert ui.words(["ui/src/App.tsx", "docs/a.md"]) == [
        "prettier",
        "--write",
        "src/App.tsx",
        "../docs/a.md",
    ]


def test_commit_subject_defaults_and_validation() -> None:
    assert edit.default_subject("AGENTS.md") == "docs: edit AGENTS.md"
    assert (
        edit.default_subject("tools/tests/test_edit.py")
        == "test: edit tools/tests/test_edit.py"
    )
    assert edit.default_subject("ignite.toml") == "chore: edit ignite.toml"

    long_path = "engine/crates/very_long_crate_name/src/deeply/nested/module/file.rs"
    assert edit.default_subject(long_path) == "chore: edit file.rs"
    assert len(edit.default_subject(long_path)) <= 60

    assert edit.normalize_subject("", "AGENTS.md") == "docs: edit AGENTS.md"
    assert edit.normalize_subject("tighten rule", "AGENTS.md") == "docs: tighten rule"
    assert (
        edit.normalize_subject("fix(ui): align bar", "ui/a.ts") == "fix(ui): align bar"
    )

    with pytest.raises(edit.EditError, match="max 60"):
        edit.normalize_subject("x" * 60, "AGENTS.md")


def test_edit_formats_commits_and_lands(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    formatter = tmp_path / "fmt.py"
    formatter.write_text(
        "import pathlib, sys\n"
        "for arg in sys.argv[1:]:\n"
        "    p = pathlib.Path(arg)\n"
        "    p.write_text(p.read_text().upper())\n",
        encoding="utf-8",
    )
    command = json.dumps(f"{sys.executable} {formatter.as_posix()} {{files}}")
    ignite_toml = (
        "[format]\n"
        "rules = [\n"
        f'  {{ paths = ["**/*.md"], exclude = [".agents/**"], command = {command} }},\n'
        '  { paths = ["**/*.rs"], command = "should-not-run {files}" },\n'
        "]\n"
    )
    repo = init_repo(
        tmp_path / "main", {"notes.md": "before\n", "ignite.toml": ignite_toml}
    )
    tree = init_repo(
        tmp_path / "tree", {"notes.md": "before\n", "ignite.toml": ignite_toml}
    )

    leaked = tmp_path / "editor_env.txt"
    editor = tmp_path / "editor.py"
    editor.write_text(
        "import os, pathlib, sys\n"
        f"pathlib.Path({str(leaked)!r}).write_text(os.environ.get('IGNITE_SESSION', ''))\n"
        "p = pathlib.Path(sys.argv[1])\n"
        "p.write_text('after edit\\n')\n",
        encoding="utf-8",
    )

    fake = FakeIgnite(
        tree,
        start_waits=1,
        land_codes=[edit.IGNITE_RUNNING],
        land_outputs=["waiting\nnext     ignite wait L-42\n"],
        wait_codes=[0, 0],
    )
    fake.install(monkeypatch)
    monkeypatch.delenv("IGNITE_SESSION", raising=False)

    code = edit.edit_file(
        "notes.md",
        repo=repo,
        cwd=repo,
        message="update notes",
        editor=python_script_cmd(editor),
    )
    assert code == 0
    assert (tree / "notes.md").read_text(encoding="utf-8") == "AFTER EDIT\n"
    assert git_log_subjects(tree) == ["docs: update notes", "chore: initial"]
    assert leaked.read_text(encoding="utf-8") == ""

    verbs = [cmd[1] for cmd, _ in fake.calls]
    assert verbs == ["start", "wait", "start", "land", "wait"]
    sessions = {session for _, session in fake.calls}
    assert len(sessions) == 1
    session = next(iter(sessions))
    assert session is not None and session.startswith("edit-")


def test_no_changes_closes_ticket_without_landing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = init_repo(tmp_path / "main", {"notes.md": "same\n"})
    tree = init_repo(tmp_path / "tree", {"notes.md": "same\n"})
    noop_editor = tmp_path / "noop.py"
    noop_editor.write_text("import sys\n", encoding="utf-8")

    fake = FakeIgnite(tree)
    fake.install(monkeypatch)

    code = edit.edit_file(
        "notes.md",
        repo=repo,
        cwd=repo,
        editor=python_script_cmd(noop_editor),
    )
    assert code == 0
    assert fake.closed_reasons == ["no changes"]
    assert git_log_subjects(tree) == ["chore: initial"]


def test_missing_file_requires_new_flag(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = init_repo(tmp_path / "main", {"existing.md": "hi\n"})
    tree = init_repo(tmp_path / "tree", {"existing.md": "hi\n"})
    editor = tmp_path / "writer.py"
    editor.write_text(
        "import pathlib, sys\npathlib.Path(sys.argv[1]).write_text('created\\n')\n",
        encoding="utf-8",
    )

    with pytest.raises(edit.EditError, match="--new"):
        edit.edit_file(
            "sub/brand_new.md", repo=repo, cwd=repo, editor=python_script_cmd(editor)
        )

    fake = FakeIgnite(tree)
    fake.install(monkeypatch)
    code = edit.edit_file(
        "sub/brand_new.md",
        repo=repo,
        cwd=repo,
        allow_new=True,
        editor=python_script_cmd(editor),
        ask=lambda _prompt: "",
    )
    assert code == 0
    assert (tree / "sub" / "brand_new.md").read_text(encoding="utf-8") == "created\n"
    assert git_log_subjects(tree) == ["docs: edit sub/brand_new.md", "chore: initial"]


def test_failed_land_can_reedit_and_amend_over_style_format(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = init_repo(tmp_path / "main", {"file.txt": "v0\n"})
    tree = init_repo(tmp_path / "tree", {"file.txt": "v0\n"})

    count_file = tmp_path / "count.txt"
    editor = tmp_path / "editor.py"
    editor.write_text(
        "import pathlib, sys\n"
        f"c = pathlib.Path({str(count_file)!r})\n"
        "n = int(c.read_text()) + 1 if c.exists() else 1\n"
        "c.write_text(str(n))\n"
        "pathlib.Path(sys.argv[1]).write_text(f'v{n}\\n')\n",
        encoding="utf-8",
    )

    def add_style_commit_first_time(t: Path) -> None:
        if (
            git_log_subjects(t)[0] != "style: format"
            and (t / "file.txt").read_text() == "v1\n"
        ):
            (t / "file.txt").write_text("v1 formatted\n")
            subprocess.run(["git", "-C", str(t), "add", "file.txt"], check=True)
            subprocess.run(
                ["git", "-C", str(t), "commit", "-q", "-m", "style: format"], check=True
            )

    fake = FakeIgnite(
        tree,
        land_codes=[2, 0],
        land_outputs=["FAIL     check failed\n", "landed   ok\n"],
        on_land=add_style_commit_first_time,
    )
    fake.install(monkeypatch)

    answers: Iterator[str] = iter(["e"])
    code = edit.edit_file(
        "file.txt",
        repo=repo,
        cwd=repo,
        message="chore: update file",
        editor=python_script_cmd(editor),
        ask=lambda _prompt: next(answers),
    )
    assert code == 0
    assert (tree / "file.txt").read_text(encoding="utf-8") == "v2\n"
    assert git_log_subjects(tree) == ["chore: update file", "chore: initial"]


def test_abort_at_commit_prompt_closes_ticket(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    repo = init_repo(tmp_path / "main", {"notes.md": "before\n"})
    tree = init_repo(tmp_path / "tree", {"notes.md": "before\n"})
    editor = tmp_path / "editor.py"
    editor.write_text(
        "import pathlib, sys\npathlib.Path(sys.argv[1]).write_text('changed\\n')\n",
        encoding="utf-8",
    )

    fake = FakeIgnite(tree)
    fake.install(monkeypatch)

    code = edit.edit_file(
        "notes.md",
        repo=repo,
        cwd=repo,
        editor=python_script_cmd(editor),
        ask=lambda _prompt: "q",
    )
    assert code == 1
    assert fake.closed_reasons == ["aborted"]


def test_edit_is_registered_in_cli() -> None:
    assert cli.commands()["edit"] == "edit"
