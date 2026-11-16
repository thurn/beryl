"""`just b3d live`: which socket a request goes to, and how it is sent and answered."""

import socket
import threading
from pathlib import Path

import pytest

from beryl_tools.beryl3d import live


def make(folder: Path, pid: int) -> Path:
    path = folder / f"beryl3d-{pid}.sock"
    path.touch()
    return path


def test_with_no_socket_the_error_says_how_to_start_an_app(tmp_path: Path) -> None:
    with pytest.raises(ValueError, match="just b3d app --fixture pay-default"):
        live.choose(tmp_path, None)


def test_the_only_socket_is_chosen_without_a_process_id(tmp_path: Path) -> None:
    only = make(tmp_path, 123)
    (tmp_path / "unrelated.sock").touch()
    assert live.choose(tmp_path, None) == only
    assert live.choose(tmp_path, 123) == only


def test_with_two_sockets_a_process_id_is_needed_and_the_error_lists_them(
    tmp_path: Path,
) -> None:
    first, second = make(tmp_path, 123), make(tmp_path, 456)
    with pytest.raises(ValueError, match=r"123, 456.*--app"):
        live.choose(tmp_path, None)
    assert live.choose(tmp_path, 456) == second
    with pytest.raises(ValueError, match="no app with process id 789.*123, 456"):
        live.choose(tmp_path, 789)
    assert first != second


def serve(path: Path, answer: bytes, asked: list[str]) -> threading.Thread:
    listener = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    listener.bind(str(path))
    listener.listen(1)

    def run() -> None:
        connection, _ = listener.accept()
        with connection, listener:
            asked.append(connection.makefile().readline().strip())
            connection.sendall(answer)

    thread = threading.Thread(target=run)
    thread.start()
    return thread


def test_a_request_is_one_line_and_the_answer_is_read_to_the_end(
    tmp_path: Path,
) -> None:
    path = Path("/tmp") / f"b3dlive-{tmp_path.name[-8:]}.sock"
    asked: list[str] = []
    thread = serve(path, b"ok\ngame\n  table\n", asked)
    try:
        assert live.request(path, "tree") == (True, "game\n  table\n")
    finally:
        thread.join()
        path.unlink()
    assert asked == ["tree"]


def test_an_error_answer_is_not_accepted(tmp_path: Path) -> None:
    path = Path("/tmp") / f"b3dlive-{tmp_path.name[-8:]}.sock"
    thread = serve(path, b"error\nno button", [])
    try:
        assert live.request(path, "do press button") == (False, "no button")
    finally:
        thread.join()
        path.unlink()


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
