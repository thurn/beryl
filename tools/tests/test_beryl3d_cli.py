"""`just b3d`'s cargo commands."""

from beryl_tools.beryl3d import cli


def test_tests_run_the_workspace_or_the_named_crates() -> None:
    assert cli.test_command([], [], nextest=False) == ["test", "-q", "--workspace"]
    plain = cli.test_command(["a", "b"], ["pose"], nextest=False)
    assert plain == ["test", "-q", "-p", "a", "-p", "b", "pose"]
    fast = cli.test_command(["a"], [], nextest=True)
    assert fast[:2] == ["nextest", "run"] and "--no-tests=pass" in fast
    assert fast[-2:] == ["-p", "a"]
