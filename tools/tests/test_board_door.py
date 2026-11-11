from pathlib import Path

from beryl_tools import board_door


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


OPEN = "s.board().set_life(Seat::P0, 1);\n"


def test_rules_code_may_not_open_the_board_door(tmp_path: Path) -> None:
    # Tests and callers outside the rules may, and a name that only contains it is no door.
    write(tmp_path, "rust/crates/beryl_engine/src/tests/combat.rs", OPEN)
    write(tmp_path, "rust/crates/beryl_cards/src/tests.rs", OPEN)
    write(tmp_path, "rust/crates/beryl_session/src/save.rs", OPEN)
    write(tmp_path, "rust/crates/beryl_engine/src/turn.rs", "s.dashboard();\n")
    assert board_door.check(tmp_path) == []
    write(tmp_path, "rust/crates/beryl_engine/src/turn.rs", "\n" + OPEN)
    write(tmp_path, "rust/crates/beryl_engine/src/lib.rs", "use k::board::Board;\n")
    write(tmp_path, "rust/crates/beryl_cards/src/white/a.rs", OPEN)
    facade, rule, card = board_door.check(tmp_path)
    assert facade.startswith("rust/crates/beryl_engine/src/lib.rs:1: ")
    assert rule.startswith("rust/crates/beryl_engine/src/turn.rs:2: ")
    assert card.startswith("rust/crates/beryl_cards/src/white/a.rs:1: ")
    assert "add the mutator the rule needs" in rule
