"""Exercise the Android host across Home, surface recreation and platform requests."""

import time
from collections.abc import Callable
from typing import Any

from .phone import ACTIVITY, BUNDLE, adb, adb_checked, wait_for_log
from .phone_log import (
    logged_inputs,
    resumed_entries,
    resumed_surface_was_shown,
    suspension_bounds,
)


def host_log() -> str:
    return adb("shell", "run-as", BUNDLE, "cat", "files/host.log").stdout


def request(what: str) -> str | None:
    # A shell UID cannot reach the non-exported debug receiver. Send as the installed
    # debug app. Name the emulator's owner explicitly because app UIDs cannot resolve
    # Android's default "current or self" cross-user target.
    sent = adb(
        "shell",
        "run-as",
        BUNDLE,
        "am",
        "broadcast",
        "--user",
        "0",
        "-a",
        f"{BUNDLE}.TEST",
        "-p",
        BUNDLE,
        "--es",
        "what",
        what,
    )
    if sent.returncode:
        return f"could not send {what}: {sent.stderr or sent.stdout}".strip()
    deadline = time.monotonic() + 30
    while time.monotonic() < deadline:
        for line in host_log().splitlines():
            if line == what:
                return None
            if line.startswith(f"{what} failed:"):
                return line
        time.sleep(1)
    return f"the host did not carry out {what}"


def return_to_app() -> None:
    adb_checked(
        "shell",
        "am",
        "start",
        "-W",
        "--activity-clear-top",
        "--activity-single-top",
        "-n",
        ACTIVITY,
    )


def exercise(
    read: Callable[[], str | None],
    errors: Callable[[], str],
    label: str,
) -> tuple[str | None, list[str]]:
    pid: str = adb("shell", "pidof", BUNDLE).stdout.strip()

    def same_process() -> bool:
        return bool(pid) and adb("shell", "pidof", BUNDLE).stdout.strip() == pid

    if wait_for_log(read, same_process, errors, label, event="first_shown_ms") is None:
        return read(), ["the first frame was never shown"]

    adb_checked("shell", "input", "keyevent", "KEYCODE_HOME")
    if (
        wait_for_log(
            read,
            same_process,
            errors,
            label,
            event="suspension and surface loss",
            ready=lambda log: any(
                event["event"] == "Suspended"
                for event in logged_inputs(log, "lifecycle")
            )
            and "surface-lost" in host_log().splitlines(),
        )
        is None
    ):
        return read(), ["Home did not suspend the app and destroy its surface"]

    return_to_app()
    adb_checked("shell", "am", "send-trim-memory", BUNDLE, "RUNNING_CRITICAL")

    def resumed_and_trimmed(parsed: list[dict[str, Any]]) -> bool:
        cycle = suspension_bounds(parsed)
        if cycle is None:
            return False
        after = resumed_entries(parsed, cycle[1])
        return resumed_surface_was_shown(after) and any(
            event["event"] == "LowMemory" for event in logged_inputs(after, "lifecycle")
        )

    if (
        wait_for_log(
            read,
            same_process,
            errors,
            label,
            event="resumed frame and low memory",
            ready=resumed_and_trimmed,
        )
        is None
    ):
        return read(), ["the resumed frame or low-memory event did not arrive"]

    for what in ("haptic", "share"):
        fault = request(what)
        if fault:
            return read(), [fault]

    # Sharing can pause or obscure Beryl. Bring it forward for the final frame and PNG.
    return_to_app()
    log = wait_for_log(read, same_process, errors, label)
    return (
        (log, [])
        if log is not None
        else (read(), ["the replay did not finish after resume"])
    )
