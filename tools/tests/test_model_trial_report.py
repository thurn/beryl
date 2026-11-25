"""The model trial's reviewer, report and judging page, on made-up records. No test starts
`claude` or calls `ignite`, git or the efficiency store: the reviewer's output is a recorded
stream-json answer."""

from __future__ import annotations

import json
import re
from pathlib import Path
from typing import Any

import pytest

from beryl_tools.model_trial import blind, judge, manifest, report, review
from beryl_tools.model_trial.blind import Filed, Side
from beryl_tools.model_trial.manifest import FILED, Grade, Pair, Review, Verdict
from beryl_tools.model_trial.spec import Spec, parse

SPEC: Spec = parse(
    {
        "arms": {
            "source": {"tier": "opus", "provider": "claude", "effort": "medium"},
            "copy": {"tier": "sonnet", "provider": "claude", "effort": "high"},
        },
        "cap_usd": 260,
        "tickets": ["E1.1", "E1.2", "E1.3"],
    }
)
OPUS, SONNET = "claude-opus-5-5", "claude-sonnet-5-5"
# What must never reach the reviewer or the page: the models, the sessions, the tickets.
GIVEAWAYS: re.Pattern[str] = re.compile(
    r"opus|sonnet|claude|cli-claude|E1\.1|TCOPY|\$1\.50|42 agent minutes|trial/",
    re.IGNORECASE,
)


def pair(token: str, source: str, order: int, **fields: Any) -> Pair:
    base: dict[str, Any] = {
        "pair": token,
        "trial": "T1",
        "source": source,
        "order": order,
        "state": FILED,
        "kind": "engine",
        "copy": f"C{order}XYZ",
        "models": {"source": OPUS, "copy": SONNET},
        "filed_at": "2026-11-24T10:00:00+00:00",
    }
    return Pair(**{**base, **fields})


def grades(token: str, source: dict[str, Any], copy: dict[str, Any]) -> list[Grade]:
    rows = []
    for arm, fields in (("source", source), ("copy", copy)):
        base: dict[str, Any] = {
            "pair": token,
            "trial": "T1",
            "arm": arm,
            "ticket": f"{arm}-{token}",
            "ended": "landed" if arm == "source" else "closed",
            "finished": True,
            "check": "pass",
        }
        rows.append(Grade(**{**base, **fields}))
    return rows


def answer(**picks: str) -> dict[str, Any]:
    rubric = {
        key: {"pick": picks.get(key, "tie"), "evidence": "a.rs:1 shows it"}
        for key in review.RUBRIC
    }
    return {"rubric": rubric, "defects": [], "shared": "no"}


def reviewed(
    token: str, run: int, a_arm: str, version: str = "v1", **picks: str
) -> Review:
    return Review(
        pair=token,
        trial="T1",
        run=run,
        seed=1,
        a_arm=a_arm,
        reviewer_model=OPUS,
        rubric_version=version,
        ok=True,
        answer=answer(**picks),
        cost_usd=2.5,
    )


def trial(**fields: Any) -> report.Trial:
    """Three graded pairs: `p1` that both runs give to the copy, `p2` whose runs split, and
    `p3` that is void."""
    p1 = pair(
        "p1",
        "E1.1",
        0,
        plant={"find": "x", "replace": "y", "expect": "z"},
        audit={"harm": "none", "note": "nothing built on it"},
    )
    p2 = pair("p2", "E1.2", 1, kind="interface")
    p3 = pair("p3", "E1.3", 2)
    rows = (
        grades(
            "p1",
            {"dollars": 4.0, "agent_minutes": 30.0},
            {"dollars": 1.0, "agent_minutes": 20.0},
        )
        + grades(
            "p2",
            {"dollars": 6.0, "agent_minutes": 50.0, "attachments": ["a.png"]},
            {"dollars": 2.0, "agent_minutes": 10.0, "check": "fail"},
        )
        + grades(
            "p3",
            {"dollars": 100.0, "void": ["the copy ran git fetch"]},
            {"dollars": 5.0},
        )
    )
    # Run 0 shows the source as A, run 1 the copy as A: the copy wins p1 under both labels.
    p1_answer = answer(overall="B", correct="B", size="A")
    p1_answer["planted"] = {"A": False, "B": True}
    swapped = answer(overall="A", correct="A", size="A")
    swapped["planted"] = {"A": True, "B": False}
    reviews = [
        Review("p1", "T1", 0, 1, "source", OPUS, "v1", True, p1_answer, 2.5),
        Review("p1", "T1", 1, 1, "copy", OPUS, "v1", True, swapped, 2.5),
        reviewed("p2", 0, "source", overall="A"),
        reviewed("p2", 1, "copy", overall="A"),
    ]
    base: dict[str, Any] = {
        "id": "T1",
        "spec": SPEC,
        "pairs": [p1, p2, p3],
        "grades": rows,
        "reviews": reviews,
    }
    return report.Trial(**{**base, **fields})


