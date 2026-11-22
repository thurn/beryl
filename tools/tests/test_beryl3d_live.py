"""`just b3d live`: which socket a request goes to, and how it is sent and answered."""

import argparse
import socket
import tempfile
import threading
from collections.abc import Iterator
from pathlib import Path

import pytest

from beryl_tools import system
from beryl_tools.beryl3d import live, play, stand_in


def running(_pid: int) -> bool:
    return True


def make(folder: Path, pid: int) -> Path:
    path = folder / f"beryl3d-{pid}.sock"
    path.touch()
    return path


def test_with_no_socket_the_error_says_how_to_start_an_app(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="just b3d app --fixture pay-default"):
        live.choose(tmp_path, None, running)


def test_the_only_socket_is_chosen_without_a_process_id(tmp_path: Path) -> None:
    only = make(tmp_path, 123)
    (tmp_path / "unrelated.sock").touch()
    assert live.choose(tmp_path, None, running) == only
    assert live.choose(tmp_path, 123, running) == only


def test_with_two_sockets_a_process_id_is_needed_and_the_error_lists_them(
    tmp_path: Path,
) -> None:
    first, second = make(tmp_path, 123), make(tmp_path, 456)
    with pytest.raises(ValueError, match=r"123, 456.*--app"):
        live.choose(tmp_path, None, running)
    assert live.choose(tmp_path, 456, running) == second
    with pytest.raises(ValueError, match="no app with process id 789.*123, 456"):
        live.choose(tmp_path, 789, running)
    assert first != second


def test_a_socket_whose_app_has_ended_is_removed_and_not_chosen(
    tmp_path: Path,
) -> None:
    ended, live_one = make(tmp_path, 123), make(tmp_path, 456)
    assert live.choose(tmp_path, None, lambda pid: pid == 456) == live_one
    assert not ended.exists()


def ask(path: Path, line: str, answer: bytes, asked: list[str]) -> tuple[bool, str]:
    # The app uses Unix sockets. Windows Python exercises the same framing
    # over a local TCP stream since its socket module has no AF_UNIX.
    family = socket.AF_INET if system.WINDOWS else socket.AF_UNIX
    listener: socket.socket = socket.socket(family, socket.SOCK_STREAM)
    with listener:
        listener.settimeout(live.LIMIT_S)
        listener.bind(("127.0.0.1", 0) if system.WINDOWS else str(path))
        listener.listen(1)

        def run() -> None:
            connection, _ = listener.accept()
            with connection, connection.makefile() as incoming:
                asked.append(incoming.readline().strip())
                connection.sendall(answer)

        thread = threading.Thread(target=run)
        thread.start()
        try:
            if system.WINDOWS:
                with socket.create_connection(
                    listener.getsockname(), timeout=live.LIMIT_S
                ) as connection:
                    return live.exchange(connection, line)
            return live.request(path, line)
        finally:
            thread.join()


@pytest.fixture
def socket_path() -> Iterator[Path]:
    # The native sockaddr has a fixed-length path. Keep the name short even when pytest's
    # directory is deep.
    with tempfile.TemporaryDirectory(dir=live.socket_dir(), prefix="b3d-") as directory:
        path = Path(directory) / "s"
        yield path
        path.unlink(missing_ok=True)


def test_a_request_is_one_line_and_the_answer_is_read_to_the_end(
    socket_path: Path,
) -> None:
    asked: list[str] = []
    assert ask(socket_path, "tree", b"ok\ngame\n  table\n", asked) == (
        True,
        "game\n  table\n",
    )
    assert asked == ["tree"]


def test_an_error_answer_is_not_accepted(socket_path: Path) -> None:
    assert ask(socket_path, "do press button", b"error\nno button", []) == (
        False,
        "no button",
    )


def test_a_binary_with_the_socket_name_is_found(tmp_path: Path) -> None:
    clean = tmp_path / "clean"
    clean.write_bytes(b"\x00beryl3d\x00beryl_3d")
    assert live.release_socket_paths(clean) == []
    dirty = tmp_path / "dirty"
    dirty.write_bytes(b"ab" + b"beryl3d-" + b"1.sock")
    assert live.release_socket_paths(dirty) == [2]
    objects = tmp_path / "objects"
    objects.write_bytes(b"target/release/deps/beryl3d-dc7ac5ee12d5f047.beryl_3d.cgu.o")
    assert live.release_socket_paths(objects) == []
    unit = tmp_path / "unit"
    unit.write_bytes(b"\x01beryl3d-dc7ac5ee12d5f047\x00")
    assert live.release_socket_paths(unit) == []
    folder = tmp_path / "folder"
    folder.write_bytes(b"\rberyl3d-tablesberyl3d: search: nothing was searched\x00")
    assert live.release_socket_paths(folder) == []
    encoded = tmp_path / "encoded"
    encoded.write_bytes(b"mod.rs\x00\x08beryl3d-\xc0\x05.sock\x00")
    assert live.release_socket_paths(encoded) == [8]


def test_a_duration_is_a_whole_number_with_a_unit_and_at_most_an_hour() -> None:
    assert play.seconds("90s") == 90
    assert play.seconds("20m") == 1200
    assert play.seconds("1h") == 3600
    for bad in ("2h", "0s", "10", "1.5m", "m"):
        with pytest.raises(ValueError, match="--for"):
            play.seconds(bad)


def test_without_for_play_exits_2(capsys: pytest.CaptureFixture[str]) -> None:
    args = argparse.Namespace(run_for=None, opponent="mcts-hidden", no_build=True)
    assert play.run_play(args) == 2
    assert "--for is required" in capsys.readouterr().err


def test_the_follow_up_names_the_pid_the_socket_and_each_live_command() -> None:
    path = live.socket_dir() / "beryl3d-4242.sock"
    text = play.follow_up(4242, path)
    assert f"process id 4242, socket {path}" in text
    for what in ("tree", "png", "do", "quit"):
        assert f"just b3d live {what} --app 4242" in text


def test_play_prints_them_when_the_game_is_on_screen(
    monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str], tmp_path: Path
) -> None:
    class Process:
        pid = 4242

        def poll(self) -> None:
            return None

    pressed: list[str] = []

    class Running:
        def __init__(self, process: object, lines: list[str]) -> None:
            self.socket = Path("/tmp/beryl3d-4242.sock")

        def act(self, action: str) -> None:
            pressed.append(action)

    monkeypatch.setattr(play.paths, "cache", lambda: tmp_path)
    monkeypatch.setattr(play, "launch", lambda *_: Process())
    monkeypatch.setattr(play, "wait_log", lambda *_: [])
    monkeypatch.setattr(play.stand_in, "Running", Running)
    monkeypatch.setattr(play.stand_in, "wait_tree", lambda *_: None)
    args = argparse.Namespace(run_for="60s", opponent="mcts-hidden", no_build=True)
    assert play.run_play(args) == 0
    assert pressed == ['press button "Start"']
    assert "just b3d live tree --app 4242" in capsys.readouterr().out


def test_another_opponent_is_found_by_the_name_of_its_toggle() -> None:
    tree = stand_in.parse_tree(
        't=0ms\nregion "start"\n  toggle "Searcher" #1 0,0 1x1\n  toggle "Expert" #2 0,0 1x1\n'
    )
    assert play.toggle_title(tree, "expert") == "Expert"
    with pytest.raises(stand_in.StandInError, match="Searcher, Expert"):
        play.toggle_title(tree, "random")
