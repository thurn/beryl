"""`just fseventsd`: watch macOS's file-event daemon, and restart it when it has grown too large.

On 2026-11-15, after 12 days of uptime, `fseventsd` had a 72 GB memory footprint, 54 GB of it
compressed or in swap, which was nearly all of the machine's 64 GB of swap. A healthy daemon
uses tens of megabytes. Its memory appears to grow with every file path it has ever seen, and
never shrinks: cargo names each build's object files anew, and every worktree, clone and
temporary directory adds paths. A sample of 20 minutes of file events that day held 142,115
events on 102,907 distinct paths. Beryl's tools now make fewer new paths (`coverage.in_tree`
reuses one tree path; `just client` clones its build directory once per tree), but builds still
make them, so only a restart gives the memory back. launchd starts the daemon again at once.

Restarting it takes root, so this file installs itself as a launchd daemon, which needs the
user to run one command with sudo:

    sudo python3 tools/src/beryl_tools/fseventsd.py install

That copies this file to a root-owned path ([`INSTALLED`]), so that later edits to the
checkout never run as root, and loads a launchd job that runs `check` every [`EVERY`] seconds.
`check` restarts the daemon when its footprint is over [`LIMIT_GB`] and logs each restart to
[`LOG`]. Programs that watch files get one event telling them to rescan; Time Machine and
backup tools may rescan the disk once after a restart.

Standard library only, and it imports nothing from beryl_tools: root runs the installed copy.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import subprocess
import sys
import time
from pathlib import Path

LABEL = "ca.beryl.fseventsd-watchdog"
INSTALLED = Path("/usr/local/libexec/beryl-fseventsd-watchdog.py")
PLIST = Path(f"/Library/LaunchDaemons/{LABEL}.plist")
LOG = Path("/var/log/beryl-fseventsd-watchdog.log")
# The command that installs the watchdog, run from the main checkout: a worktree's copy is
# deleted when its change lands.
COMMAND = "sudo python3 tools/src/beryl_tools/fseventsd.py install"
# A footprint this large is far past healthy (tens of MB) and still far from filling swap: on
# 2026-11-15 the daemon's footprint went from 72 to 74 GB in about 30 minutes while agents built.
LIMIT_GB = 4.0
EVERY = 600
# The Python the launchd job runs: macOS's own, which every Mac with the command-line tools
# has. The Python that ran `install` may be Homebrew's, whose path is removed when Homebrew
# moves to a newer Python, and the job would then fail every run without logging anything.
# This file keeps to Python 3.9 for it.
PYTHON = "/usr/bin/python3"
UNITS = {"B": 1, "K": 1 << 10, "M": 1 << 20, "G": 1 << 30, "T": 1 << 40}


def parse_size(text: str) -> int | None:
    """Bytes in a size as `top` prints it, such as `72G`, `845M+` or `12K-`."""
    match = re.fullmatch(r"(\d+(?:\.\d+)?)([BKMGT])[+-]?", text.strip())
    if not match:
        return None
    return int(float(match.group(1)) * UNITS[match.group(2)])


def daemon_pid() -> int | None:
    out = subprocess.run(
        ["pgrep", "-x", "fseventsd"], capture_output=True, text=True, check=False
    ).stdout.split()
    return int(out[0]) if out else None


def footprint(pid: int) -> int | None:
    """The process's memory footprint in bytes, compressed and swapped pages included: what
    Activity Monitor calls Memory. `top` reads it for any process without root."""
    out = subprocess.run(
        ["top", "-l", "1", "-pid", str(pid), "-stats", "mem"],
        capture_output=True,
        text=True,
        check=False,
    ).stdout.splitlines()
    return parse_size(out[-1]) if out else None


def check(limit_gb: float) -> int:
    """Restarts the daemon if its footprint is over `limit_gb`. Run by launchd as root."""
    pid = daemon_pid()
    size = footprint(pid) if pid else None
    if pid is None or size is None or size <= limit_gb * UNITS["G"]:
        return 0
    os.kill(pid, 15)
    stamp = time.strftime("%Y-%m-%dT%H:%M:%S%z")
    line = f"{stamp} restarted fseventsd (pid {pid}) at {size / UNITS['G']:.1f} GB\n"
    with open(LOG, "a") as log:
        log.write(line)
    return 0


def plist(limit_gb: float) -> str:
    arguments = [PYTHON, str(INSTALLED), "check", "--limit-gb", str(limit_gb)]
    strings = "".join(f"\n        <string>{a}</string>" for a in arguments)
    return f"""<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LABEL}</string>
    <key>ProgramArguments</key>
    <array>{strings}
    </array>
    <key>StartInterval</key>
    <integer>{EVERY}</integer>
    <key>RunAtLoad</key>
    <true/>
</dict>
</plist>
"""


def install(limit_gb: float) -> int:
    if os.geteuid() != 0:
        print(f"fseventsd: run with sudo: {COMMAND}")
        return 2
    INSTALLED.parent.mkdir(parents=True, exist_ok=True)
    shutil.copyfile(Path(__file__).resolve(), INSTALLED)
    os.chown(INSTALLED, 0, 0)
    INSTALLED.chmod(0o755)
    subprocess.run(["launchctl", "bootout", f"system/{LABEL}"], capture_output=True)
    PLIST.write_text(plist(limit_gb))
    os.chown(PLIST, 0, 0)
    PLIST.chmod(0o644)
    loaded = subprocess.run(["launchctl", "bootstrap", "system", str(PLIST)])
    if loaded.returncode == 0:
        print(f"fseventsd: watchdog installed; restarts are logged to {LOG}")
    return loaded.returncode


def uninstall() -> int:
    if os.geteuid() != 0:
        print(f"fseventsd: run with sudo: {COMMAND.replace('install', 'uninstall')}")
        return 2
    subprocess.run(["launchctl", "bootout", f"system/{LABEL}"], capture_output=True)
    PLIST.unlink(missing_ok=True)
    INSTALLED.unlink(missing_ok=True)
    return 0


def status(_args: argparse.Namespace) -> int:
    """Prints the daemon's footprint and whether the watchdog is installed."""
    if sys.platform != "darwin":
        print("fseventsd: macOS only")
        return 0
    pid = daemon_pid()
    size = footprint(pid) if pid else None
    shown = "unknown" if size is None else f"{size / UNITS['G']:.1f} GB"
    print(f"fseventsd: pid {pid}, memory footprint {shown} (healthy: under 0.1 GB)")
    if PLIST.exists():
        print(
            f"fseventsd: watchdog installed; restarts over {LIMIT_GB:g} GB, logged to {LOG}"
        )
        if LOG.exists():
            print("".join(LOG.read_text().splitlines(keepends=True)[-5:]), end="")
    else:
        print(
            "fseventsd: no watchdog; to install one, read tools/src/beryl_tools/fseventsd.py\n"
            f"and run this from the main checkout:\n  {COMMAND}"
        )
    return 0


def register(sub: argparse._SubParsersAction[argparse.ArgumentParser]) -> None:
    p = sub.add_parser(
        "fseventsd", help="show the file-event daemon's memory and its watchdog"
    )
    p.set_defaults(func=status)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    parser.add_argument("action", choices=["check", "install", "uninstall", "status"])
    parser.add_argument("--limit-gb", type=float, default=LIMIT_GB)
    args = parser.parse_args(argv)
    if args.action == "check":
        return check(args.limit_gb)
    if args.action == "install":
        return install(args.limit_gb)
    if args.action == "uninstall":
        return uninstall()
    return status(args)


if __name__ == "__main__":
    sys.exit(main())
