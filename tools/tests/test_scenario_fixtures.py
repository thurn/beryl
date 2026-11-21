from pathlib import Path

import pytest

from beryl_tools import scenario_fixtures


@pytest.mark.parametrize(
    ("code", "line"),
    [
        ("beryl_cards::scenarios().iter()\n    .find(|(name, _)| *name == CARD);", 1),
        ("beryl_cards::scenarios()[0];", 1),
        ("let corpus = beryl_cards::scenarios();\nlet cases = corpus[0];", 2),
        (
            "for (_, cases) in beryl_cards::scenarios() {\n    use_case(&cases[0]);\n}",
            2,
        ),
        ("use beryl_cards::{scenarios as corpus, CARDS};\ncorpus().iter().nth(1);", 2),
        ("beryl_cards::written_scenarios_for(names::CLOCKWORK);", 1),
        ("let [first, ..] = beryl_cards::scenarios() else { return };", 1),
        (
            "for (_, cases) in beryl_cards::scenarios() {\n"
            '    for case in cases { if case.name == "a case" { run(case); } }\n}',
            2,
        ),
    ],
    ids=[
        "name",
        "index",
        "local",
        "loop",
        "alias",
        "fixed-tool-query",
        "pattern",
        "case-name",
    ],
)
def test_selecting_from_the_card_files_fails(code: str, line: int) -> None:
    assert scenario_fixtures.borrowed_lines(code, in_registry=False) == [line]


def test_corpus_sweeps_and_local_cases_pass(tmp_path: Path) -> None:
    for rel, source in [
        (
            "rust/crates/example/tests/cases.rs",
            """
            // beryl_cards::scenarios()[0] used to be brittle.
            let example = "beryl_cards::scenarios()[0]";
            let owned = [Scenario::new("a local case")];
            run(&owned[0]);
            for (_, cases) in beryl_cards::scenarios() {
                for case in *cases { run(case); }
            }
            """,
        ),
        (
            "rust/cards/example/set/card.rs",
            "beryl_cards::scenarios()[0];",
        ),
    ]:
        path = tmp_path / rel
        path.parent.mkdir(parents=True)
        path.write_text(source)
    files = [path.relative_to(tmp_path).as_posix() for path in tmp_path.rglob("*.rs")]
    assert scenario_fixtures.check(tmp_path, files) == []
