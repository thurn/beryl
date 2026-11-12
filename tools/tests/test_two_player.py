from pathlib import Path

from beryl_tools import two_player


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


IMPORT = "use crate::two_player::TwoPlayerSeat as _;\n"


def test_rules_code_may_not_name_the_two_player_module(tmp_path: Path) -> None:
    # The facade's re-export, tests, and callers outside the rules may.
    write(tmp_path, "rust/crates/beryl_engine/src/lib.rs", "pub use k::two_player;\n")
    write(tmp_path, "rust/crates/beryl_rules/src/tests/combat.rs", IMPORT)
    write(tmp_path, "rust/crates/beryl_cards/src/tests.rs", IMPORT)
    write(tmp_path, "rust/crates/beryl_session/src/save.rs", IMPORT)
    write(tmp_path, "rust/crates/beryl_rules/src/turn.rs", "fn two_players() {}\n")
    assert two_player.check(tmp_path) == []
    # The rules' own root may not, nor a module written beside the facade's.
    write(tmp_path, "rust/crates/beryl_rules/src/combat.rs", "\n" + IMPORT)
    write(tmp_path, "rust/crates/beryl_rules/src/lib.rs", "use k::two_player;\n")
    write(tmp_path, "rust/crates/beryl_engine/src/turn.rs", IMPORT)
    write(tmp_path, "rust/crates/beryl_cards/src/white/a.rs", IMPORT)
    rule, root, beside, card = two_player.check(tmp_path)
    assert rule.startswith("rust/crates/beryl_rules/src/combat.rs:2: ")
    assert root.startswith("rust/crates/beryl_rules/src/lib.rs:1: ")
    assert beside.startswith("rust/crates/beryl_engine/src/turn.rs:1: ")
    assert card.startswith("rust/crates/beryl_cards/src/white/a.rs:1: ")
    assert "opponents_of" in rule
