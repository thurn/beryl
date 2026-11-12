from pathlib import Path

from beryl_tools import discarded_push


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


DISCARD = "let _ = list.push(slot);\n"
KEPT = """\
list.push(slot)?;
list.push_bounded(slot);
let pushed = list.push(slot);
if later.push_if_room(cost) {}
let _ = resolve(&mut ctx);
count_ = out.push(1);
match x { _ => list.push(slot)?, }
units.push(kinds).ok()?;
"""


def test_rules_code_may_not_discard_a_failed_push(tmp_path: Path) -> None:
    # Tests may, in their own files or after a file's code, and so may code outside the
    # kernel, the rules and the cards. A push whose result is read is no discard.
    write(tmp_path, "rust/crates/beryl_engine/src/tests/combat.rs", DISCARD)
    write(tmp_path, "rust/crates/beryl_cards/src/tests.rs", DISCARD)
    write(tmp_path, "rust/crates/beryl_session/src/save.rs", DISCARD)
    inline = KEPT + "#[cfg(test)]\nmod tests {\n    " + DISCARD + "}\n"
    write(tmp_path, "rust/crates/beryl_kernel/src/fixed.rs", inline)
    assert discarded_push.check(tmp_path) == []
    # Whatever the statement's shape: over several lines, with no `let`, or as an option.
    several = "let _ = list\n    .get_or_insert_with(Default::default)\n    .push(a);\n"
    write(tmp_path, "rust/crates/beryl_engine/src/layers.rs", "\n" + several)
    write(tmp_path, "rust/crates/beryl_kernel/src/zone.rs", "_ = cards.push(card);\n")
    write(tmp_path, "rust/crates/beryl_cards/src/white/a.rs", "v.push(x).ok();\n")
    kernel, rule, card = discarded_push.check(tmp_path)
    assert card.startswith("rust/crates/beryl_cards/src/white/a.rs:1: ")
    assert rule.startswith("rust/crates/beryl_engine/src/layers.rs:2: ")
    assert kernel.startswith("rust/crates/beryl_kernel/src/zone.rs:1: ")
    assert "push_bounded" in rule
