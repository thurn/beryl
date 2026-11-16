"""Sending sources into the `beryl3d-bench` machine, shared by the count and GPU gates."""

import os
import subprocess
import threading
from pathlib import Path

import pytest

from beryl_tools.beryl3d import counts, gpu, source


def test_sources_lists_the_files_the_builds_read() -> None:
    files = source.sources()
    assert "rust/beryl3d/Cargo.toml" in files
    assert all(f.split("/")[0] in ("rust", "data", "docs", "ui") for f in files)
    assert not any("/target/" in f or f.endswith(".DS_Store") for f in files)


def test_both_gates_build_from_the_machines_copy_of_the_sources() -> None:
    take_in = source.take_in_script()
    assert "tar -x" in take_in and "rsync -r --checksum --delete" in take_in
    for script in (gpu.fragments_script(["stress-255"]), counts.script(["stress-255"])):
        assert take_in in script
        assert f"cd {source.SOURCE}/rust/beryl3d" in script
        # The machine's working directory is the checkout's path, which it cannot see.
        assert take_in in script.split(f"cd {source.SOURCE}")[0]


def test_only_the_take_in_lines_read_the_stream(tmp_path: Path) -> None:
    """A command before the take-in lines that reads stdin gets nothing, and the files arrive
    although more bytes than a pipe holds follow the archive."""
    sent = tmp_path / "sent"
    (sent / "rust").mkdir(parents=True)
    (sent / "rust" / "lib.rs").write_text("fn main() {}\n")
    archive = subprocess.run(
        ["tar", "-c", "-f", "-", "-C", str(sent), "rust"],
        check=True,
        capture_output=True,
    ).stdout
    # More than a pipe holds after the archive, as padding a reader may leave unread.
    trailing = b"\0" * (1 << 17)
    script = (
        'eaten=$(cat | wc -c | tr -d " ")\n'
        + source.take_in_script()
        + 'echo "eaten $eaten"\n'
    )
    home = tmp_path / "home"
    home.mkdir()
    done = subprocess.run(
        ["bash", "-c", source.stream_script(script)],
        input=archive + trailing,
        env={**os.environ, "HOME": str(home)},
        capture_output=True,
        check=True,
        timeout=60,
    )
    assert done.stdout.decode().strip() == "eaten 0"
    assert (home / "b3d-source" / "rust" / "lib.rs").read_text() == "fn main() {}\n"


def test_a_file_list_longer_than_a_pipe_holds_is_sent(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    """tar once read its file list from a pipe this process wrote before the machine's shell
    started. A list over a pipe's buffer then left both blocked for ever, and the sweep's GPU
    gate was killed at its hang limit. The shell here is a fake `limactl` that reads the whole
    stream and counts its bytes."""
    files = tmp_path / "files"
    files.mkdir()
    names = []
    for i in range(1500):
        path = files / f"{i:04d}-{'x' * 60}.rs"
        path.write_text("fn main() {}\n")
        names.append(str(path))
    assert len("\0".join(names)) > 1 << 16
    bin_dir = tmp_path / "bin"
    bin_dir.mkdir()
    limactl = bin_dir / "limactl"
    limactl.write_text('#!/bin/sh\necho "read $(wc -c | tr -d " ")"\n')
    limactl.chmod(0o755)
    monkeypatch.setenv("PATH", f"{bin_dir}{os.pathsep}{os.environ['PATH']}")
    monkeypatch.setattr(source, "sources", lambda: names)

    result: list[subprocess.CompletedProcess[str] | None] = []
    sender = threading.Thread(
        target=lambda: result.append(source.run_with_sources("true")), daemon=True
    )
    sender.start()
    # Only catches a hang: the send takes well under a second.
    sender.join(timeout=120)
    assert result, "sending the sources hung"
    done = result[0]
    assert done is not None and done.returncode == 0
    assert int(done.stdout.split()[-1]) > 1500 * 512
