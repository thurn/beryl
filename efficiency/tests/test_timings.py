"""Timing ingestion and the compile command over synthetic invocations only."""

import json
from pathlib import Path

import pytest

from beryl_efficiency import changes, cli, facts, ingest, metrics, paths, store, timings
from beryl_efficiency.scope import Scope, parse_since


def test_ingest_query_modes_freshness_and_metrics(
    tmp_path: Path, cache: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    folder = cache / "ci" / "compile-timings"
    folder.mkdir(parents=True)
    samples = [
        ("one", "alpha", "check", 4, False),
        ("two", "alpha", "check", 8, False),
        ("three", "alpha", "check", 0, True),
        ("one", "beta", "check", 4, False),
        ("one", "alpha", "clippy", 12, False),
        ("one", "alpha", "test-build", 20, False),
    ]
    rows = [
        {
            "run": r,
            "crate": c,
            "mode": m,
            "seconds": s,
            "fresh": f,
            "time": "2026-11-16T12:00:00Z",
            "host": "jarvis",
        }
        for r, c, m, s, f in samples
    ]
    (folder / "one.jsonl").write_text(
        "".join(json.dumps(r) + "\n" for r in rows) + '{"bad":true}\n'
    )
    source = timings.Timings(cache)
    summary = ingest.run(paths.store(), paths.lock(), [source], "jarvis")
    assert summary is not None and summary.total().skipped == 1
    second = ingest.run(paths.store(), paths.lock(), [source], "jarvis")
    assert second is not None and second.total().bytes_read == 0
    # Replay the same run from another file: its primary key keeps it idempotent.
    (folder / "duplicate.jsonl").write_text(json.dumps(rows[0]) + "\n")
    ingest.run(paths.store(), paths.lock(), [source], "jarvis")
    assert cli.main(["compile", "--no-ingest", "--since", "all", "--json"]) == 0
    costs = json.loads(capsys.readouterr().out)["costs"]
    alpha = next(r for r in costs if r["crate"] == "alpha" and r["mode"] == "check")
    assert alpha == {
        "crate": "alpha",
        "mode": "check",
        "builds": 2,
        "fresh": 1,
        "median_seconds": 6,
        "seconds": 12,
        "share": 75,
    }
    assert {r["median_seconds"] for r in costs if r["mode"] != "check"} == {12, 20}
    assert cli.main(["metrics", "--no-ingest", "--json"]) == 0
    listed = json.loads(capsys.readouterr().out)["metrics"]
    assert "compile.alpha.check" in {m["name"] for m in listed}
    db = store.open_store(paths.store())
    try:
        assert db.execute("SELECT count(*) FROM compile_timings").fetchone()[0] == 6
        assert (
            set(f"compile.alpha.{m}" for m in timings.MODES) <= metrics.METRICS.keys()
        )
        from beryl_efficiency.facts import load

        points = metrics.METRICS["compile.alpha.check"].points(
            db, load(db), Scope(parse_since("all"))
        )
        assert sorted(s for _, s in points) == [4, 8]
        assert timings.report(db, Scope(parse_since("2026-11-17"))).data["costs"] == []
        assert (
            timings.report(db, Scope(parse_since("all"), host="other")).data["costs"]
            == []
        )
    finally:
        db.close()


@pytest.mark.parametrize(
    "seconds,fresh", [(-1, False), (float("nan"), False), (2, True), (True, False)]
)
def test_malformed_duration_is_not_a_measurement(seconds: float, fresh: bool) -> None:
    with pytest.raises((ValueError, TypeError)):
        timings.Timings(Path("unused")).parse(
            Path("unused"),
            json.dumps(
                {
                    "mode": "check",
                    "seconds": seconds,
                    "fresh": fresh,
                }
            ).encode(),
        )


def test_changes_resolve_crate_metrics_without_a_prior_metrics_query(
    cache: Path,
) -> None:
    name = "compile.beryl_example.test-build"
    change = changes.Change("Reduce example compile cost", name, "down", None)
    assert changes.split_metric(name)[0] == name
    assert changes.problems([change], [], lambda _: True) == []
    assert metrics.get("compile.beryl_example.unknown") is None
    db = store.open_store(paths.store())
    try:
        # Resolving names must not depend on any timing row or CLI call.
        metrics.METRICS.pop(name, None)
        mark = changes.mark(db, facts.load(db), change, None)
        assert mark.verdict == "not on staging yet"
        assert name in metrics.METRICS
    finally:
        db.close()
