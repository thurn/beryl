"""A stand-in for the hub's private efficiency reads (ignite/crates/ignite_hub/src/routes/
efficiency.rs), served over HTTP on this machine from synthetic rows. It checks the machine's
bearer token and `X-Actor` as the hub does, pages records and metrics in arrival order with
the hub's cursors, and can be made to fail after a number of requests or stop answering.
"""

from __future__ import annotations

import json
import threading
from dataclasses import dataclass, field
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

from beryl_efficiency import exchange, store

TOKEN = "synthetic-machine-token"
PROJECT = "test"
CURSOR = "v1-"


@dataclass
class FleetHub:
    """The hub's rows: `records` as the hub sends them (with each machine's name and cores),
    `metrics` as `{machine, machine_name, source, data, at}` in arrival order, and `state`.
    """

    records: list[dict[str, object]] = field(default_factory=list)
    metrics: list[dict[str, object]] = field(default_factory=list)
    state: dict[str, object] = field(default_factory=dict)
    # The most rows a page holds, so that a few rows take several pages.
    page_size: int = 2
    # How many more requests succeed before the hub answers 503; None for no limit.
    budget: int | None = None
    # Each request's route and query, in order.
    requests: list[str] = field(default_factory=list)
    server: ThreadingHTTPServer | None = None
    # The checkout whose `ignite.toml` names this hub (`configure`).
    checkout: Path = Path()

    @property
    def url(self) -> str:
        assert self.server is not None
        return f"http://127.0.0.1:{self.server.server_address[1]}"

    def start(self) -> FleetHub:
        hub = self

        class Handler(BaseHTTPRequestHandler):
            def do_GET(self) -> None:
                status, body = hub.answer(
                    self.path,
                    self.headers.get("Authorization"),
                    self.headers.get("X-Actor"),
                )
                data = json.dumps(body).encode()
                self.send_response(status)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(data)))
                self.send_header("Cache-Control", "no-store")
                self.end_headers()
                self.wfile.write(data)

            def log_message(self, format: str, *args: object) -> None:
                del format, args

        server = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        threading.Thread(target=server.serve_forever, daemon=True).start()
        self.server = server
        return self

    def stop(self) -> None:
        """Stops answering; a stopped hub refuses connections. Stopping twice is harmless."""
        server = self.server
        if server is not None:
            server.shutdown()
            server.server_close()

    def configure(self, checkout: Path, home: Path) -> None:
        """Names this hub in `checkout`'s `ignite.toml` and registers the machine in the
        Ignite home, as `ignite install` does."""
        self.checkout = checkout
        checkout.mkdir(parents=True, exist_ok=True)
        (checkout / "ignite.toml").write_text(
            f'[hub]\nurl = "{self.url}/"\nproject = "{PROJECT}"\n', encoding="utf-8"
        )
        (home / "machine.json").write_text(
            json.dumps({"id": "m-2", "token": TOKEN}), encoding="utf-8"
        )

    def answer(
        self, path: str, auth: str | None, actor: str | None
    ) -> tuple[int, dict[str, object]]:
        parts = urlsplit(path)
        route = parts.path.removeprefix(f"/{PROJECT}/api/efficiency/")
        query = {k: v[0] for k, v in parse_qs(parts.query).items()}
        self.requests.append(f"{route}?{parts.query}")
        if auth != f"Bearer {TOKEN}" or not actor:
            return 401, {"error": "efficiency reads need a registered machine's token"}
        budget = self.budget
        if budget is not None:
            if budget <= 0:
                return 503, {"error": "unavailable"}
            self.budget = budget - 1
        limit = min(int(query.get("limit", "1000")), self.page_size)
        if route == "records":
            after = int(query.get("after", "0"))
            page = [r for r in self.records if int(str(r["id"])) > after][:limit]
            following = int(str(page[-1]["id"])) if page else after
            return 200, {"records": page, "next_id": following}
        if route == "metrics":
            after = int(query.get("cursor", f"{CURSOR}0").removeprefix(CURSOR), 16)
            page = self.metrics[after : after + limit]
            headers = []
            for machine, source in dict.fromkeys(
                (str(r["machine"]), str(r["source"])) for r in page
            ):
                found = self.header(machine, source)
                if found is not None:
                    headers.append(
                        {"machine": machine, "source": source, "data": found}
                    )
            return 200, {
                "rows": page,
                "headers": headers,
                "next_cursor": f"{CURSOR}{after + len(page):x}",
            }
        if route == "state":
            return 200, self.state
        return 404, {"error": "Unknown route"}

    def header(self, machine: str, source: str) -> str | None:
        """The newest-schema export header of one machine and source, as the hub sends it."""
        found = [
            str(r["data"])
            for r in self.metrics
            if r["machine"] == machine
            and r["source"] == source
            and "export" in json.loads(str(r["data"]))
        ]
        return max(
            found, key=lambda d: int(json.loads(d).get("schema") or 0), default=None
        )