# The report.


def test_a_rubric_line_counts_only_when_both_runs_pick_the_same_arm() -> None:
    found = {r.pair.pair: r.picks or {} for r in report.results(trial())}
    assert found["p1"]["overall"] == "copy"
    assert found["p1"]["correct"] == "copy"
    # `size` was A both times, which is a different arm on each run.
    assert found["p1"]["size"] == "tie"
    assert found["p2"]["overall"] == "tie"
    text = report.table(trial())
    assert "| Overall | 0 | 1 | 1 |" in text


def test_the_table_gives_medians_with_their_counts_and_leaves_void_pairs_out() -> None:
    text = report.table(trial())
    assert "## All pairs (2 counted)" in text
    assert "| Median dollars | $5.00 (n=2) | $1.50 (n=2), 30% of the source's |" in text
    assert (
        "| Checks pass / fail / unknown | 2 / 0 / 0 (n=2) | 1 / 1 / 0 (n=2) |" in text
    )
    assert "## Void pairs (1)" in text
    assert "- E1.3 / C2XYZ: the copy ran git fetch" in text
    assert "| Interface pairs with no image | 0 of 1 | 1 of 1 |" in text


def test_the_spend_counts_the_copies_and_reviewer_but_not_the_source() -> None:
    text = report.table(trial())
    # The copies cost 1 + 2 + 5 (a void pair's copy was still paid for), the reviewer 4 x 2.5.
    assert "Spend: $18.00 of the $260.00 cap" in text
    assert (
        "The source arm's sessions cost $110.00 (n=3), which the cap leaves out."
        in text
    )


def test_the_header_ends_with_what_the_trial_does_not_equalize() -> None:
    lines = report.table(trial()).splitlines()
    assert lines[lines.index(report.FAIRNESS) + 1] == ""
    assert lines.index(report.FAIRNESS) < lines.index("## All pairs (2 counted)")


def test_a_planted_pair_reports_who_noticed_and_the_audit() -> None:
    text = report.table(trial())
    assert (
        "- E1.1: opus (source) noticed: no; sonnet (copy) noticed: yes; "
        "the audit found none: nothing built on it." in text
    )


def test_the_agreement_figure_appears_once_verdicts_exist() -> None:
    assert "matched the user's" not in report.table(trial())
    verdicts = [
        # A was the copy, which the reviewer also picked.
        Verdict("p1", "T1", "copy", "A", "A", "A"),
        # The reviewer's runs split, which "no preference" matches.
        Verdict("p2", "T1", "source", "B", "none", "none"),
        Verdict("p3", "T1", "source", "A", "A", "A"),
    ]
    text = report.table(trial(verdicts=verdicts))
    assert (
        "The reviewer's overall pick matched the user's on 2 of 2 judged pairs." in text
    )


def test_suspect_pairs_count_the_reviewers_finding_and_list_the_note() -> None:
    t = trial()
    t.reviews[2].answer["shared"] = [
        {"a": "x.rs:3", "b": "y.rs:4", "what": "a helper name"}
    ]
    t.pairs[0].contamination = {"decision": "clear", "note": "both copied the ticket"}
    t.grades[0].suspect = [
        {"what": "both arms added 'fn linked_slot_key'", "where": "both diffs"}
    ]
    text = report.table(t)
    assert "## Contamination: 1 suspect, 1 cleared, 0 voided" in text
    assert "found both sides share a helper name (x.rs:3 and y.rs:4)" in text
    assert "Note: both copied the ticket" in text


