import json

from beryl_tools import mobile


def journal(*runs: tuple[int, list[str]]) -> str:
    """A journal of app runs, each its pid and the messages it logged after opening."""
    lines = []
    for pid, messages in runs:
        lines.append({"source": "journal", "message": "opened", "data": {"pid": pid}})
        lines += [{"source": "app", "message": m, "data": None} for m in messages]
    return "\n".join(json.dumps(line) for line in lines)


def messages(text: str, pid: int) -> list[str]:
    return [r["message"] for r in mobile.run_records(text, pid)]


def test_run_records_are_the_runs_own() -> None:
    text = journal((7, ["started", "ui ready"]), (8, ["started"]))
    assert messages(text, 8) == ["opened", "started"]
    assert messages(text, 9) == []


def test_run_records_ignore_an_earlier_run_with_the_same_pid() -> None:
    # A freshly booted emulator hands the app the pid an earlier run had.
    text = journal((7, ["started", mobile.READY]), (8, []), (7, ["started"]))
    assert mobile.READY not in messages(text, 7)
