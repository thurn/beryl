"""`just ios-smoke --game <deck>`: plays a whole game of one deck in the headless iOS Simulator.

The app starts a fresh game of the deck from a fixed seed, with the stand-in agent `greedy`
answering for the person and `mcts-hidden-quick` across the table. The run passes when the frame
log's summary has a `result`, no card was drawn blank and no glyph was missing in the last frame,
and the app wrote no error and was still running when the result was read. The only time limit
bounds a hang: a slow game never fails for being slow.
"""

import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import Any

from .. import mobile_sdk as mobile
from .. import paths as repo_paths, system
from .phone import (
    BUNDLE,
    ERRORS,
    LIBRARY_LOG,
    ios_app,
    out_dir,
    run_name,
    wait_for_log,
)
from .phone_log import lines
from .phone_search import STAND_IN, play

OPPONENT = "mcts-hidden-quick"
# The seed of each deck's game: of seeds 1 to 20 played on the desktop with `--agent greedy
# --opponent mcts-hidden-quick --window none`, the one whose game ended in the fewest turns
# (the lowest seed, where several tied). Each comment gives that game's turns and winner.
SEEDS = {
    "wu-control": 1,  # 21 turns, the person's seat (greedy) won.
    "forgetful-fish": 9,  # 9 turns, the person's seat (greedy) won.
}
# How long a game may run before it counts as hung.
GAME_LIMIT_S = 30 * 60
# A game can end within a second of starting, before the card faces and glyphs have streamed in,
# so the assets are checked in the summary the app writes this long after the result.
SETTLE_S = 10


def read_if_present(path: Path) -> str:
    return path.read_text(encoding="utf-8") if path.exists() else ""


def final_summary(log: str) -> dict[str, Any]:
    """The last summary in the log that holds a game result."""
    found = [
        line["summary"]
        for line in lines(log)
        if "summary" in line and line["summary"].get("result")
    ]
    return found[-1] if found else {}


def game_faults(summary: dict[str, Any], alive: bool, error: str) -> list[str]:
    """What is wrong with a finished run: no result, blank cards or missing glyphs in the last
    frame, an error the app wrote, or an app that was no longer running."""
    faults = []
    if not summary.get("result"):
        faults.append("the frame log's summary has no result")
    faceless = summary.get("faceless_cards", 0)
    missing = summary.get("missing_glyphs", 0)
    if faceless or missing:
        faults.append(
            f"the last frame drew {faceless} cards blank with no face and lacked"
            f" {missing} glyphs: the host is not delivering its assets"
        )
    if error:
        faults.append(f"the app wrote an error:\n{error}")
    if not alive:
        faults.append("the app was no longer running when the result was read")
    return faults


def run_ios_game(deck: str) -> int:
    label = f"ios-smoke --game {deck}"
    seed = SEEDS[deck]
    bundle = ios_app("iphonesimulator", release=False, sign=False)
    run = out_dir("phone") / f"{run_name()}-ios-game"
    run.mkdir()
    png = run / "screen.png"
    with system.file_lock(repo_paths.cache_root() / "ios-smoke.lock"):
        udid, booted = mobile.simulator()
        mobile.boot(udid)
        try:
            mobile.install(udid, bundle)
            container = mobile.run(
                "xcrun", "simctl", "get_app_container", udid, BUNDLE, "data"
            )
            documents = Path(container.stdout.strip()) / "Documents"
            documents.mkdir(parents=True, exist_ok=True)
            for old in [
                *documents.glob("*.jsonl"),
                documents / ERRORS,
                documents / LIBRARY_LOG,
            ]:
                old.unlink(missing_ok=True)
            # The live game's replays and ledger, so that the app starts a fresh game.
            shutil.rmtree(documents / "logs", ignore_errors=True)
            environment = {
                "BERYL_CLIENT_STAND_IN": STAND_IN,
                "BERYL_CLIENT_STAND_IN_DECK": deck,
                "BERYL_CLIENT_STAND_IN_OPPONENT": OPPONENT,
                "BERYL_CLIENT_STAND_IN_SEED": str(seed),
            }
            launched = subprocess.run(
                [
                    *["xcrun", "simctl", "launch", "--terminate-running-process"],
                    *[udid, BUNDLE],
                ],
                env={
                    **os.environ,
                    **{
                        f"SIMCTL_CHILD_{key}": value
                        for key, value in environment.items()
                    },
                },
                capture_output=True,
                text=True,
                check=True,
            )
            pid = int(launched.stdout.rsplit(":", 1)[1])

            def read() -> str | None:
                logs = sorted(documents.glob("*.jsonl"))
                return logs[-1].read_text(encoding="utf-8") if logs else None

            def errors() -> str:
                return read_if_present(documents / ERRORS)

            log = wait_for_log(
                read,
                lambda: mobile.alive(pid),
                errors,
                label,
                limit_s=GAME_LIMIT_S,
                event="game result",
                ready=lambda parsed: any(
                    "summary" in line and line["summary"].get("result")
                    for line in parsed
                ),
            )
            alive = mobile.alive(pid)
            if log is not None:
                time.sleep(SETTLE_S)
                alive = alive and mobile.alive(pid)

                def suspend() -> None:
                    # The screenshot shows the table, not the app that replaces it.
                    mobile.run("xcrun", "simctl", "io", udid, "screenshot", str(png))
                    # Opening another app sends the client to the background, which makes it
                    # write a summary.
                    mobile.run(
                        "xcrun", "simctl", "launch", udid, "com.apple.Preferences"
                    )

                log, stopped = play(
                    read, lambda: mobile.alive(pid), errors, suspend, label, play_s=0
                )
                if stopped:
                    print(f"{label}: {stopped}")
            error = errors() + (
                read_if_present(documents / LIBRARY_LOG) if errors() else ""
            )
        finally:
            mobile.run("xcrun", "simctl", "terminate", udid, BUNDLE)
            if not booted:
                mobile.run("xcrun", "simctl", "shutdown", udid)
    if log is None:
        print(f"{label}: FAILED")
        return 1
    saved = png.with_suffix(".jsonl")
    saved.write_text(log, encoding="utf-8")
    summary = final_summary(log)
    print(f"{label}: deck {deck}, seed {seed}")
    print(f"{label}: winner {summary.get('result')}")
    print(f"{label}: turns {summary.get('turns')}")
    print(f"{label}: frames {summary.get('frames')} ({saved})")
    print(f"{label}: screen {png}")
    faults = game_faults(summary, alive, error)
    for fault in faults:
        print(f"{label}: FAILED: {fault}")
    if not faults:
        print(f"{label}: passed")
    return 1 if faults else 0