def test_two_trials_side_by_side_flag_a_differing_rubric_version() -> None:
    first = trial()
    second = trial(
        id="T2",
        reviews=[reviewed("p1", 0, "source", "v2"), reviewed("p1", 1, "copy", "v2")],
    )
    text = report.compare([first, second])
    assert "**The rubric version differs** (T1: v1; T2: v2)" in text
    assert "reviewer model differs" not in text
    assert "| | T1 | T2 |" in text
    assert "| Pairs counted | 2 | 2 |" in text
    assert "claude-sonnet-5-5" in text


# The reviewer.


def stream(result: object, cost: float = 1.25) -> str:
    text = result if isinstance(result, str) else json.dumps(result)
    events = [
        {"type": "system", "subtype": "init", "model": OPUS},
        {
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": "Reading."}]},
        },
        {
            "type": "result",
            "subtype": "success",
            "is_error": False,
            "result": text,
            "total_cost_usd": cost,
        },
    ]
    return "\n".join(json.dumps(e) for e in events) + "\n"


def material(tmp_path: Path) -> review.Material:
    images = tmp_path / "graded" / "source"
    images.mkdir(parents=True, exist_ok=True)
    shot = images / "E1.1-claude-opus-after.png"
    shot.write_bytes(b"\x89PNG")
    leaky = (
        "Landed by cli-claude-afc47913 on claude-opus-5-5 for E1.1 in 42 agent minutes, "
        "$1.50, at 2026-11-24T10:00:00Z; see trial/TCOPY and /Volumes/x/trees/tree-08/a.rs."
    )
    return review.Material(
        ticket=blind.ticket_markdown(
            "Keep links",
            "Fix E1.1's links, as Opus said.\n\n" + blind.COPY_RULES + "\nreset to abc",
            "It works.",
            ["E1.1", "TCOPY"],
        ),
        sides={
            "source": Side(
                diff="+fn a() {}\n",
                note=leaky,
                comments=[leaky],
                filed=[Filed("Follow-up for TCOPY", leaky)],
                check="pass",
                images=[shot],
            ),
            "copy": Side(diff=None, note="Sonnet gave up.", check="unknown"),
        },
        commits={"source": "f" * 40, "copy": None},
        ticket_ids=["E1.1", "TCOPY"],
        hub=True,
        plant={
            "find": "The store keys links by slot.",
            "replace": "The store keys links by name.",
            "expect": "Says the store keys by slot.",
        },
    )


def run_review(
    tmp_path: Path, outputs: list[str], run: int = 0
) -> tuple[Review, list[dict[str, str]]]:
    seen: list[dict[str, str]] = []

    def runner(argv: list[str], cwd: Path) -> str:
        files = {
            str(p.relative_to(cwd)): p.read_text(encoding="utf-8", errors="replace")
            for p in cwd.rglob("*")
            if p.is_file()
        }
        seen.append({"argv": " ".join(argv[:-1]), "prompt": argv[-1], **files})
        return outputs.pop(0)

    def snapshot(commit: str, dest: Path) -> None:
        dest.mkdir(parents=True)
        (dest / "AGENTS.md").write_text("the rules", encoding="utf-8")

    folder = tmp_path / "blind" / "abc123"
    row = review.review_run(
        material(tmp_path),
        pair("p1", "E1.1", 0),
        run,
        7,
        OPUS,
        "high",
        "claude",
        folder,
        runner,
        snapshot,
    )
    assert not folder.exists()
    return row, seen


def good() -> dict[str, Any]:
    found = answer(overall="A")
    found["planted"] = {"A": True, "B": False}
    found["defects"] = [
        {
            "side": "B",
            "where": "src/a.rs:12",
            "input": "a token that left",
            "what": "lost",
        }
    ]
    return found


def test_a_review_run_records_the_answer_and_its_cost(tmp_path: Path) -> None:
    row, seen = run_review(tmp_path, [stream(good())])
    assert row.ok and row.attempts == 1 and row.cost_usd == 1.25
    assert row.a_arm == review.a_arm(7, 0)
    assert row.rubric_version == review.rubric_version()
    assert review.picks(row)["overall"] == row.a_arm
    other = "copy" if row.a_arm == "source" else "source"
    assert review.defects(row) == {row.a_arm: 0, other: 1}
    argv = seen[0]["argv"]
    assert argv.startswith("claude -p --disallowedTools Bash")
    assert (
        "--model claude-opus-5-5 --effort high --output-format stream-json --verbose"
        in argv
    )


