"""`just b3d`'s cargo commands and the text commands it hands to the `b3d` program."""

import json
import os
import plistlib
import subprocess
import sys
from collections.abc import Iterator
from pathlib import Path

import pytest

from beryl_tools import paths
from beryl_tools.beryl3d import app, cli


def test_release_replaces_the_macos_app_with_a_signed_bundle(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch, capsys: pytest.CaptureFixture[str]
) -> None:
    binary = tmp_path / "release" / "beryl3d"
    if sys.platform == "win32":
        binary = binary.with_suffix(".exe")
    binary.parent.mkdir()
    binary.write_bytes(b"release executable")
    binary.chmod(0o755)
    bundle: Path = binary.with_name("Beryl 3D.app")
    bundle.mkdir()
    (bundle / "old").touch()

    manifest = tmp_path / "crates" / "beryl_3d" / "Cargo.toml"
    manifest.parent.mkdir(parents=True)
    manifest.write_text('[package]\nversion = "0.2.3"\n')
    audio_root = tmp_path / "data" / "audio" / "assets"
    audio: Path = audio_root / "music" / "menu-0.m4a"
    audio.parent.mkdir(parents=True)
    audio.write_bytes(b"music")
    icon = manifest.parent / "macos" / "icon.icns"
    icon.parent.mkdir()
    icon.write_bytes(b"icon")

    def sign(
        command: list[str | Path], *, check: bool
    ) -> subprocess.CompletedProcess[str]:
        assert command[:-1] == ["/usr/bin/codesign", "--force", "--sign", "-"] and check
        assert (bundle / "old").exists()
        contents = Path(command[-1]) / "Contents"
        assert (contents / "Resources" / "audio" / "music" / audio.name).exists()
        (contents / "signed").touch()
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(app, "RELEASE_BINARY", binary)
    monkeypatch.setattr(app, "build_release", lambda: 0)
    monkeypatch.setattr(app.paths, "WORKSPACE", tmp_path)
    monkeypatch.setattr(app.paths, "build_lock", lambda: tmp_path / "build.lock")
    monkeypatch.setattr(app.audio, "OUTPUT", audio_root)
    monkeypatch.setattr(subprocess, "run", sign)

    assert cli.main(["release"]) == 0
    if sys.platform != "darwin":
        assert f"release: {binary}" in capsys.readouterr().out
        assert binary.read_bytes() == b"release executable"
        assert (bundle / "old").exists()
        return
    assert f"release: {bundle}" in capsys.readouterr().out
    contents = bundle / "Contents"
    info = plistlib.loads((contents / "Info.plist").read_bytes())
    assert info["CFBundlePackageType"] == "APPL"
    assert info["CFBundleIdentifier"] == "ca.thurn.beryl3d"
    assert info["CFBundleShortVersionString"] == "0.2.3"
    executable = contents / "MacOS" / info["CFBundleExecutable"]
    assert executable.read_bytes() == binary.read_bytes() and os.access(
        executable, os.X_OK
    )
    assert (contents / "Resources" / info["CFBundleIconFile"]).read_bytes() == b"icon"
    assert (
        contents / "Resources" / "audio" / "music" / audio.name
    ).read_bytes() == b"music"
    assert (contents / "signed").exists()
    assert not (bundle / "old").exists()


def test_release_build_records_the_local_filing_tools(
    tmp_path: Path, monkeypatch: pytest.MonkeyPatch
) -> None:
    tools = {
        "uv": str(tmp_path / "package-manager" / "uv"),
        "ignite": str(tmp_path / "local-tools" / "ignite"),
    }
    monkeypatch.setattr(app.bugs.shutil, "which", tools.get)
    monkeypatch.setattr(app.paths, "build_lock", lambda: tmp_path / "build.lock")
    envs: list[dict[str, str]] = []

    def cargo(arguments: list[str], env: dict[str, str] | None = None) -> int:
        assert "--release" in arguments
        assert env is not None
        envs.append(env)
        return 0

    monkeypatch.setattr(cli, "cargo", cargo)

    assert app.build_release() == 0
    (env,) = envs
    assert env["BERYL_BUNDLE_BUGS_REPO"] == str(paths.REPO)
    assert env["BERYL_BUNDLE_BUGS_UV"] == tools["uv"]
    assert str(Path(tools["ignite"]).parent) in env["BERYL_BUNDLE_BUGS_PATH"].split(
        os.pathsep
    )


