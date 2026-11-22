"""`just b3d live`: drive a running debug build (docs/beryl-3d.md, "Driving the running app").

The app listens on a Unix socket, `$TMPDIR/beryl3d-<process id>.sock`, when it is a build with
the `tools` feature. A request is one line (`tree`, `png`, `do <action>` or `quit`) and the answer is
read to the end of the connection: `ok` or `error` on its first line, then the tree, the
picture's path or what went wrong. `release_socket_paths` is the check that a release build
contains no such path.
"""

from __future__ import annotations

import argparse
import ctypes
import os
import re
import select
import socket
import sys
import tempfile
from pathlib import Path
from typing import Callable, Literal

from .. import deps, system

# How long a request may take before it gives up, in seconds.
LIMIT_S = 10.0
SOCKET: re.Pattern[str] = re.compile(r"beryl3d-(\d+)\.sock")
# What a release binary must not contain: the format string of the socket's file name,
# `beryl3d-{pid}.sock` (live.rs). The compiler stores it as the text `beryl3d-`, a few bytes
# that describe the argument, and the text `.sock`, so `.sock` must follow within 16 bytes.
# Other names that start with `beryl3d-` are not sockets: the manual table's temporary folder
# `beryl3d-tables`, and the object files and compilation unit that the debug info names,
# `deps/beryl3d-<16 hex digits>.<crate>...` and `beryl3d-<16 hex digits>`.
SOCKET_NAME: re.Pattern[bytes] = re.compile(rb"beryl3d-.{0,16}?\.sock", re.DOTALL)


def socket_dir() -> Path:
    override = os.environ.get("TMPDIR")
    if system.WINDOWS:
        return Path(
            override
            or os.environ.get("TMP")
            or os.environ.get("TEMP")
            or tempfile.gettempdir()
        )
    return Path(override or "/tmp")


def sockets(
    folder: Path, running: Callable[[int], bool] = system.alive
) -> dict[int, Path]:
    """The live sockets in `folder`, by the process id in their names. A socket whose process
    has ended is left by an app that was killed before it could remove it (`just b3d smoke`
    ends the app with a signal): it is deleted and not listed."""
    found: dict[int, Path] = {}
    for path in sorted(folder.glob("beryl3d-*.sock")):
        match = SOCKET.fullmatch(path.name)
        if not match:
            continue
        pid = int(match.group(1))
        if running(pid):
            found[pid] = path
        else:
            path.unlink(missing_ok=True)
    return found


def choose(
    folder: Path, app: int | None, running: Callable[[int], bool] = system.alive
) -> Path:
    """The socket to speak to: the one with process id `app`, or the only one. Raises
    ValueError, naming what exists, when there is none, several or no such process."""
    found = sockets(folder, running)
    if app is not None:
        if app in found:
            return found[app]
        listed = ", ".join(map(str, found)) or "none"
        raise ValueError(
            f"no app with process id {app} has a socket in {folder}; they are: {listed}"
        )
    if not found:
        raise ValueError(
            f"no running app has a socket in {folder}; start one with"
            " `just b3d app --fixture pay-default --for 20s` in the background"
        )
    if len(found) > 1:
        listed = ", ".join(map(str, found))
        raise ValueError(
            f"several apps have sockets in {folder} (process ids {listed}); name one with --app"
        )
    return next(iter(found.values()))


# The Windows Winsock family number. The project's Windows Python can send and receive
# through this socket but does not expose AF_UNIX or marshal its path addresses.
WINDOWS_AF_UNIX = 1


