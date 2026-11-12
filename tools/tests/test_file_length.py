import json
from pathlib import Path

from beryl_tools import file_length


def write(root: Path, rel: str, lines: int) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text("x\n" * lines)


def test_the_check_over_a_tree(tmp_path: Path) -> None:
    long = file_length.LIMIT + 2
    write(tmp_path, "rust/crates/beryl_rules/src/cast.rs", long)
    write(tmp_path, "rust/crates/beryl_rules/src/short.rs", file_length.LIMIT)
    allowlist = tmp_path / "allowlist.json"
    allowlist.write_text(json.dumps({"rust/crates/beryl_rules/src/cast.rs": long}))
    assert file_length.check(tmp_path, allowlist) == []
    # A file crossing the limit fails, tests, generated files and the drivers aside.
    write(tmp_path, "ui/src/game/Board.tsx", long)
    write(tmp_path, "rust/crates/beryl_rules/src/tests/big.rs", long)
    write(tmp_path, "rust/crates/beryl_rules/src/tests.rs", long)
    write(tmp_path, "ui/src/generated/protocol.ts", long)
    write(tmp_path, "ui/scripts/drive.mjs", long)
    write(tmp_path, "rust/crates/beryl_search/src/expert/a.rs", long)
    write(tmp_path, "tools/tests/test_big.py", long)
    [added] = file_length.check(tmp_path, allowlist)
    assert "Board.tsx" in added and "not on the allowlist" in added
    (tmp_path / "ui/src/game/Board.tsx").unlink()
    # An allowlisted file may not grow, and must be lowered or removed as it shrinks.
    write(tmp_path, "rust/crates/beryl_rules/src/cast.rs", long + 1)
    [grew] = file_length.check(tmp_path, allowlist)
    assert "up from" in grew
    write(tmp_path, "rust/crates/beryl_rules/src/cast.rs", long - 1)
    [shrunk] = file_length.check(tmp_path, allowlist)
    assert "lower it" in shrunk
    write(tmp_path, "rust/crates/beryl_rules/src/cast.rs", 5)
    [gone] = file_length.check(tmp_path, allowlist)
    assert "remove the entry" in gone
    # Shrinking rewrites the file to match.
    assert file_length.check(tmp_path, allowlist, shrink=True) == []
    assert json.loads(allowlist.read_text()) == {}
