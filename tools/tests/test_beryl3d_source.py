"""Sending sources into the `beryl3d-bench` machine, shared by the count and GPU gates."""

from beryl_tools.beryl3d import counts, gpu, source


def test_sources_lists_the_files_the_builds_read() -> None:
    files = source.sources()
    assert "rust/beryl3d/Cargo.toml" in files
    assert all(f.split("/")[0] in ("rust", "data", "docs", "ui") for f in files)
    assert not any("/target/" in f or f.endswith(".DS_Store") for f in files)


def test_both_gates_build_from_the_machines_copy_of_the_sources() -> None:
    take_in = source.take_in_script()
    assert "tar -x" in take_in and "rsync -r --checksum --delete" in take_in
    for script in (gpu.fragments_script(["stress-255"]), counts.script([], "0.1")):
        assert take_in in script
        assert f"cd {source.SOURCE}/rust/beryl3d" in script
        # The machine's working directory is the checkout's path, which it cannot see.
        assert take_in in script.split(f"cd {source.SOURCE}")[0]
