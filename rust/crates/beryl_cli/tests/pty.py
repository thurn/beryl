"""Runs a command in a pseudo-terminal, as at a person's terminal, and presses Ctrl-C.

usage: pty.py OUT ROWS COLS PRESSES GAMES -- COMMAND...

The first press comes once the output shows a decision being played, a second once the
panel shows the stop asked for (⏸). Writes everything the command printed to OUT and prints
a JSON object: the command's exit code, and before and after each press, how many lines
GAMES (a games.jsonl) held. Standard library only; tests/longthink_tty.rs runs it.
"""

import json
import os
import pty
import select
import struct
import sys
import time
from fcntl import ioctl
from termios import TIOCSWINSZ

# A run that never ends is a bug, not a slow machine: this only keeps a test from hanging.
DEADLINE_S = 600


def lines(path: str) -> int:
    try:
        with open(path, "rb") as f:
            return f.read().count(b"\n")
    except FileNotFoundError:
        return 0


def main() -> int:
    out, rows, cols, presses, games = sys.argv[1:6]
    assert sys.argv[6] == "--"
    command = sys.argv[7:]
    pid, fd = pty.fork()
    if pid == 0:
        ioctl(0, TIOCSWINSZ, struct.pack("HHHH", int(rows), int(cols), 0, 0))
        os.execvp(command[0], command)
    seen = b""
    marks = [b"decision ", "⏸".encode()]
    counts: list[list[int]] = []
    start = time.monotonic()
    while True:
        if time.monotonic() - start > DEADLINE_S:
            os.kill(pid, 9)
        ready, _, _ = select.select([fd], [], [], 0.05)
        if not ready:
            continue
        try:
            chunk = os.read(fd, 65536)
        except OSError:
            chunk = b""
        if not chunk:
            break
        seen += chunk
        n = len(counts)
        # The second mark must come after the first press.
        since = seen[counts[-1][2] :] if counts else seen
        if n < int(presses) and marks[n] in since:
            before = lines(games)
            os.write(fd, b"\x03")
            counts.append([before, lines(games), len(seen)])
    _, status = os.waitpid(pid, 0)
    with open(out, "wb") as f:
        f.write(seen)
    print(
        json.dumps(
            {
                "code": os.waitstatus_to_exitcode(status),
                "presses": [c[:2] for c in counts],
            }
        )
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
