"""`just client ios-device --ota`: the files the iPhone fetches to install the app from a page.
The page's link names the manifest, the manifest names the `.ipa` and the app's identifier,
and the `.ipa` holds the app under `Payload/` with its executable still executable.
"""

import plistlib
import re
import stat
import zipfile
from pathlib import Path

from beryl_tools.client import phone_ota

BASE = "https://mac.tailnet.ts.net:8445"


def app(tmp_path: Path) -> Path:
    bundle = tmp_path / "Beryl.app"
    bundle.mkdir()
    info = {"CFBundleIdentifier": "ca.thurn.beryl", "CFBundleShortVersionString": "0.3"}
    (bundle / "Info.plist").write_bytes(plistlib.dumps(info))
    (bundle / "Beryl").write_bytes(b"binary")
    (bundle / "Beryl").chmod(0o755)
    return bundle


def test_the_page_links_to_a_manifest_that_names_the_served_ipa(
    tmp_path: Path,
) -> None:
    served = tmp_path / "served"
    page = phone_ota.stage(app(tmp_path), served, BASE)
    assert page == f"{BASE}/"
    index = (served / "index.html").read_text()
    link = re.search(r"itms-services://\?action=download-manifest&amp;url=(\S+?)'", index)
    assert link is not None and link[1].startswith(f"{BASE}/manifest-")
    manifest = plistlib.loads((served / link[1].removeprefix(f"{BASE}/")).read_bytes())
    item = manifest["items"][0]
    assert item["metadata"]["bundle-identifier"] == "ca.thurn.beryl"
    assert item["metadata"]["bundle-version"] == "0.3"
    ipa_url = item["assets"][0]["url"]
    assert ipa_url.startswith(f"{BASE}/") and ipa_url.endswith(".ipa")
    assert (served / ipa_url.removeprefix(f"{BASE}/")).exists()


def test_the_ipa_holds_the_app_under_payload_and_keeps_it_executable(
    tmp_path: Path,
) -> None:
    ipa = tmp_path / "Beryl.ipa"
    phone_ota.pack_ipa(app(tmp_path), ipa)
    with zipfile.ZipFile(ipa) as packed:
        names = sorted(packed.namelist())
        mode = packed.getinfo("Payload/Beryl.app/Beryl").external_attr >> 16
    assert names == ["Payload/Beryl.app/Beryl", "Payload/Beryl.app/Info.plist"]
    assert mode & stat.S_IXUSR
