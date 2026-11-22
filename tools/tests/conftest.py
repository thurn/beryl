import shutil
import sys
from pathlib import Path

import pytest


@pytest.fixture
def bash_executable() -> str:
    """Run guest scripts under a local Bash, using the Bash that ships with Windows Git."""
    if sys.platform != "win32":
        return "bash"
    git = shutil.which("git")
    assert git is not None, "Git for Windows is required"
    bash = Path(git).parent.parent / "bin" / "bash.exe"
    assert bash.is_file(), f"Git for Windows Bash was not found at {bash}"
    return str(bash)


@pytest.fixture(autouse=True)
def own_cache(
    tmp_path_factory: pytest.TempPathFactory, monkeypatch: pytest.MonkeyPatch
) -> Path:
    """Every test gets a cache of its own, so that nothing a test caches reaches the machine's
    `$BERYL_CACHE`, and the environment of the process that runs pytest, such as a low-priority
    setting, does not change what a test sees."""
    cache = tmp_path_factory.mktemp("cache")
    monkeypatch.setenv("BERYL_CACHE", str(cache))
    monkeypatch.delenv("BERYL_LOW_PRIORITY", raising=False)
    return cache
