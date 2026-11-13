"""`card diverge` and its checks, on a small fake oracle report and rules text."""

import json
from pathlib import Path

import pytest

from beryl_tools import card_diverge

RULES = {
    "R70123b": "If a player is searching a hidden zone for cards with a stated quality,"
    " that player isn't required to find some or all of those cards. Example: skipped.",
}
KEY = ("Spellseeker", "finds nothing")


def scenario(status: str) -> dict[str, object]:
    return {"card": KEY[0], "name": KEY[1], "status": status}


def report(xmage: str, forge: str | None) -> dict[str, dict[str, object]]:
    found: dict[str, dict[str, object]] = {"xmage": {"scenarios": [scenario(xmage)]}}
    found["forge"] = {"scenarios": [scenario(forge)] if forge else []}
    return found


def judge(
    found: dict[str, dict[str, object]], oracle: str = "xmage", rule: str = "701.23b"
) -> dict[str, str]:
    return card_diverge.judge(found, *KEY, oracle, rule, "it must find one", RULES)


def test_writes_an_entry_when_the_other_oracle_agrees() -> None:
    entry = judge(report("fail", "pass"))
    assert entry["oracle"] == "xmage"
    assert entry["cr"] == (
        "CR 701.23b (https://yawgatog.com/resources/magic-rules/#R70123b): "
        '"If a player is searching a hidden zone for cards with a stated quality,'
        " that player isn't required to find some or all of those cards.\""
    )
    assert card_diverge.problems([entry], RULES) == []


@pytest.mark.parametrize(
    ("found", "why"),
    [
        (report("fail", "fail"), "definition is wrong"),
        (report("fail", None), "did not play"),
        (report("fail", "unsupported"), "did not play"),
        (report("fail", "allowlisted"), "did not play"),
        (report("pass", "pass"), "nothing to record"),
        (report("allowlisted", "pass"), "already has an entry"),
    ],
)
def test_refuses_without_the_other_oracle_agreeing(
    found: dict[str, dict[str, object]], why: str
) -> None:
    with pytest.raises(card_diverge.Refused, match=why):
        judge(found)


def test_refuses_a_rule_outside_the_pinned_release() -> None:
    with pytest.raises(card_diverge.Refused, match="not a rule"):
        judge(report("fail", "pass"), rule="999.9z")


def test_blames_forge_when_xmage_agrees() -> None:
    assert judge(report("pass", "fail"), "forge")["oracle"] == "forge"


def test_check_fails_a_hand_edited_entry() -> None:
    entry = judge(report("fail", "pass"))
    assert card_diverge.problems([entry], RULES) == []
    for edit in (
        {"cr": entry["cr"].replace("isn't required", "is required")},
        {"scenario": "*"},
        {"oracle": "mtgo"},
    ):
        assert card_diverge.problems([{**entry, **edit}], RULES)
    assert card_diverge.problems([entry, entry], RULES)
    assert card_diverge.problems([{**entry, "extra": "x"}], RULES)


def test_oracle_run_fails_unbacked_and_stale_entries() -> None:
    assert card_diverge.faults(report("allowlisted", "pass")) == []
    assert card_diverge.faults(report("allowlisted", "fail"))
    assert card_diverge.faults(report("allowlisted", None))
    stale = report("pass", "pass")
    stale["xmage"]["scenarios"][0]["stale_allowlist"] = True  # type: ignore[index]
    assert card_diverge.faults(stale)


def test_write_appends_the_entry(tmp_path: Path) -> None:
    path = tmp_path / "divergences.json"
    path.write_text("[]\n")
    entry = judge(report("fail", "pass"))
    card_diverge.write(path, entry)
    assert json.loads(path.read_text()) == [entry]
