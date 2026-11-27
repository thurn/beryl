"""`just client ios-device --ota`: installs the app on the user's iPhone when it is not on the
Mac's Wi-Fi, over Tailscale.

`devicectl` reaches a phone over USB or over the local network: CoreDevice finds a phone by
Bonjour, which does not cross Tailscale, and it has no option to connect to an address. iOS can
also install an app from a web page: a link to `itms-services://?action=download-manifest&url=
<manifest>` makes the phone fetch a manifest plist, which names the app's `.ipa` file, and
install it. The manifest and the `.ipa` must come over HTTPS with a certificate the phone
trusts, and the app must be signed for that phone. The development profile that `ios_app`
signs with already lists every phone registered to the team, and `tailscale serve` puts a
valid certificate for the Mac's `*.ts.net` name in front of a local port.

So this command packs the signed `Beryl.app` as an `.ipa`, writes the manifest and a page with
the install link, serves the folder from a local HTTP server, and runs `tailscale serve` in
front of it on a port that only the tailnet reaches, until it is stopped. The Tailscale app
from the Mac App Store cannot serve a folder itself (its sandbox forbids it), which is why the
local server exists. The user opens the printed page on the phone, with Tailscale on, and taps
Install.
"""

from __future__ import annotations

import html
import http.server
import json
import plistlib
import shutil
import subprocess
import threading
import time
import zipfile
from functools import partial
from pathlib import Path
from typing import Any

# The HTTPS port `tailscale serve` listens on. Not 443, which may already be a public Funnel,
# and serving on another port with plain `tailscale serve` never makes it public.
HTTPS_PORT = 8445
TITLE = "Beryl"


def tailnet_name() -> str:
    """This Mac's name on the tailnet, such as `jarvis.tail1234.ts.net`."""
    shown = subprocess.run(
        ["tailscale", "status", "--json"], capture_output=True, text=True, check=False
    )
    if shown.returncode != 0:
        raise SystemExit(
            f"ios-device: `tailscale status` failed, so Tailscale is not running:"
            f" {shown.stderr.strip()}"
        )
    status: dict[str, Any] = json.loads(shown.stdout)
    if status.get("BackendState") != "Running":
        raise SystemExit("ios-device: Tailscale is not connected on this Mac")
    return str(status["Self"]["DNSName"]).rstrip(".")


def pack_ipa(bundle: Path, ipa: Path) -> None:
    """Writes `bundle` as an `.ipa`: a zip holding `Payload/<name>.app`. zipfile keeps each
    file's mode, so the app's executable stays executable."""
    with zipfile.ZipFile(ipa, "w", zipfile.ZIP_DEFLATED) as packed:
        for path in sorted(bundle.rglob("*")):
            if path.is_file():
                packed.write(path, f"Payload/{bundle.name}/{path.relative_to(bundle)}")


def manifest(bundle: Path, ipa_url: str) -> bytes:
    """The plist that an `itms-services` link names: where the `.ipa` is, and the app's
    identifier and version, which must match the app's own Info.plist."""
    info: dict[str, Any] = plistlib.loads((bundle / "Info.plist").read_bytes())
    item = {
        "assets": [{"kind": "software-package", "url": ipa_url}],
        "metadata": {
            "bundle-identifier": info["CFBundleIdentifier"],
            "bundle-version": info.get("CFBundleShortVersionString", "1.0"),
            "kind": "software",
            "title": TITLE,
        },
    }
    return plistlib.dumps({"items": [item]})


def install_page(manifest_url: str, built: str) -> str:
    """The page the user opens on the phone. Its link starts the install."""
    link = "itms-services://?action=download-manifest&url=" + manifest_url
    return (
        "<!doctype html><meta name=viewport content='width=device-width'>"
        f"<title>{TITLE}</title>"
        "<body style='font:20px -apple-system;text-align:center;padding-top:30vh'>"
        f"<p><a href='{html.escape(link)}'>Install {TITLE}</a>"
        f"<p style='color:gray;font-size:14px'>Built {html.escape(built)}</body>"
    )


def stage(bundle: Path, folder: Path, base_url: str) -> str:
    """Writes the `.ipa`, manifest and page into `folder`, which is served at `base_url`, and
    returns the page's address. Each build's files get new names, so neither Safari nor the
    installer uses an older build it has cached."""
    shutil.rmtree(folder, ignore_errors=True)
    folder.mkdir(parents=True)
    stamp = str(int(time.time()))
    pack_ipa(bundle, folder / f"Beryl-{stamp}.ipa")
    (folder / f"manifest-{stamp}.plist").write_bytes(
        manifest(bundle, f"{base_url}/Beryl-{stamp}.ipa")
    )
    built = time.strftime("%Y-%m-%d %H:%M")
    page = install_page(f"{base_url}/manifest-{stamp}.plist", built)
    (folder / "index.html").write_text(page, encoding="utf-8")
    return f"{base_url}/"


class Handler(http.server.SimpleHTTPRequestHandler):
    extensions_map = {
        ".html": "text/html",
        ".plist": "text/xml",
        ".ipa": "application/octet-stream",
    }

    def log_message(self, format: str, *args: Any) -> None:
        print(f"ios-device: served {self.path} ({args[1] if len(args) > 1 else ''})")


def serve(bundle: Path, folder: Path) -> int:
    """Stages the install page for `bundle` and serves it over the tailnet until stopped."""
    name = tailnet_name()
    base_url = f"https://{name}:{HTTPS_PORT}"
    page = stage(bundle, folder, base_url)
    server = http.server.ThreadingHTTPServer(
        ("127.0.0.1", 0), partial(Handler, directory=str(folder))
    )
    local = f"http://127.0.0.1:{server.server_address[1]}"
    threading.Thread(target=server.serve_forever, daemon=True).start()
    print(f"ios-device: on the iPhone, with Tailscale on, open {page} and tap Install")
    print("ios-device: serving until stopped (Ctrl-C)")
    try:
        # Without --bg, `tailscale serve` runs until it is stopped and then removes its
        # configuration, so nothing is left serving after this command ends.
        return subprocess.run(
            ["tailscale", "serve", f"--https={HTTPS_PORT}", local], check=False
        ).returncode
    except KeyboardInterrupt:
        return 0
    finally:
        server.shutdown()