def test_the_second_run_swaps_a_and_b() -> None:
    for seed in range(8):
        assert {review.a_arm(seed, 0), review.a_arm(seed, 1)} == {"source", "copy"}


def test_an_answer_that_is_not_the_asked_json_is_run_once_more_then_failed(
    tmp_path: Path,
) -> None:
    row, _ = run_review(tmp_path, [stream("I prefer A."), stream({"rubric": {}}, 0.5)])
    assert not row.ok and row.attempts == 2 and row.cost_usd == 1.75
    assert row.error and "rubric.correct" in row.error
    row, _ = run_review(tmp_path, [stream("```json\nnot json\n```"), stream(good())])
    assert row.ok and row.attempts == 2


def test_nothing_shown_to_the_reviewer_names_a_model_session_ticket_or_cost(
    tmp_path: Path,
) -> None:
    for run in (0, 1):
        _, seen = run_review(tmp_path, [stream(good())], run)
        shown = {
            k: v
            for k, v in seen[0].items()
            if k not in ("argv", "A/AGENTS.md", "B/AGENTS.md")
        }
        for name, text in shown.items():
            assert not GIVEAWAYS.search(name), name
            if name != "prompt":
                assert not GIVEAWAYS.search(text), (name, GIVEAWAYS.search(text))
        image = next(n for n in shown if n.endswith(".png"))
        assert re.fullmatch(r"[AB]-images/image-1\.png", image)
        assert "some time" in shown[f"{image[0]}.md"]
        # The prompt carries the hub and planted paragraphs and no model or ticket.
        assert "switching the repaired check back on" in shown["prompt"]
        assert "The store keys links by name." in shown["prompt"]
        assert not re.search(r"sonnet|E1\.1|TCOPY", shown["prompt"], re.IGNORECASE)
        assert blind.COPY_RULES not in shown["ticket.md"]


