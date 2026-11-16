"""`just fseventsd`: reading the daemon's footprint as `top` prints it."""

from beryl_tools import fseventsd


def test_sizes_read_as_top_prints_them() -> None:
    assert fseventsd.parse_size("72G") == 72 << 30
    assert fseventsd.parse_size("845M+") == 845 << 20
    assert fseventsd.parse_size("12K-") == 12 << 10
    assert fseventsd.parse_size("N/A") is None


def test_the_launchd_job_runs_the_installed_copy() -> None:
    job = fseventsd.plist(4.0)
    assert f"<string>{fseventsd.INSTALLED}</string>" in job
    assert "<string>check</string>" in job
