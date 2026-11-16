from __future__ import annotations

import json
import os
import subprocess
import sys
import threading
import time
from pathlib import Path

import pytest

from beryl_tools import tokens
from beryl_tools.tokens import Pool


@pytest.fixture(autouse=True)
def outside_lease(monkeypatch: pytest.MonkeyPatch) -> None:
    """The tests may themselves run under a check step's lease."""
    monkeypatch.delenv(tokens.LEASE_ENV, raising=False)
    monkeypatch.delenv(tokens.POOL_ENV, raising=False)
    monkeypatch.delenv(tokens.CLASS_ENV, raising=False)


def never() -> bool:
    return False


def once() -> bool:
    return True


def test_lease_takes_what_is_free_and_releases(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    first = pool.acquire("queue", 6, never)
    assert first is not None and first.size == 6
    second = pool.acquire("queue", 6, never)
    assert second is not None and second.size == 2
    assert pool.acquire("queue", 1, once) is None
    first.release()
    second.release()
    third = pool.acquire("queue", 8, never)
    assert third is not None and third.size == 8


def test_lower_classes_leave_room_for_higher(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    # Slow CI may take every token an agent may: three quarters of the pool.
    ci = pool.acquire("ci", 8, never)
    assert ci is not None and ci.size == 6
    # Its tokens count against the agents' ceiling, so an agent waits for them.
    assert pool.acquire("agent", 1, once) is None
    # A push may take all but an eighth of the pool, which leaves one token to the queue.
    push = pool.acquire("push", 8, never)
    assert push is not None and push.size == 1
    assert pool.acquire("push", 1, once) is None
    queue = pool.acquire("queue", 8, never)
    assert queue is not None and queue.size == 1


def test_sweeps_hold_at_most_a_quarter_of_the_pool(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    sweep = pool.acquire("sweep", 8, never)
    assert sweep is not None and sweep.size == 2
    assert pool.acquire("sweep", 1, once) is None
    # Their tokens count against the classes above, which still find room beside them.
    ci = pool.acquire("ci", 8, never)
    assert ci is not None and ci.size == 4


def test_a_waiting_ci_step_holds_back_sweeps(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    (tmp_path / f"wait-{os.getpid()}-1-ci").write_text(f"{os.getpid()} ci")
    assert pool.acquire("sweep", 1, once) is None


def test_sweep_scripts_take_sweep_tokens(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(tokens.paths, "REPO", tmp_path / "tree")
    assert tokens.script_class("sweep") == "sweep"
    assert tokens.script_class("b3d-sweep") == "sweep"
    assert tokens.script_class("bisect") == "agent"


def test_unheld_drops_a_lease_and_low_priority() -> None:
    env = {
        "PATH": "/bin",
        tokens.LEASE_ENV: "6",
        tokens.CLASS_ENV: "ci",
        "CARGO_BUILD_JOBS": "6",
        tokens.POOL_ENV: "0",
        "BERYL_LOW_PRIORITY": "1",
    }
    assert tokens.unheld(env) == {"PATH": "/bin"}


def test_a_waiting_queue_check_holds_back_pushes(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    (tmp_path / f"wait-{os.getpid()}-1-queue").write_text(f"{os.getpid()} queue")
    assert pool.acquire("push", 1, once) is None
    assert pool.acquire("queue", 8, never) is not None


def test_the_merge_queue_s_check_takes_queue_tokens(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setattr(tokens.paths, "REPO", tmp_path / tokens.QUEUE_TREE)
    assert tokens.script_class("check") == "queue"
    assert tokens.script_class("ci") == "ci"
    monkeypatch.setattr(tokens.paths, "REPO", tmp_path / "some-branch")
    assert tokens.script_class("check") == "push"


def test_a_waiting_higher_class_holds_back_lower_ones(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 4)
    held = pool.acquire("push", 4, never)
    assert held is not None
    (tmp_path / f"wait-{os.getpid()}-1-push").write_text(f"{os.getpid()} push")
    held.release()
    assert pool.acquire("ci", 1, once) is None
    assert pool.acquire("push", 1, never) is not None


def test_tokens_of_an_exited_process_are_taken_back(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 2)
    gone = subprocess.run(
        [sys.executable, "-c", "import os; print(os.getpid())"],
        capture_output=True,
        text=True,
        check=True,
    )
    for i in range(2):
        (tmp_path / str(i)).write_text(f"{gone.stdout.strip()} push")
    lease = pool.acquire("push", 2, once)
    assert lease is not None and lease.size == 2


def test_waiters_of_one_class_share_what_frees_up(tmp_path: Path) -> None:
    pool: Pool = Pool(tmp_path, 8)
    held = pool.acquire("queue", 8, never)
    assert held is not None
    got: list[int] = []

    def wait() -> None:
        lease = pool.acquire("queue", 8, never)
        assert lease is not None
        got.append(lease.size)

    threads = [threading.Thread(target=wait) for _ in range(2)]
    for t in threads:
        t.start()
    # Each thread's waiter file is written before the pool frees up.
    while len(list(tmp_path.glob("wait-*"))) < 2:
        threading.Event().wait(0.05)
    held.release()
    for t in threads:
        t.join()
    assert sorted(got) == [4, 4]


def test_a_command_inside_a_lease_uses_it(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv(tokens.LEASE_ENV, "3")
    lease = Pool(tmp_path, 8).acquire("agent", 8, once)
    assert lease is not None and lease.size == 3 and lease.inherited
    assert not list(tmp_path.iterdir())


def test_pool_off_grants_everything(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    monkeypatch.setenv(tokens.POOL_ENV, "0")
    pool = Pool(tmp_path, 2)
    assert pool.acquire("ci", 5, once) is not None
    assert pool.acquire("ci", 5, once) is not None


def test_lease_environment_sets_every_thread_count() -> None:
    env = tokens.Lease(5, []).env()
    assert env["CARGO_BUILD_JOBS"] == "5"
    assert env["NEXTEST_TEST_THREADS"] == "5"
    assert env[tokens.LEASE_ENV] == "5"
    assert tokens.Lease(0, []).env() == {}


def test_a_lease_passes_its_class_to_commands_inside_it(tmp_path: Path) -> None:
    lease = Pool(tmp_path, 8).acquire("push", 3, never)
    assert lease is not None
    assert lease.env()[tokens.CLASS_ENV] == "push"
    assert tokens.CLASS_ENV not in tokens.Lease(3, []).env()


def test_a_lease_tops_up_with_what_has_freed(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 12)
    small = pool.acquire("push", 4, never)
    others = pool.acquire("push", 8, never)
    assert small is not None and others is not None
    # While the other lanes run, there is nothing to take.
    assert pool.top_up("push", small.size, 8).size == 0
    others.release()
    # Once they finish, the lease grows to what it asked for, and no further.
    extra = pool.top_up("push", small.size, 8)
    assert extra.size == 4
    assert len(pool.holders()) == 8
    extra.release()
    small.release()
    assert not pool.holders()


def test_a_top_up_yields_to_a_waiting_higher_class(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    (tmp_path / f"wait-{os.getpid()}-1-push").write_text(f"{os.getpid()} push")
    assert pool.top_up("agent", 2, 6).size == 0


def test_top_up_outside_a_lease_takes_nothing(monkeypatch: pytest.MonkeyPatch) -> None:
    assert tokens.top_up(8).size == 0
    monkeypatch.setenv(tokens.LEASE_ENV, "3")
    # Without a class, as from a lease taken before leases recorded one.
    assert tokens.top_up(8).size == 0


def test_light_steps_take_no_tokens() -> None:
    assert tokens.step_tokens("repo", "just") == 0
    assert tokens.step_tokens("repo", "just", 1, 18) == 0


def test_heavy_lanes_running_at_once_share_the_pool() -> None:
    # Slow CI starts about ten heavy lanes at once; each takes an even share of the pool.
    assert tokens.step_tokens("rust", "clippy", 9, 18) == 2
    assert tokens.step_tokens("rust", "tests", 3, 18) == 6
    # A lane alone takes at most two thirds, and never less than one token.
    assert tokens.step_tokens("rust", "tests", 1, 18) == 12
    assert tokens.step_tokens("rust", "tests", 30, 18) == 1


def leases_logged(root: Path) -> list[dict[str, object]]:
    log = root / tokens.LEASE_LOG
    if not log.exists():
        return []
    return [json.loads(line) for line in log.read_text().splitlines()]


def test_a_released_lease_logs_one_line(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    blocker = pool.acquire("agent", 5, never)
    assert blocker is not None
    # Agents may hold six of eight tokens, so this lease gets one of the four it wants.
    lease = pool.acquire("agent", 4, never, command="test-rust")
    assert lease is not None and lease.size == 1
    lease.release()
    lease.release()
    lines = leases_logged(tmp_path)
    assert len(lines) == 1
    first = lines[0]
    assert first["class"] == "agent"
    assert first["command"] == "test-rust"
    assert first["cores"] == 1
    assert first["wanted"] == 4
    assert isinstance(first["waited_s"], float)
    assert isinstance(first["held_s"], float)
    assert first["worktree"] == tokens.paths.REPO.name


def test_a_wait_that_gives_up_logs_a_line_with_no_cores(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 4)
    blocker = pool.acquire("queue", 4, never)
    assert blocker is not None
    checks = iter([False, False, True])
    assert pool.acquire("push", 2, lambda: next(checks), command="record") is None
    (line,) = leases_logged(tmp_path)
    assert line["outcome"] == "gave_up"
    assert (line["class"], line["command"], line["cores"], line["wanted"]) == (
        "push",
        "record",
        0,
        2,
    )
    assert float(str(line["waited_s"])) >= 0.0 and line["held_s"] == 0.0
    assert not list(tmp_path.glob(f"{tokens.WAIT_INFO}-*"))
    blocker.release()


# A process that waits for a token of a full pool until it is killed.
WAITER = """
import sys
from pathlib import Path
from beryl_tools.tokens import Pool
Pool(Path(sys.argv[1]), 2).acquire("push", 2, command="cargo build")
"""


def test_a_wait_whose_process_dies_is_logged_by_the_next_lister(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 2)
    blocker = pool.acquire("queue", 2, never)
    assert blocker is not None
    waiter = subprocess.Popen([sys.executable, "-c", WAITER, str(tmp_path)])
    deadline = time.monotonic() + 30
    while not list(tmp_path.glob(f"{tokens.WAIT_INFO}-*")):
        assert time.monotonic() < deadline and waiter.poll() is None
        time.sleep(0.05)
    time.sleep(0.6)
    waiter.kill()
    waiter.wait()
    with tokens.system.file_lock(tmp_path / "lock"):
        assert pool.waiters() == []
    (line,) = [x for x in leases_logged(tmp_path) if x.get("outcome")]
    assert line["outcome"] == "abandoned"
    assert line["command"] == "cargo build" and line["cores"] == 0
    assert float(str(line["waited_s"])) >= 0.4
    assert not list(tmp_path.glob(f"{tokens.WAIT_INFO}-*"))
    blocker.release()


def test_inherited_and_top_up_leases_log_nothing(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pool = Pool(tmp_path, 8)
    held = pool.acquire("push", 2, never)
    assert held is not None
    pool.top_up("push", 2, 6).release()
    monkeypatch.setenv(tokens.LEASE_ENV, "3")
    inner = pool.acquire("agent", 8, once)
    assert inner is not None and inner.inherited
    inner.release()
    assert leases_logged(tmp_path) == []
    held.release()
    assert len(leases_logged(tmp_path)) == 1


def test_a_failed_log_write_does_not_fail_the_release(
    tmp_path: Path, capsys: pytest.CaptureFixture[str]
) -> None:
    pool = Pool(tmp_path, 8)
    (tmp_path / tokens.LEASE_LOG).mkdir()
    lease = pool.acquire("agent", 2, never)
    assert lease is not None
    lease.release()
    assert not list(tmp_path.glob("[0-9]*"))
    assert "could not log the lease" in capsys.readouterr().err


def test_the_lease_log_drops_lines_older_than_it_keeps(tmp_path: Path) -> None:
    log = tmp_path / tokens.LEASE_LOG
    log.write_text(
        json.dumps({"time": "2000-01-01T00:00:00Z"})
        + "\n"
        + json.dumps({"time": "2999-01-01T00:00:00Z"})
        + "\n"
    )
    lease = Pool(tmp_path, 8).acquire("agent", 2, never)
    assert lease is not None
    lease.release()
    times = [line["time"] for line in leases_logged(tmp_path)]
    assert times[0] == "2999-01-01T00:00:00Z" and len(times) == 2


def test_an_elastic_lease_marks_every_token_after_its_first(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    lease = pool.acquire("agent", 4, never, elastic_from=1)
    assert lease is not None and lease.size == 4
    held = pool.holders()
    assert sorted(h.elastic for h in held.values()) == [False, True, True, True]
    extra = pool.top_up("agent", 4, 6, elastic=True)
    assert extra.size == 2
    assert sum(h.elastic for h in pool.holders().values()) == 5
    # Giving back takes only elastic tokens, and drops them from the lease.
    assert pool.give_back(lease, 9) == 3
    assert lease.size == 4 and len(lease.files) == 1
    lease.release()
    extra.release()
    assert not pool.holders()


def test_a_short_lease_waits_for_lower_classes_to_give_back(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    # Agents may hold six of the eight tokens; this build holds all six, five of them elastic.
    build = pool.acquire("agent", 6, never, elastic_from=1)
    assert build is not None and build.size == 6
    assert not pool.higher_waiting("agent")
    got: list[tokens.Lease | None] = []
    push = threading.Thread(target=lambda: got.append(pool.acquire("push", 6, never)))
    push.start()
    # The push could take one token now, but waits for the build's elastic ones instead.
    deadline = time.monotonic() + 5
    while not pool.higher_waiting("agent"):
        assert time.monotonic() < deadline
        time.sleep(0.01)
    assert pool.give_back(build, 5) == 5
    push.join(5)
    granted = got[0] if got else None
    assert granted is not None and granted.size == 6
    granted.release()
    build.release()


def test_a_short_lease_takes_what_is_free_once_it_has_waited(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    pool = Pool(tmp_path, 8)
    build = pool.acquire("agent", 6, never, elastic_from=1)
    assert build is not None
    # Without elastic tokens below, or once `RECLAIM_SECONDS` has passed, nothing waits.
    monkeypatch.setattr(tokens, "RECLAIM_SECONDS", 0.0)
    push = pool.acquire("push", 6, once)
    assert push is not None and push.size == 1
    push.release()
    build.release()
    fixed = pool.acquire("agent", 6, never)
    assert fixed is not None
    monkeypatch.setattr(tokens, "RECLAIM_SECONDS", 60.0)
    push = pool.acquire("push", 6, once)
    assert push is not None and push.size == 1
    push.release()
    fixed.release()


# A program that waits until its growing lease's file says it holds `n` tokens, then prints it.
GROWN_PROGRAM = """
import os, sys, time
path, n = os.environ["BERYL_CPU_TOKENS_FILE"], sys.argv[1]
for _ in range(200):
    if open(path).read().strip() == n:
        break
    time.sleep(0.05)
print(open(path).read().strip())
"""


def test_a_growing_program_lease_takes_tokens_as_they_free(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root: Path = tmp_path / "pool"
    monkeypatch.setattr(tokens, "Pool", lambda: Pool(root, 8))
    monkeypatch.setattr(tokens, "pool_size", lambda: 8)
    monkeypatch.setattr(tokens, "GROW_SECONDS", 0.05)
    monkeypatch.setenv(tokens.CLASS_ENV, "push")
    # Agents hold their ceiling of six tokens, in a lease that cannot shrink.
    held = Pool(root, 8).acquire("agent", 6, never)
    assert held is not None
    agents: tokens.Lease = held
    seen: list[bool] = []

    def release_agents() -> None:
        # While the program is short of the six it wants, it holds agents back as a waiter.
        deadline = time.monotonic() + 5
        while not Pool(root, 8).higher_waiting("agent") and time.monotonic() < deadline:
            time.sleep(0.01)
        seen.append(Pool(root, 8).higher_waiting("agent"))
        agents.release()

    releaser = threading.Thread(target=release_agents)
    releaser.start()
    done = tokens.run_program(
        [sys.executable, "-c", GROWN_PROGRAM, "6"],
        grow=True,
        check=False,
        capture_output=True,
        text=True,
    )
    releaser.join(5)
    assert seen == [True]
    assert done.stdout.strip() == "6"
    # It started with the one token pushes had left and logged what it grew to.
    line = json.loads((root / tokens.LEASE_LOG).read_text().splitlines()[-1])
    assert (line["class"], line["cores"], line["grown_to"]) == ("push", 1, 6)
    assert not Pool(root, 8).holders() and not list(root.glob("wait-*"))


def test_a_program_lease_grows_only_when_asked(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    root = tmp_path / "pool"
    monkeypatch.setattr(tokens, "Pool", lambda: Pool(root, 8))
    monkeypatch.setattr(tokens, "pool_size", lambda: 8)
    monkeypatch.setenv(tokens.CLASS_ENV, "push")
    agents = Pool(root, 8).acquire("agent", 6, never)
    assert agents is not None
    done = tokens.run_program(
        [
            sys.executable,
            "-c",
            "import os; print(os.environ.get('BERYL_CPU_TOKENS_FILE'))",
        ],
        check=False,
        capture_output=True,
        text=True,
    )
    agents.release()
    assert done.stdout.strip() == "None"
    line = json.loads((root / tokens.LEASE_LOG).read_text().splitlines()[-1])
    assert "grown_to" not in line