def test_the_rubric_version_changes_with_the_prompt(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    before = review.rubric_version()
    changed = tmp_path / "prompt.txt"
    changed.write_text(
        review.PROMPT_FILE.read_text(encoding="utf-8") + "\nOne more line.\n",
        encoding="utf-8",
    )
    monkeypatch.setattr(review, "PROMPT_FILE", changed)
    assert review.rubric_version() != before


def test_review_runs_every_graded_pair_that_is_not_void(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv("BERYL_CACHE", str(tmp_path / "cache"))
    t = trial()
    for p in t.pairs:
        manifest.save_pair(p)
    for g in t.grades:
        manifest.append_grade(g)
    monkeypatch.setattr(review, "gather", lambda p, arms: material(tmp_path / p.pair))
    monkeypatch.setattr(review.io, "model_ids", lambda provider: {"opus": OPUS})
    monkeypatch.setattr(review.io, "claude_program", lambda: "claude")
    outputs = [stream(good()) for _ in range(4)]
    args = type("Args", (), {"trial": "T1"})()
    review.cmd_review(
        args, SPEC, lambda argv, cwd: outputs.pop(0), lambda c, d: d.mkdir(parents=True)
    )
    rows = manifest.read_reviews("T1")
    assert sorted((r.pair, r.run) for r in rows) == [
        ("p1", 0),
        ("p1", 1),
        ("p2", 0),
        ("p2", 1),
    ]
    assert {r.a_arm for r in rows if r.pair == "p1"} == {"source", "copy"}
    # A second call finds nothing left to do and starts no reviewer.
    calls: list[list[str]] = []

    def again(argv: list[str], cwd: Path) -> str:
        calls.append(argv)
        return ""

    review.cmd_review(args, SPEC, again, lambda c, d: None)
    assert not calls


# The user's page.


def test_the_page_names_no_model_or_ticket_in_its_html(tmp_path: Path) -> None:
    m = material(tmp_path)
    item = judge.Item(
        number=1,
        pair="p1",
        a_arm="copy",
        ticket=m.ticket,
        diffs={"A": "", "B": m.sides["source"].diff or ""},
        images={"A": [], "B": m.sides["source"].images},
    )
    page = judge.pair_html(item, 3, None)
    assert not GIVEAWAYS.search(page), GIVEAWAYS.search(page)
    assert (
        "p1" not in page and "copy" not in page.lower() and "source" not in page.lower()
    )
    assert "src='/image/1/B/1'" in page
    assert "E1.1" not in judge.index_html([item], {})


def test_a_verdict_needs_a_pick_for_each_question() -> None:
    item = judge.Item(number=2, pair="p2", a_arm="source", ticket="", diffs={})
    assert judge.verdict_of(item, "T1", {"quality": ["A"], "overall": ["B"]}) is None
    found = judge.verdict_of(
        item, "T1", {"quality": ["A"], "initiative": ["none"], "overall": ["B"]}
    )
    assert found == Verdict("p2", "T1", "source", "A", "none", "B")


def test_judged_pairs_are_spread_across_the_kinds_and_kept() -> None:
    found = {
        f"p{i}": (pair(f"p{i}", f"E1.{i}", i, kind=kind), {})
        for i, kind in enumerate(["engine"] * 6 + ["interface"] * 2 + ["tooling"] * 2)
    }
    chosen = judge.choose(found, 4, seed=3)
    kinds = sorted(found[c["pair"]][0].kind for c in chosen)
    assert kinds == ["engine", "engine", "interface", "tooling"]
    assert judge.choose(found, 4, seed=3) == chosen


def test_each_copy_arm_gets_its_own_tables_and_a_source_is_paid_for_once() -> None:
    two = parse(
        {
            "arms": {
                "source": {"tier": "opus", "provider": "claude", "effort": "medium"},
                "copy": {"tier": "sonnet", "provider": "claude", "effort": "high"},
                "hydrogen": {
                    "tier": "opus",
                    "provider": "agy-gemini",
                    "effort": "high",
                },
            },
            "cap_usd": 400,
            "tickets": ["E1.1", "E1.2", "E1.3"],
        }
    )
    base = trial(spec=two)
    extra = pair(
        "h1",
        "E1.1",
        0,
        arm="hydrogen",
        main="p1",
        models={"source": OPUS, "copy": "gemini-4-hydrogen"},
    )
    rows = grades("h1", {"dollars": 4.0, "ticket": "source-p1"}, {"dollars": 3.0})
    for g in base.grades:
        if g.pair == "p1" and g.arm == "source":
            g.ticket = "source-p1"
    text = report.table(
        report.Trial(
            id="T1",
            spec=two,
            pairs=[*base.pairs, extra],
            grades=[*base.grades, *rows],
            reviews=base.reviews,
        )
    )
    assert "## sonnet (copy) against the source: all pairs (2 counted)" in text
    assert "## agy-gemini (hydrogen) against the source: all pairs (1 counted)" in text
    assert "3 filed (and 1 copies on the other arms)" in text
    # E1.1's source is graded twice but paid for once: 4 + 6 + 100.
    assert "The source arm's sessions cost $110.00 (n=3)" in text
    assert "measure the model and that harness together" in text
    assert (
        "hydrogen `tier:opus` on agy-gemini at high effort (gemini-4-hydrogen)" in text
    )


def test_an_arm_whose_pairs_ran_on_two_models_gets_tables_for_each() -> None:
    base = trial()
    p2 = next(p for p in base.pairs if p.pair == "p2")
    p2.models = {"source": OPUS, "copy": "claude-sonnet-5-6"}
    text = report.table(base)
    assert (
        f"## sonnet (copy, {SONNET}) against the source: all pairs (1 counted)" in text
    )
    assert (
        "## sonnet (copy, claude-sonnet-5-6) against the source: all pairs (1 counted)"
        in text
    )


def test_a_partial_time_leaves_only_the_minutes_out() -> None:
    base = trial()
    for g in base.grades:
        if g.pair == "p1" and g.arm == "source":
            g.minutes_partial = True
    rows = {
        line: cells
        for line, *cells in report.group_rows(SPEC, report.results(base)[:2])
    }
    assert rows["Median dollars"][0].startswith("$5.00 (n=2)")
    assert (
        rows["Median agent minutes"][0] == "50.0 (n=1), 1 left out for a partial time"
    )