def windows_address(
    connection: socket.socket, operation: Literal["connect", "bind"], path: Path
) -> int:
    """Call Winsock with a Windows sockaddr_un and return its error code, or zero."""

    class Address(ctypes.Structure):
        _fields_ = [("family", ctypes.c_ushort), ("path", ctypes.c_char * 108)]

    encoded = os.fsencode(path)
    if len(encoded) >= 108 or b"\0" in encoded:
        raise OSError(f"invalid Unix socket path: {path}")
    address = Address(WINDOWS_AF_UNIX, encoded)
    winsock = ctypes.windll.ws2_32  # pyre-ignore[16]: Windows only
    call = getattr(winsock, operation)
    size = ctypes.sizeof(ctypes.c_ushort) + len(encoded) + 1
    if call(ctypes.c_size_t(connection.fileno()), ctypes.byref(address), size) == 0:
        return 0
    return int(winsock.WSAGetLastError())


def connect(connection: socket.socket, path: Path, limit: float) -> None:
    if not system.WINDOWS:
        connection.connect(str(path))
        return
    error = windows_address(connection, "connect", path)
    if error == 10035:  # WSAEWOULDBLOCK: the nonblocking connect is pending.
        _, ready, failed = select.select([], [connection], [connection], limit)
        if not ready and not failed:
            raise TimeoutError(f"connect: {path}")
        error = connection.getsockopt(socket.SOL_SOCKET, socket.SO_ERROR)
    if error:
        raise OSError(error, f"connect: {path}")


def request(path: Path, line: str, limit: float = LIMIT_S) -> tuple[bool, str]:
    """Sends `line` to the socket at `path`. Gives whether the app said `ok` and the rest of
    its answer. Raises OSError, including TimeoutError, when the app does not answer in time.
    """
    family = WINDOWS_AF_UNIX if system.WINDOWS else socket.AF_UNIX
    with socket.socket(family, socket.SOCK_STREAM) as connection:
        connection.settimeout(limit)
        connect(connection, path, limit)
        return exchange(connection, line)


def exchange(connection: socket.socket, line: str) -> tuple[bool, str]:
    """Send one request on a connected stream and read its response through EOF."""
    connection.sendall(f"{line}\n".encode())
    chunks = []
    while chunk := connection.recv(65536):
        chunks.append(chunk)
    status, _, body = b"".join(chunks).decode(errors="replace").partition("\n")
    return status == "ok", body


def live_arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument(
        "what",
        choices=("tree", "png", "do", "quit"),
        help="what to ask the app; quit ends it as closing its window does",
    )
    parser.add_argument(
        "action", nargs="?", help="with `do`: an action, as `--do` takes one"
    )
    parser.add_argument(
        "--app", type=int, help="the app's process id, when several are running"
    )


def run_live(args: argparse.Namespace) -> int:
    if (args.what == "do") != (args.action is not None):
        print(
            "live: `do` takes an action, and `tree`, `png` and `quit` take none",
            file=sys.stderr,
        )
        return 2
    try:
        path = choose(socket_dir(), args.app)
    except ValueError as error:
        print(f"live: {error}", file=sys.stderr)
        return 2
    line = f"do {args.action}" if args.what == "do" else args.what
    try:
        accepted, body = request(path, line)
    except OSError as error:
        print(
            f"live: the app at {path} did not answer within {LIMIT_S:.0f} s: {error}",
            file=sys.stderr,
        )
        return 1
    if not accepted:
        print(f"live: {body}", file=sys.stderr)
        return 1
    print(body if args.what in ("tree", "png") else "done")
    return 0


def release_socket_paths(binary: Path) -> list[int]:
    """The offsets in `binary` where the live socket's file name appears."""
    return [match.start() for match in SOCKET_NAME.finditer(binary.read_bytes())]


def run_release_socket(_args: argparse.Namespace) -> int:
    """Builds the release app and fails when its binary names a live socket."""
    from .app import RELEASE_BINARY, build_release  # app imports this module.

    if build_release() != 0:
        return deps.CANNOT_RUN
    binary = RELEASE_BINARY
    found = release_socket_paths(binary)
    if found:
        print(
            f"release-socket: FAILED: {binary} contains a path starting 'beryl3d-' at {found[:5]}"
        )
        return 1
    print(f"release-socket: passed: {binary} has no live socket path")
    return 0
