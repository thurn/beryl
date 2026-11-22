"""Every Rust workspace package, built alone, gives each dependency the whole workspace's
features (`beryl_tools.workspace_hack` says why). When a package differs, Ignite builds the
whole workspace's test binaries for a change that only needs that package's.
"""

import sys

import pytest

from beryl_tools import workspace_hack


def test_a_feature_moving_from_the_host_to_the_target_changes_the_resolution() -> None:
    tree = """root v1.0.0|
|-- dep v1.0.0|{target}
|-- derive v1.0.0 (proc-macro)|
|   `-- dep v1.0.0|{host}
[build-dependencies]
`-- dep v1.0.0|{host} (*)
"""
    whole = workspace_hack.parse_feature_tree(
        tree.format(host="host-feature", target="target-feature")
    )
    swapped = tree.format(host="target-feature", target="host-feature")
    assert workspace_hack.compare(
        workspace_hack.parse_feature_tree(swapped), whole
    ) == {
        ("host", "dep v1.0.0"): ("target-feature", "host-feature"),
        ("target", "dep v1.0.0"): ("host-feature", "target-feature"),
    }


@pytest.mark.parametrize("native, other", [("macos", "windows"), ("windows", "macos")])
def test_regenerating_native_tables_preserves_the_other_platform(
    native: str, other: str
) -> None:
    other_tables = workspace_hack.dependency_tables(
        other, {("shared", "1.2.3"): {"other"}}
    )
    previous = workspace_hack.manifest(
        workspace_hack.HEADER, other, {("shared", "1.2.3"): {"other"}}
    )
    previous = workspace_hack.manifest(previous, native, {("old", "1.0.0"): {"native"}})
    updated = workspace_hack.manifest(previous, native, {("new", "2.0.0"): {"native"}})
    assert other_tables in updated
    assert (
        workspace_hack.dependency_tables(native, {("new", "2.0.0"): {"native"}})
        in updated
    )
    assert "old =" not in updated


def test_every_package_alone_has_the_whole_workspaces_features() -> None:
    if sys.platform not in workspace_hack.NATIVE_TARGETS:
        pytest.skip("the hack crate lists macOS and Windows dependencies")
    found = workspace_hack.differences()
    assert found == {}, (
        "these builds resolve host or target features differently from the whole workspace:"
        " run `just workspace-hack`, which lists any that are workspace packages' own"
        " features\n" + "\n".join(workspace_hack.describe(found))
    )
