from __future__ import annotations

import os
import subprocess
import sys
import threading
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
    first = pool.acquire("push", 6, never)
    assert first is not None and first.size == 6
    second = pool.acquire("push", 6, never)
    assert second is not None and second.size == 2
    assert pool.acquire("push", 1, once) is None
    first.release()
    second.release()
    third = pool.acquire("push", 8, never)
    assert third is not None and third.size == 8


def test_lower_classes_leave_room_for_higher(tmp_path: Path) -> None:
    pool = Pool(tmp_path, 8)
    # Slow CI may take every token an agent may: three quarters of the pool.
    ci = pool.acquire("ci", 8, never)
    assert ci is not None and ci.size == 6
    # Its tokens count against the agents' ceiling, so an agent waits for them.
    assert pool.acquire("agent", 1, once) is None
    push = pool.acquire("push", 8, never)
    assert push is not None and push.size == 2


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
    held = pool.acquire("push", 8, never)
    assert held is not None
    got: list[int] = []

    def wait() -> None:
        lease = pool.acquire("push", 8, never)
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
