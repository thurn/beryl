"""Open two native debug clients at the same local table."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import threading
import time
from collections.abc import Iterator
from contextlib import contextmanager

from .. import deps, paths as repo_paths, system
from . import app, paths, stand_in
from .command import Command


@contextmanager
def client(profile: str, headless: bool) -> Iterator[stand_in.Running]:
    window = "none" if headless else "foreground"
    command = [str(app.BINARY), "--window", window]
    if headless:
        command += ["--for", "120s"]
    process = subprocess.Popen(
        command,
        stdin=subprocess.DEVNULL,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
        env={**os.environ, "BERYL_PROFILE": profile},
    )
    running = stand_in.Running(process)
    reader = threading.Thread(target=running.read, daemon=True)
    reader.start()
    try:
        stand_in.wait_tree(running, "start")
        yield running
    finally:
        if process.poll() is None:
            try:
                running.quit()
            except stand_in.StandInError:
                process.kill()
        process.wait()
        reader.join()
        if process.stdout is not None:
            process.stdout.close()
        if headless:
            data = stand_in.data_folder(running)
            if data is not None and data.name == profile:
                shutil.rmtree(data, ignore_errors=True)


def join(host: stand_in.Running, guest: stand_in.Running) -> None:
    host.act('press button "Host a table"')
    host.act('type field "table name" "Host"')
    host.act('press toggle "Local only"')
    host.act('press button "Host"')
    prefix = "Invite code "
    deadline = time.monotonic() + stand_in.START_LIMIT_S
    while True:
        texts = host.tree().named("text")
        invite = next(
            (name[len(prefix) :] for name in texts if name.startswith(prefix)), None
        )
        if invite is not None:
            break
        if time.monotonic() > deadline:
            raise stand_in.StandInError("the host did not show an invite code")
        time.sleep(stand_in.POLL_S)
    if not invite.startswith("beryl-local:"):
        raise stand_in.StandInError("the host did not create a localhost invite")
    guest.act('press button "Join a table"')
    guest.act('type field "table name" "Guest"')
    guest.act(f'type field "invite code" {json.dumps(invite)}')
    guest.act('press button "Join"')
    deadline = time.monotonic() + stand_in.START_LIMIT_S
    while not guest.tree().region("lobby"):
        if time.monotonic() > deadline:
            raise stand_in.StandInError("the guest did not reach the host's lobby")
        time.sleep(stand_in.POLL_S)


def run(args: argparse.Namespace) -> int:
    if repo_paths.is_agent() and not args.headless:
        print("table-pair: use --headless when running as an agent", file=sys.stderr)
        return 2
    if not args.no_build and app.build() != 0:
        return deps.CANNOT_RUN
    suffix = f"-headless-{os.getpid()}" if args.headless else ""
    try:
        with (
            system.file_lock(paths.cache() / "table-pair.lock", timeout=0),
            client(f"host{suffix}", args.headless) as host,
            client(f"guest{suffix}", args.headless) as guest,
        ):
            join(host, guest)
            print("table-pair: host and guest joined the same local table", flush=True)
            if not args.headless:
                while host.process.poll() is None and guest.process.poll() is None:
                    time.sleep(0.1)
            return host.process.poll() or guest.process.poll() or 0
    except (OSError, stand_in.StandInError, system.LockHeld) as error:
        print(f"table-pair: {error}", file=sys.stderr)
        return 1


def arguments(parser: argparse.ArgumentParser) -> None:
    parser.add_argument("--no-build", action="store_true")
    parser.add_argument(
        "--headless", action="store_true", help="quit after both clients join"
    )


COMMANDS = (
    Command(
        "table-pair",
        "opens a host and guest at the same native local table",
        (
            ("just b3d table-pair", "open the native lobby in two windows"),
            (
                "just b3d table-pair --headless",
                "verify the native localhost connection",
            ),
        ),
        run,
        arguments,
    ),
)
