from pathlib import Path

from beryl_tools import oracle_memory, paths
from beryl_tools.oracle_memory import FAIL, GENERATED, NEW, PASS, Memory

SAMPLED = "sample"


def entry(card: str, name: str, snapshot: str = "life 20") -> dict[str, object]:
    return {"card": card, "name": name, "final": snapshot}


def memory(
    outcomes: dict[str, str] | None = None, always: set[str] | None = None
) -> Memory:
    return Memory("side", outcomes or {}, always or set())


def remember(m: Memory, e: dict[str, object], outcome: str) -> None:
    m.outcomes[m.key(e)] = outcome


def names(entries: list[dict[str, object]]) -> list[str]:
    return [oracle_memory.label(e) for e in entries]


def test_the_fast_check_plays_new_generated_scenarios_and_unpassed_cards() -> None:
    passed, failed, unknown = (entry(GENERATED, f"Generated {i}") for i in (1, 2, 3))
    gap = entry(GENERATED, "Generated 4")
    mine, mine_passed = entry("Counterspell", "counters"), entry(
        "Counterspell", "fizzles"
    )
    other = entry("Opt", "scry")
    m = memory(always={oracle_memory.label(gap)})
    remember(m, passed, PASS)
    # A failure another run recorded is already on staging: slow CI owns it.
    remember(m, failed, FAIL)
    remember(m, mine_passed, PASS)
    entries = [passed, failed, unknown, gap, mine, mine_passed, other]
    kept = m.select(entries, None, ["Counterspell"], True, SAMPLED)
    assert names(kept) == ["Generated / Generated 3", "Counterspell / counters"]


def test_a_rules_change_rewrites_a_generated_scenario_and_it_plays_again() -> None:
    m = memory()
    remember(m, entry(GENERATED, "Generated 1"), PASS)
    rewritten = entry(GENERATED, "Generated 1", snapshot="life 17")
    assert m.select([rewritten], None, [], True, SAMPLED) == [rewritten]
    # So does every scenario when the oracles' side changes.
    other_side = Memory("other side", m.outcomes, set())
    assert other_side.seen(entry(GENERATED, "Generated 1")) is None


def test_slow_ci_plays_its_sample_whole_and_everything_not_yet_passed() -> None:
    sample, changed = entry("Opt", "scry"), entry("Counterspell", "counters")
    passed, failed = entry(GENERATED, "Generated 1"), entry(GENERATED, "Generated 2")
    listed, derived = entry(GENERATED, "Generated 3"), entry("Island", "generated tap")
    m = memory(always={oracle_memory.label(listed)})
    for e in (sample, changed, passed):
        remember(m, e, PASS)
    remember(m, failed, FAIL)
    chosen = {"Opt": SAMPLED, "Counterspell": "card file changed"}
    entries = [sample, changed, passed, failed, listed, derived]
    kept = m.select(entries, chosen, [], False, SAMPLED)
    assert names(kept) == [
        "Opt / scry",
        "Generated / Generated 2",
        "Island / generated tap",
    ]
    assert chosen[GENERATED] == NEW and chosen["Island"] == NEW


def test_without_flags_every_scenario_or_the_named_cards_play() -> None:
    m = memory()
    entries = [entry(GENERATED, "Generated 1"), entry("Opt", "scry")]
    for e in entries:
        remember(m, e, PASS)
    assert m.select(entries, None, [], False, SAMPLED) == entries
    assert names(m.select(entries, None, ["Opt"], False, SAMPLED)) == ["Opt / scry"]


def test_outcomes_are_recorded_and_the_last_one_wins(tmp_path: Path) -> None:
    store = tmp_path / "outcomes.txt"
    ok, bad, stale = (entry("Opt", n) for n in ("scry", "draw", "loot"))
    results = {
        "xmage": {
            "scenarios": [
                {"card": "Opt", "name": "scry", "status": "pass"},
                {"card": "Opt", "name": "draw", "status": "fail"},
                {"card": "Opt", "name": "loot", "status": "pass"},
            ]
        },
        "forge": {
            "scenarios": [
                {"card": "Opt", "name": "scry", "status": "unsupported"},
                {"card": "Opt", "name": "draw", "status": "pass"},
                # A pass the allowlist explains as a divergence leaves that entry stale.
                {
                    "card": "Opt",
                    "name": "loot",
                    "status": "pass",
                    "stale_allowlist": True,
                },
            ]
        },
    }
    m = memory()
    m.record([ok, bad, stale], results, store)
    assert [m.seen(e) for e in (ok, bad, stale)] == [PASS, FAIL, FAIL]
    lines = store.read_text().splitlines()
    assert len(lines) == 3
    store.write_text(store.read_text() + f"{m.key(bad)} {PASS}\n")
    loaded = Memory.load([], store)
    loaded.side = m.side
    assert loaded.seen(bad) == PASS and loaded.seen(ok) == PASS


def test_the_side_hashes_the_exporters(tmp_path: Path) -> None:
    (tmp_path / "tools/src/beryl_tools").mkdir(parents=True)
    exporter = tmp_path / "tools/src/beryl_tools/xmage_java.py"
    exporter.write_text("one")
    before = oracle_memory.side(["pin"], tmp_path)
    exporter.write_text("two")
    assert oracle_memory.side(["pin"], tmp_path) != before
    assert oracle_memory.side(["other pin"], tmp_path) != before


def test_only_changes_that_reach_the_scenarios_call_for_the_lane() -> None:
    def package(name: str, *deps: str) -> dict[str, object]:
        manifest = paths.REPO / "rust" / "crates" / name / "Cargo.toml"
        return {
            "name": name,
            "manifest_path": str(manifest),
            "dependencies": [{"name": d, "path": f"rust/crates/{d}"} for d in deps],
        }

    packages = [
        package("beryl_rules"),
        package("beryl_cards", "beryl_rules"),
        package("beryl_cli", "beryl_cards"),
        package("beryl_mobile"),
    ]
    assert oracle_memory.reached(["rust/crates/beryl_rules/src/combat.rs"], packages)
    assert not oracle_memory.reached(["rust/crates/beryl_cli/src/main.rs"], packages)
    assert not oracle_memory.reached(["tools/x.py", "docs/checks.md"], packages)
    assert oracle_memory.reached(["data/card-store.tsv"], packages)
