from pathlib import Path

from beryl_tools import plan_references


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


def test_code_may_not_cite_a_plan(tmp_path: Path) -> None:
    write(
        tmp_path, "rust/crates/a/src/lib.rs", "// Plain words about a planned move.\n"
    )
    write(tmp_path, "docs/notes.md", "plan " + "K1\n")
    assert plan_references.check(tmp_path) == []
    write(tmp_path, "rust/crates/a/src/lib.rs", "//\n// Effect data (plan " + "K1).\n")
    write(
        tmp_path, "rust/crates/a/Cargo.toml", "# The vocabulary (plan" + ", Crates)\n"
    )
    write(tmp_path, "tools/src/beryl_tools/x.py", "# See docs/project" + "-50k.md\n")
    toml, rust, tool = plan_references.check(tmp_path)
    assert toml.startswith("rust/crates/a/Cargo.toml:1: ")
    assert rust.startswith("rust/crates/a/src/lib.rs:2: ")
    assert tool.startswith("tools/src/beryl_tools/x.py:1: ")