def at(hour: int, minute: int) -> str:
    return f"2026-11-15T{hour:02d}:{minute:02d}:00.000Z"


MACHINES = {"m-1": ("Jarvis.local", 8), "m-2": ("Cygnus", 16)}


def record(
    id: int,
    machine: str,
    verb: str,
    family: str,
    wall: float,
    cpu: float,
    end: str,
    outcome: str = "pass",
    commit: str = "k1",
) -> dict[str, object]:
    """A command record as the hub sends it. The class of `tip` and `land` is the verb."""
    name, cores = MACHINES[machine]
    return {
        "id": id,
        "machine": machine,
        "machine_name": name,
        "machine_cores": cores,
        "ticket": "t.1",
        "session": None,
        "tree": "tree-07",
        "verb": verb,
        "class": "land" if verb == "cached" else verb,
        "family": family,
        "queued_s": 0.0,
        "wall_s": wall,
        "cpu_s": cpu,
        "instructions": None,
        "peak_mb": 100,
        "outcome": outcome,
        "commit_sha": commit,
        "ignite": "i1",
        "at": end,
    }


def export_lines(tmp_path: Path, host: str) -> list[str]:
    """Another machine's efficiency export, of one request, as the lines it posts."""
    other = store.open_store(tmp_path / f"{host}.sqlite")
    other.execute(
        "INSERT INTO requests (host, provider, transcript, message, time) "
        "VALUES (?, 'claude', 't', 'm0', ?)",
        (host, at(9, 0)),
    )
    other.commit()
    out = tmp_path / f"{host}.jsonl"
    exchange.export(other, host, None, out)
    other.close()
    return [line for line in out.read_text(encoding="utf-8").splitlines() if line]


def metric(machine: str, source: str, data: str) -> dict[str, object]:
    return {
        "machine": machine,
        "machine_name": MACHINES[machine][0],
        "source": source,
        "data": data,
        "at": at(11, 0),
    }


def two_machines(tmp_path: Path) -> FleetHub:
    """Jarvis (`m-1`) and Cygnus (`m-2`, this machine) ran tip verification on k1. Jarvis ran
    the gate. Cygnus's tip run on k2 failed `engine`, the regression the hub attributes to
    k1..k2 and repair ticket `t.1`, and Jarvis's run on k3 an hour later passed. Both posted
    their efficiency exports and Jarvis a plan usage sample."""
    sample = {
        "status": "ok",
        "provider": "claude",
        "percent_used": 40.0,
        "window": "Weekly",
        "at": at(11, 0),
    }
    return FleetHub(
        records=[
            record(1, "m-1", "tip", "rust", 120, 480, at(10, 40)),
            record(2, "m-2", "tip", "engine", 60, 240, at(10, 41)),
            record(3, "m-1", "land", "rust", 90, 100, at(10, 12)),
            record(4, "m-2", "tip", "engine", 60, 120, at(11, 40), "fail", "k2"),
            record(5, "m-1", "tip", "rust", 120, 480, at(12, 40), commit="k3"),
            record(6, "m-2", "cached", "format", 0, 0, at(10, 12)),
        ],
        metrics=[
            *(metric("m-1", "jarvis", d) for d in export_lines(tmp_path, "jarvis")),
            *(metric("m-2", "cygnus", d) for d in export_lines(tmp_path, "cygnus")),
            metric("m-1", "plan_usage", json.dumps(sample)),
        ],
        state={
            "tickets": [
                ["t", at(10, 0), None, None, 0],
                ["t.1", at(10, 0), at(10, 0), at(11, 0), 1],
            ],
            "tips": [["k1", at(10, 30), at(10, 45), "verified"]],
            "regressions": [
                {
                    "check_name": "engine:smoke",
                    "ticket": "t.1",
                    "first_bad": "k2",
                    "last_good": "k1",
                    "state": "fixed",
                    "platform": None,
                    "culprit_ticket": "t.2",
                }
            ],
            "handoffs": [],
            "workers": [["m-1", "worker-1", "worker"]],
        },
    )