def test_tests_run_the_workspace_or_the_named_crates() -> None:
    assert cli.test_command([], [], nextest=False) == ["test", "-q", "--workspace"]
    assert cli.test_command([], ["pose", "turn"], nextest=False) == [
        "test",
        "-q",
        "--workspace",
        "--",
        "pose",
        "turn",
    ]
    plain = cli.test_command(["a", "b"], ["pose", "turn"], nextest=False)
    assert plain == [
        "test",
        "-q",
        "--workspace",
        "--no-run",
        "--message-format=json-render-diagnostics",
    ]
    fast = cli.test_command(["a"], [], nextest=True)
    assert fast[:2] == ["nextest", "run"] and "--no-tests=pass" in fast
    assert fast[-3:] == ["--workspace", "-E", "(package(=a))"]
    assert cli.test_command([], [], nextest=True)[-1] == "--workspace"


def test_named_crates_under_nextest_build_what_the_shared_build_built() -> None:
    """Naming packages to cargo changes which features it resolves, so nextest builds the
    whole workspace, as `SHARED_BUILD` does, and filters the tests instead."""
    named = cli.test_command(["a", "b"], ["pose", "turn"], nextest=True)
    assert "-p" not in named and "--workspace" in named
    assert named[-2:] == [
        "-E",
        "(package(=a) | package(=b)) & (test(pose) | test(turn))",
    ]


