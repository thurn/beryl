import sys
from pathlib import Path

from beryl_tools import checks, edit


def test_the_subject_names_the_file_and_passes_the_commit_check() -> None:
    assert (
        edit.subject("docs/benchmarking.md", new=False)
        == "docs: update benchmarking.md"
    )
    assert edit.subject("rust/crates/x/src/lib.rs", new=True) == "chore: add lib.rs"
    long = edit.subject("a" * 80 + ".md", new=False)
    assert len(long) == checks.COMMIT_MAX and checks.COMMIT.match(long)


def test_the_worktree_is_named_after_the_path() -> None:
    assert edit.worktree_name("docs/benchmarking.md") == "edit-docs-benchmarking-md"
    assert edit.worktree_name("AGENTS.md") == "edit-agents-md"
    assert len(edit.worktree_name("x/" * 40 + "y.md")) <= len("edit-") + 40


def test_files_are_formatted_by_the_tool_their_check_uses() -> None:
    tree: Path = Path("/tree")

    def tool(rel: str) -> str | None:
        fmt = edit.formatter(tree, rel)
        return (
            Path(fmt[0][0]).name.removesuffix(".cmd") + " " + fmt[1].as_posix()
            if fmt
            else None
        )

    assert tool("README.md") == f"{Path(sys.executable).name} /tree"
    assert tool(".beads/notes.md") is None
    assert tool("rust/crates/beryl_app/src/main.rs") == "cargo /tree/rust"
    assert tool("ui/src/App.tsx") == "prettier /tree/ui"
    assert tool("justfile") == "just /tree"
    assert tool("data/card-printings.csv") is None
