"""Every Rust workspace package, built alone, gives each dependency the whole workspace's
features (`beryl_tools.workspace_hack` says why). When a package differs, Ignite builds the
whole workspace's test binaries for a change that only needs that package's.
"""

import sys

import pytest

from beryl_tools import workspace_hack

# beryl_workspace_hack lists macOS's resolution of the dependencies.
pytestmark: pytest.MarkDecorator = pytest.mark.skipif(
    sys.platform != "darwin", reason="the hack crate lists macOS's dependencies"
)


def test_every_package_alone_has_the_whole_workspaces_features() -> None:
    found = workspace_hack.differences()
    assert found == {}, (
        "these packages, built alone, resolve features differently from the whole workspace:"
        " run `just workspace-hack`, which lists any that are workspace packages' own"
        " features\n" + "\n".join(workspace_hack.describe(found))
    )
