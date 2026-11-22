"""Exercise the native Cargo profiles across a separate workspace's path dependencies."""

import os
import shutil
import subprocess
from pathlib import Path

import pytest

from beryl_tools.beryl3d import cli, paths


def crate(root: Path, name: str, dependencies: dict[str, Path], source: str) -> Path:
    directory = root / "crates" / name
    (directory / "src").mkdir(parents=True)
    lines = ["[package]", f'name = "{name}"', 'version = "0.1.0"', "[dependencies]"]
    lines.extend(
        f'{key} = {{ path = "{value.as_posix()}" }}'
        for key, value in dependencies.items()
    )
    (directory / "Cargo.toml").write_text("\n".join(lines) + "\n")
    (directory / "src" / "lib.rs").write_text(source)
    return directory


@pytest.mark.parametrize("nextest", [False, True])
def test_cargo_test_selects_packages_and_either_name_filter(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, nextest: bool
) -> None:
    nextest_path = shutil.which("cargo-nextest") if nextest else None
    if nextest and nextest_path is None:
        pytest.skip("cargo-nextest is not installed")
    monkeypatch.setattr(cli.shutil, "which", lambda _name: nextest_path)
    monkeypatch.setattr(cli, "record", lambda: 0)
    monkeypatch.setattr(paths, "WORKSPACE", tmp_path)
    monkeypatch.setenv("CARGO_TARGET_DIR", str(tmp_path / "target"))
    (tmp_path / "Cargo.toml").write_text(
        '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\n'
    )
    selected = """
        #[test]
        fn pose_one() { std::fs::write("pose-ran", "").unwrap(); }
        #[test]
        fn turn_two() { std::fs::write("turn-ran", "").unwrap(); }
        #[test]
        fn other() { panic!("name filter leaked"); }
    """
    host = crate(tmp_path, "beryl_host", {}, selected)
    space = crate(tmp_path, "kit_space", {}, selected)
    crate(
        tmp_path,
        "ignored",
        {},
        '#[test] fn pose_three() { panic!("package filter leaked"); }\n',
    )

    assert (
        cli.main(["cargo-test", "-p", "beryl_host", "-p", "kit_space", "pose", "turn"])
        == 0
    )
    for directory in (host, space):
        assert (directory / "pose-ran").is_file()
        assert (directory / "turn-ran").is_file()


@pytest.mark.parametrize("profile", ["dev", "test"])
def test_native_profiles_relink_changed_path_dependencies_without_incremental_reuse(
    tmp_path: Path, profile: str
) -> None:
    # Use the real profiles, not a second declaration of their intended settings. This
    # catches Cargo's inheritance and named-package overrides as well as the wildcard.
    manifest = (paths.WORKSPACE / "Cargo.toml").read_text()
    profiles = manifest[manifest.index("[profile.dev]") :]
    main = tmp_path / "main"
    main.mkdir()
    (main / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/*"]\n')
    kernel = crate(main, "beryl_kernel", {}, "pub fn value() -> u32 { 1 }\n")
    rules = crate(
        main,
        "beryl_rules",
        {"beryl_kernel": kernel},
        "pub fn value() -> u32 { beryl_kernel::value() }\n",
    )
    scenario = crate(
        main,
        "beryl_scenario",
        {"beryl_rules": rules},
        "pub fn value() -> u32 { beryl_rules::value() }\n",
    )
    client = crate(
        main,
        "beryl_cli",
        {"beryl_scenario": scenario},
        "pub fn value() -> u32 { beryl_scenario::value() }\n",
    )
    store = crate(main, "beryl_card_store", {}, "pub fn value() -> u32 { 1 }\n")
    native: Path = main / "native"
    scene = crate(
        native,
        "app_client",
        {"beryl_card_store": store},
        "pub fn value() -> u32 { beryl_card_store::value() }\n",
    )
    inspect = crate(
        native, "beryl_inspect", {"beryl_cli": client, "app_client": scene}, ""
    )
    (inspect / "src" / "main.rs").write_text(
        'fn main() { println!("{}", beryl_cli::value() + app_client::value()); }\n'
    )
    (native / "Cargo.toml").write_text(
        '[workspace]\nresolver = "3"\nmembers = ["crates/*"]\n' + profiles
    )
    target: Path = tmp_path / "target"
    env: dict[str, str] = {
        key: value
        for key, value in os.environ.items()
        if key != "CARGO_INCREMENTAL" and not key.startswith("CARGO_PROFILE_")
    }

    def build_and_run() -> str:
        built = subprocess.run(
            [
                "cargo",
                "build",
                "--offline",
                "--workspace",
                "--profile",
                profile,
                "--target-dir",
                str(target),
                # Unpacked debug info is a native Mac setting. The incremental policy
                # can be checked on Windows too, with its supported debug-info format.
                "--config",
                'profile.dev.split-debuginfo="off"',
            ],
            cwd=native,
            env=env,
            capture_output=True,
            text=True,
            check=False,
        )
        assert built.returncode == 0, built.stderr
        binary = (
            target
            / "debug"
            / ("beryl_inspect.exe" if os.name == "nt" else "beryl_inspect")
        )
        return subprocess.check_output([str(binary)], text=True).strip()

    assert build_and_run() == "2"
    incremental = target / "debug" / "incremental"
    caches = {path.name.rsplit("-", 1)[0] for path in incremental.iterdir()}
    # The main-workspace dependencies compile in full, except the crates between the card
    # crates and `record`, which every card edit recompiles.
    assert caches == {"app_client", "beryl_inspect", "beryl_cli"}
    # A second revision must rebuild and relink the main-workspace dependency chain,
    # while an edited native member must continue to use its incremental cache.
    (kernel / "src" / "lib.rs").write_text("pub fn value() -> u32 { 20 }\n")
    (rules / "src" / "lib.rs").write_text(
        "pub fn value() -> u32 { beryl_kernel::value() + 3 }\n"
    )
    (scene / "src" / "lib.rs").write_text(
        "pub fn value() -> u32 { beryl_card_store::value() + 4 }\n"
    )
    assert build_and_run() == "28"
    assert {path.name.rsplit("-", 1)[0] for path in incremental.iterdir()} == caches
