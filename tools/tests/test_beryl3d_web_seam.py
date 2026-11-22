"""The files for the web projection can be removed when Beryl switches clients."""

import re
import subprocess

from beryl_tools import paths
from beryl_tools.beryl3d.paths import WORKSPACE


def test_web_parity_removal_paths_exist() -> None:
    architecture = (paths.REPO / "docs/beryl-3d-architecture.md").read_text()
    seam = architecture.split("## The web-parity seam\n", 1)[1].split("\n## ", 1)[0]
    removal_paths = re.findall(r"`((?:rust|tools|data)/[^`]+)`", seam)
    assert removal_paths
    assert not [name for name in removal_paths if not (paths.REPO / name).exists()]


def test_app_client_type_checks_without_web_fit() -> None:
    checked = subprocess.run(
        ["cargo", "check", "--locked", "-p", "app_client", "--no-default-features"],
        cwd=WORKSPACE,
        capture_output=True,
        text=True,
        check=False,
    )
    assert checked.returncode == 0, checked.stderr
