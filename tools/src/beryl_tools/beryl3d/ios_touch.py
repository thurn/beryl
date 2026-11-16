"""`just b3d ios-smoke --touch`: builds the iOS host and its UI test
(`ios/Beryl3DUITests`), runs the test against a live game in the headless Simulator, and checks
the frame log the app wrote with `phone_log.ios_touch_faults`: each touch, the rotation, a
nonzero top inset, and no frame in the last 2 seconds before the test suspends the app.
"""

from __future__ import annotations

import subprocess
import time
from pathlib import Path

from .. import mobile, paths as repo_paths, system
from . import phone
from .phone_log import ios_touch_faults, lines

# How long the UI test may run before it counts as hung.
UI_TEST_LIMIT_S = 600


def run_ios_touch() -> int:
    """`ios-smoke --touch`: builds the app and its UI test, runs the test against a live game
    in the simulator, and checks the frame log the app wrote."""
    bundle = phone.ios_app("iphonesimulator", release=False, sign=False, tests=True)
    products = bundle.parent.parent
    xctestrun = next(products.glob("Beryl3D_*.xctestrun"), None)
    if xctestrun is None:
        raise SystemExit(f"xcodebuild wrote no .xctestrun in {products}")
    run = phone.out_dir("phone") / f"{phone.run_name()}-ios-touch"
    run.mkdir()
    with system.file_lock(repo_paths.cache_root() / "ios-smoke.lock"):
        udid, booted = mobile.simulator()
        mobile.boot(udid)
        try:
            mobile.install(udid, bundle)
            container = mobile.run(
                "xcrun", "simctl", "get_app_container", udid, phone.BUNDLE, "data"
            )
            documents: Path = Path(container.stdout.strip()) / "Documents"
            documents.mkdir(parents=True, exist_ok=True)
            for old in [
                *documents.glob("*.jsonl"),
                documents / phone.ERRORS,
                documents / phone.LIBRARY_LOG,
            ]:
                old.unlink(missing_ok=True)
            started = time.monotonic()
            try:
                tested = subprocess.run(
                    [
                        *["xcodebuild", "-quiet", "test-without-building"],
                        *["-xctestrun", str(xctestrun)],
                        *["-destination", f"id={udid}"],
                        *["-resultBundlePath", str(run / "result.xcresult")],
                    ],
                    check=False,
                    timeout=UI_TEST_LIMIT_S,
                ).returncode
            except subprocess.TimeoutExpired:
                print(f"ios-smoke --touch: FAILED: the UI test ran {UI_TEST_LIMIT_S} s")
                return 1
            print(
                f"ios-smoke --touch: UI test ran in {time.monotonic() - started:.0f} s"
            )
            # The test runner installs the app again, which moves its data folder.
            container = mobile.run(
                "xcrun", "simctl", "get_app_container", udid, phone.BUNDLE, "data"
            )
            documents = Path(container.stdout.strip()) / "Documents"
            logs = sorted(documents.glob("*.jsonl"))
            log = logs[-1].read_text(encoding="utf-8") if logs else ""
            errors = documents / phone.ERRORS
            if errors.exists():
                print(errors.read_text(encoding="utf-8"))
        finally:
            mobile.run("xcrun", "simctl", "terminate", udid, phone.BUNDLE)
            if not booted:
                mobile.run("xcrun", "simctl", "shutdown", udid)
    saved = run / "frames.jsonl"
    saved.write_text(log, encoding="utf-8")
    print(f"ios-smoke --touch: frame log {saved}")
    if tested != 0:
        print("ios-smoke --touch: FAILED: the UI test failed")
        return 1
    faults, idle = ios_touch_faults(lines(log))
    if idle is not None:
        print(f"ios-smoke --touch: {idle:.0f} ms without a frame when suspended")
    for fault in faults:
        print(f"ios-smoke --touch: FAILED: {fault}")
    if faults:
        return 1
    print("ios-smoke --touch: passed")
    return 0
