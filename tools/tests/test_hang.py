import json
from datetime import date, timedelta

import pytest

from beryl_tools import hang


def test_a_steps_limit_is_several_times_its_slowest_recent_pass() -> None:
    hang.record(hang.key("ci", "rust", "tests"), 250.0)
    hang.record(hang.key("ci", "rust", "tests"), 300.0)
    hang.record(hang.key("ci", "rust", "tests"), 120.0)
    limit, why = hang.Limits("ci").limit("rust", "tests")
    assert limit == 1200.0
    assert why == "4x its slowest pass in 30 days, 300 s"


def test_the_limit_has_a_floor_a_cap_and_a_default() -> None:
    hang.record(hang.key("ci", "rust", "fmt"), 1.0)
    hang.record(hang.key("ci", "oracle", "oracle"), 900.0)
    limits = hang.Limits("ci")
    assert limits.limit("rust", "fmt")[0] == hang.FLOOR
    assert limits.limit("oracle", "oracle")[0] == hang.CAP
    assert limits.limit("app", "smoke") == (hang.UNKNOWN, "no pass in 30 days")


def test_passes_older_than_the_window_are_forgotten() -> None:
    old = (date.today() - timedelta(days=hang.DAYS + 1)).isoformat()
    name = hang.key("ci", "ui-test", "ui-test")
    hang.store_path().parent.mkdir(parents=True, exist_ok=True)
    hang.store_path().write_text(json.dumps({name: {old: 500.0}}))
    assert hang.Limits("ci").limit("ui-test", "ui-test")[0] == hang.UNKNOWN
    hang.record(name, 100.0)
    assert list(hang.read()[name]) == [date.today().isoformat()]


def test_the_environment_sets_every_limit(monkeypatch: pytest.MonkeyPatch) -> None:
    monkeypatch.setenv("CHECK_HANG_LIMIT_S", "5")
    assert hang.Limits("check").limit("rust", "clippy") == (5.0, "$CHECK_HANG_LIMIT_S")
