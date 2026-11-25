"""`model-trial new --dry-run` on made-up spec files. No test calls `ignite`: the machine name
is patched, and filing fails the test."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any

import pytest

from beryl_tools.model_trial import io, new
from beryl_tools.model_trial.spec import from_notes

SPEC: dict[str, Any] = {
    "arms": {
        "source": {"tier": "sonnet", "provider": "claude", "effort": "high"},
        "copy": {"tier": "sonnet", "provider": "claude", "effort": "medium"},
    },
    "block_size": 5,
    "max_pairs": 15,
    "cap_usd": 120,
    "tickets": [f"E9.{n}" for n in range(1, 21)],
}


@pytest.fixture(autouse=True)
def no_ignite(monkeypatch: pytest.MonkeyPatch) -> None:
    def fail(_: dict[str, Any]) -> dict[str, Any]:
        raise AssertionError("a test filed a batch")

    monkeypatch.setattr(io, "this_machine", lambda: "testbox")
    monkeypatch.setattr(io, "batch", fail)


def dry_run(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], spec: object
) -> tuple[int, str, str]:
    path = tmp_path / "spec.json"
    path.write_text(spec if isinstance(spec, str) else json.dumps(spec))
    args = argparse.Namespace(title="Trial two", spec=str(path), dry_run=True)
    code = new.cmd_new(args)
    out = capsys.readouterr()
    return code, out.out, out.err


def ops(out: str) -> list[dict[str, Any]]:
    return list(json.loads(out)["ops"])


def test_dry_run_prints_epic_blocks_and_report_in_order(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    code, out, _ = dry_run(tmp_path, capsys, SPEC)
    assert code == 0
    created = ops(out)
    # Twenty tickets, at most fifteen pairs, five per block: three blocks.
    assert [op["fields"]["title"] for op in created] == [
        "Trial two",
        "Trial two, block 1 of 3",
        "Trial two, block 2 of 3",
        "Trial two, block 3 of 3",
        "Trial two, the final reading",
    ]
    epic = created[0]["fields"]
    assert epic["type"] == "epic"
    assert epic["allocation"] == 0
    assert epic["status"] == "pending_approval"
    assert epic["labels"] == ["machine:testbox"]
    assert from_notes(epic["notes"]).tickets == tuple(SPEC["tickets"])
    for index, op in enumerate(created[1:], start=1):
        assert op["parent"] == "$0"
        assert op["deps"] == ([{"on": f"${index - 1}"}] if index > 1 else [])
        assert op["fields"]["labels"] == ["tier:high"]
        assert "`model-trial` skill" in op["fields"]["body"]
    assert [op["fields"]["estimate_minutes"] for op in created[1:]] == [45, 45, 45, 60]
    assert "block 2 of 3" in created[2]["fields"]["body"]
    assert "`--block 2`" in created[2]["fields"]["body"]


def test_a_short_list_gives_fewer_blocks(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    code, out, _ = dry_run(
        tmp_path, capsys, {**SPEC, "tickets": ["A", "B", "C", "D", "E", "F"]}
    )
    assert code == 0
    titles = [op["fields"]["title"] for op in ops(out)]
    assert titles[1:] == [
        "Trial two, block 1 of 2",
        "Trial two, block 2 of 2",
        "Trial two, the final reading",
    ]


@pytest.mark.parametrize(
    ("spec", "field"),
    [
        ({**SPEC, "tickets": []}, "tickets"),
        ({**SPEC, "block_size": 0}, "block_size"),
        ({k: v for k, v in SPEC.items() if k != "cap_usd"}, "cap_usd"),
        ("{not json", "not valid JSON"),
    ],
)
def test_a_bad_spec_files_nothing(
    tmp_path: Path, capsys: pytest.CaptureFixture[str], spec: object, field: str
) -> None:
    code, out, err = dry_run(tmp_path, capsys, spec)
    assert code == 2
    assert out == ""
    assert field in err