def test_text_commands_hand_every_argument_to_the_b3d_program(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    envs: list[dict[str, str]] = []

    def cargo(arguments: list[str], env: dict[str, str] | None = None) -> int:
        ran.append(arguments)
        envs.append(env or {})
        return 0

    monkeypatch.setattr(cli, "cargo", cargo)

    def program(
        command: list[str], env: dict[str, str] | None = None, **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        envs.append(env or {})
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli.subprocess, "run", program)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    assert cli.main(["tree", "verbs-zones", "--at", "1500ms", "--help"]) == 0
    build, run = ran
    assert build == ["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"]
    assert run == [
        str(Path("/linked/b3d")),
        "tree",
        "verbs-zones",
        "--at",
        "1500ms",
        "--help",
    ]
    # The one-thread program is told to use one thread.
    _build_env, run_env = envs
    assert run_env[cli.CORES_ENV] == "1"


def test_map_includes_every_launcher_command_with_its_source(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    def program(
        command: list[str], env: dict[str, str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        assert command == [str(Path("/linked/b3d")), "map", "commands", "--json"]
        entries = json.loads(env["BERYL3D_LAUNCHER_COMMANDS"])
        assert {entry["name"] for entry in entries} == {
            command.name for command in cli.COMMANDS
        }
        for entry in entries:
            assert (paths.REPO / entry["path"]).is_file(), entry
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli, "quick_build", lambda _arguments: 0)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    monkeypatch.setattr(cli.subprocess, "run", program)
    assert cli.main(["map", "commands", "--json"]) == 0


def test_interface_tests_run_the_shared_builds_b3d(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    """`test` builds nothing beyond the shared build, which `cargo-test` and `record` also
    start with (`cli.run_shared_b3d`)."""
    ran: list[list[str]] = []
    monkeypatch.setattr(
        cli, "cargo", lambda arguments, env=None: ran.append(arguments) or 0
    )

    def program(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli.subprocess, "run", program)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    monkeypatch.setattr(cli, "program", lambda name: Path("/built") / name)
    assert cli.main(["test"]) == 0
    assert ran == [cli.SHARED_BUILD, [str(Path("/built/b3d")), "test"]]


def test_a_missing_pack_is_recorded_and_the_command_run_again(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    monkeypatch.setattr(
        cli, "cargo", lambda arguments, env=None: ran.append(arguments) or 0
    )
    codes: Iterator[int] = iter([cli.MISSING_PACK, 0, 0])

    def program(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        return subprocess.CompletedProcess(command, next(codes))

    monkeypatch.setattr(cli.subprocess, "run", program)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    monkeypatch.setattr(cli, "program", lambda name: Path("/built") / name)
    assert cli.main(["tree", "verbs-zones"]) == 0
    assert ran == [
        ["build", "-q", "-p", "beryl_inspect", "--bin", "b3d"],
        [str(Path("/linked/b3d")), "tree", "verbs-zones"],
        cli.SHARED_BUILD,
        [str(Path("/linked/record"))],
        [str(Path("/linked/b3d")), "tree", "verbs-zones"],
    ]


def test_the_linkers_newest_binary_is_run(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(cli.paths, "WORKSPACE", tmp_path)
    debug = tmp_path / "target" / "debug"
    deps = debug / "deps"
    deps.mkdir(parents=True)
    assert cli.linked("b3d") == cli.system.exe(debug / "b3d")
    old = cli.system.exe(deps / "b3d-0000000000000000")
    new = cli.system.exe(deps / "b3d-1111111111111111")
    for path in (old, new, deps / "b3d-1111111111111111.d"):
        path.write_text("")
    os.utime(old, (1, 1))
    os.utime(new, (2, 2))
    assert cli.linked("b3d") == new
    render = cli.system.exe(deps / "b3d_render-2222222222222222")
    render.write_text("")
    assert cli.linked("b3d-render") == render


def test_a_newer_test_harness_of_the_crate_is_not_run_as_the_program(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    monkeypatch.setattr(cli.paths, "WORKSPACE", tmp_path)
    debug = tmp_path / "target" / "debug"
    deps = debug / "deps"
    deps.mkdir(parents=True)
    program = cli.system.exe(deps / "b3d-0000000000000000")
    harness = cli.system.exe(deps / "b3d-1111111111111111")
    for path in (program, harness):
        path.write_text("")
    os.utime(program, (1, 1))
    os.utime(harness, (2, 2))
    for hash_, kind in (("0000000000000000", "bin"), ("1111111111111111", "test-bin")):
        (debug / ".fingerprint" / f"beryl_inspect-{hash_}").mkdir(parents=True)
        (debug / ".fingerprint" / f"beryl_inspect-{hash_}" / f"{kind}-b3d").write_text(
            ""
        )
    assert cli.linked("b3d") == program


def test_the_listing_names_every_text_command() -> None:
    listed = cli.listing()
    for name in ("list", "info", "tree", "diff", "check", "test", "png", "film"):
        assert f"\n  {name} " in listed


def test_the_listing_names_every_command_once_and_includes_the_phone_commands() -> None:
    names = [command.name for command in cli.COMMANDS]
    assert len(names) == len(set(names))
    listed = cli.listing()
    for name in names:
        assert f"\n  {name} " in listed
    for name in ("ios-smoke", "android-smoke", "phone-search", "ios-device"):
        assert name in names


def test_picture_commands_hand_every_argument_to_the_b3d_render_program(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    ran: list[list[str]] = []
    monkeypatch.setattr(
        cli, "cargo", lambda arguments, env=None: ran.append(arguments) or 0
    )

    def program(
        command: list[str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        ran.append(command)
        return subprocess.CompletedProcess(command, 0)

    monkeypatch.setattr(cli.subprocess, "run", program)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    assert cli.main(["png", "verbs-zones", "--at", "1500ms", "--tree"]) == 0
    build, run = ran
    assert build == ["build", "-q", "-p", "beryl_pictures", "--bin", "b3d-render"]
    assert run == [
        str(Path("/linked/b3d-render")),
        "png",
        "verbs-zones",
        "--at",
        "1500ms",
        "--tree",
    ]


def test_a_build_runs_under_the_checkouts_build_lock(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    held: list[bool] = []

    def cargo(arguments: list[str], env: dict[str, str] | None = None) -> int:
        assert env is None
        # Another build of this checkout would wait here, before it starts cargo.
        try:
            with cli.system.file_lock(cli.paths.build_lock(), timeout=0):
                held.append(False)
        except cli.system.LockHeld:
            held.append(True)
        return 0

    monkeypatch.setattr(cli, "cargo", cargo)
    assert cli.shared_build() == 0
    assert held == [True]


# A compiler controlled through stdin, so concurrent builds and cancellation are tested
# without sleeps or assumptions about how quickly either process gets scheduled.
BUILD_CHILD = """
import sys
from pathlib import Path
from beryl_tools.beryl3d import cli

root = Path(sys.argv[1])
cli.paths.build_lock = lambda: root / 'build.lock'

def cargo(arguments, env=None):
    assert env is None
    assert arguments == cli.SHARED_BUILD
    try:
        with cli.system.file_lock(cli.paths.build_lock(), timeout=0):
            raise AssertionError('cargo started outside the build lock')
    except cli.system.LockHeld:
        pass
    print('locked', flush=True)
    assert sys.stdin.read(1) == 'x'
    built = root / 'built'
    if built.exists():
        print('reused', flush=True)
        return 0
    print('compiling', flush=True)
    built.write_text('ready')
    return 0

cli.cargo = cargo
print('started', flush=True)
raise SystemExit(cli.shared_build())
"""


@pytest.mark.parametrize("cancel", [False, True])
def test_parallel_builds_reuse_a_build_and_a_canceled_holder_releases_it(
    tmp_path: Path, cancel: bool
) -> None:
    children: list[subprocess.Popen[str]] = []
    try:
        for _ in range(3):
            child = subprocess.Popen(
                [sys.executable, "-c", BUILD_CHILD, str(tmp_path)],
                stdin=subprocess.PIPE,
                stdout=subprocess.PIPE,
                text=True,
            )
            children.append(child)
            output_pipe = child.stdout
            assert output_pipe is not None
            assert output_pipe.readline().strip() == "started"
            if len(children) == 1:
                assert output_pipe.readline().strip() == "locked"

        first, *followers = children
        if cancel:
            first.kill()
            first.wait()
            assert not (tmp_path / "built").exists()
        else:
            input_pipe = first.stdin
            assert input_pipe is not None
            input_pipe.write("x")
            input_pipe.flush()
            assert first.wait() == 0

        # Either follower may acquire the released lock first.
        for child in followers:
            input_pipe = child.stdin
            assert input_pipe is not None
            input_pipe.write("x")
            input_pipe.flush()
        results = []
        for child in followers:
            output, _ = child.communicate()
            assert child.returncode == 0
            locked, result = output.splitlines()
            assert locked == "locked"
            results.append(result)
        assert sorted(results) == (
            ["compiling", "reused"] if cancel else ["reused"] * 2
        )
        assert (tmp_path / "built").read_text() == "ready"
    finally:
        for child in children:
            if child.poll() is None:
                child.kill()
            child.wait()
            for pipe in (child.stdin, child.stdout):
                if pipe is not None:
                    pipe.close()


def test_a_named_spec_uses_a_cached_local_pack(
    monkeypatch: pytest.MonkeyPatch, tmp_path: Path
) -> None:
    selected: Path = tmp_path / "recordings-spec.bin"
    launched: list[list[str]] = []

    def program(
        command: list[str], env: dict[str, str], **_kwargs: object
    ) -> subprocess.CompletedProcess[str]:
        launched.append(command)
        assert env[cli.PACK_ENV] == str(selected)
        return subprocess.CompletedProcess(command, 0)

    recorded: list[tuple[str, ...]] = []

    def record(_binary: Path, *selection: str) -> Path:
        recorded.append(selection)
        return selected

    monkeypatch.delenv(cli.PACK_ENV, raising=False)
    monkeypatch.setattr(cli, "shared_build", lambda: 0)
    monkeypatch.setattr(cli, "linked", lambda name: Path("/linked") / name)
    monkeypatch.setattr(cli.pack, "record", record)
    monkeypatch.setattr(cli.paths, "DATA", tmp_path)
    (tmp_path / "corpus.txt").write_text("counterspell/other-moment\n")
    monkeypatch.setattr(cli.subprocess, "run", program)
    name = "counterspell/cast"
    args = ["tree", "--seat", "opponent", name]
    assert cli.run_program("b3d", args) == 0
    assert launched == [[str(Path("/linked/b3d")), *args]]
    assert recorded == [("--prefix", f"{name}/")]
    assert cli.pack_selection(["test", "b3d:counterspell/"]) == (
        "--prefix",
        "b3d:counterspell/",
    )
