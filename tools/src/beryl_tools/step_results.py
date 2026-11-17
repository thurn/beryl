"""Step termination semantics, separate from the checks a step managed to report.

Only `pass` and `fail` are completed test attempts. All other kinds leave required
coverage incomplete, except `deferred`, whose named downstream lane owns the work.
POSIX negative return codes and shell signal exits retain their signal; Windows
NTSTATUS exits retain the raw status without inventing a signal or its sender.
"""

from __future__ import annotations

import signal
from collections.abc import Mapping

from . import system
from .reruns import CANNOT_RUN


def signal_number(code: int | None) -> int | None:
    if code is None or system.WINDOWS:
        return None
    number = -code if code < 0 else code - 128
    return number if number in signal.valid_signals() else None


def kind(code: int | None) -> str:
    """Classifies an exit without inferring who terminated a process."""
    if code is None:
        return "stopped"
    if code == 0:
        return "pass"
    if code == CANNOT_RUN:
        return "cannot-run"
    # Python can expose a Windows NTSTATUS as signed or unsigned.
    if signal_number(code) is not None or (system.WINDOWS and code & 0x80000000):
        return "external"
    return "fail"


def incomplete(record: Mapping[str, object]) -> bool:
    """Also understands old records that predate explicit result kinds."""
    return bool(record.get("timeout")) or record.get("result_kind", "fail") not in (
        "pass",
        "fail",
    )
