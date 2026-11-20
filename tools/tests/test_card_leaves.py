from pathlib import Path

from beryl_tools import card_leaves

ALLOWLIST = '{"crates": ["beryl_cards", "beryl_app"]}'


def write(root: Path, rel: str, text: str) -> None:
    (root / rel).parent.mkdir(parents=True, exist_ok=True)
    (root / rel).write_text(text)


def manifest(name: str, body: str) -> str:
    return f'[package]\nname = "{name}"\n{body}'


def scaffold(root: Path) -> None:
    write(root, card_leaves.ALLOWLIST, ALLOWLIST)
    write(
        root,
        "rust/crates/beryl_cards/Cargo.toml",
        manifest("beryl_cards", '[dependencies]\ncards_1993 = { path = "x" }\n'),
    )
    write(
        root,
        "rust/crates/beryl_app/Cargo.toml",
        manifest("beryl_app", '[dependencies]\nberyl_cards = { path = "x" }\n'),
    )
    write(root, "rust/crates/beryl_app/src/lib.rs", "use beryl_cards::decks;\n")


def test_leaves_and_dev_dependencies_pass(tmp_path: Path) -> None:
    scaffold(tmp_path)
    dev = '[dev-dependencies]\nberyl_cards = { path = "x" }\n'
    write(tmp_path, "rust/crates/beryl_rules/Cargo.toml", manifest("beryl_rules", dev))
    # Comments and string literals are not uses.
    text = '// beryl_cards\n/// see cards_1993\nlet n = "beryl_cards"; // cards_1993\n'
    write(tmp_path, "rust/crates/beryl_rules/src/lib.rs", text)
    assert card_leaves.check(tmp_path) == []


def test_a_normal_dependency_or_a_src_use_fails(tmp_path: Path) -> None:
    scaffold(tmp_path)
    dep = '[dependencies]\nberyl_cards = { path = "x" }\n'
    write(tmp_path, "rust/crates/beryl_rules/Cargo.toml", manifest("beryl_rules", dep))
    build = '[target.x.build-dependencies]\ncards_1994 = { path = "x" }\n'
    write(
        tmp_path, "rust/crates/beryl_kernel/Cargo.toml", manifest("beryl_kernel", build)
    )
    write(tmp_path, "rust/crates/beryl_search/Cargo.toml", manifest("beryl_search", ""))
    write(tmp_path, "rust/crates/beryl_search/src/a.rs", "\nuse beryl_cards::x;\n")
    kernel, rules, search = card_leaves.check(tmp_path)
    assert "beryl_kernel depends on cards_1994" in kernel
    assert "beryl_rules depends on beryl_cards" in rules
    assert search.startswith("rust/crates/beryl_search/src/a.rs:2: ")
    assert "Cargo rebuilds every dependent" in search


def test_a_stale_allowlist_entry_fails(tmp_path: Path) -> None:
    scaffold(tmp_path)
    write(tmp_path, "rust/crates/beryl_app/Cargo.toml", manifest("beryl_app", ""))
    write(tmp_path, "rust/crates/beryl_app/src/lib.rs", "")
    assert card_leaves.check(tmp_path) == [
        f"{card_leaves.ALLOWLIST}: remove beryl_app, which no longer depends on a card "
        "crate"
    ]
