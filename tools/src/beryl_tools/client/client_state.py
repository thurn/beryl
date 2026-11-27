"""Keep transient feature state in the the client feature registry."""

from __future__ import annotations

import bisect
from pathlib import Path

from .purity import line_starts, rust_tokens

CLIENT_SOURCE = "client/crates/app_client/src/client/mod.rs"

# The client owns frame and session coordination, host input, saved preferences and
# the feature registry itself. Review new shared state here. A screen, overlay or HUD
# piece keeps its transient state in its own registered Feature::State.
CORE_FIELDS = """
frame: Frame
probe: Option<game_probe::explain::Probe>
stories: story::Stories
window: Window
show: Show
layer: Layer
features: Box<features::Features>
routes: input::Routes
settings: crate::settings::Settings
session: Option<SessionId>
step_timings: OwnTimings
views: usize
platform: Platform
now: u64
wake: Wake
requests: VecDeque<Request>
commands: VecDeque<SessionCommand>
tier: Tier
low_memory: bool
"""


def check(root: Path) -> list[str]:
    """Check each direct Client field against the shared state declarations."""
    path = root / CLIENT_SOURCE
    if not path.is_file():
        return [f"{CLIENT_SOURCE}: could not find the Client source"]
    source = path.read_text()
    tokens = rust_tokens(source)
    names = [token.text for token in tokens]
    start = next(
        (
            index + 3
            for index in range(len(names))
            if names[index : index + 3] == ["struct", "Client", "{"]
        ),
        None,
    )
    if start is None:
        return [f"{CLIENT_SOURCE}: could not find the Client struct"]

    allowed: dict[str, list[str]] = {}
    for field in CORE_FIELDS.strip().splitlines():
        declaration = [token.text for token in rust_tokens(field)]
        allowed[declaration[0]] = declaration
    while start < len(names) and names[start] != "}":
        name = names[start]
        declaration = allowed.get(name, [])
        end = start + len(declaration)
        if (
            declaration
            and names[start:end] == declaration
            and end < len(names)
            and names[end] in {",", "}"}
        ):
            start = end + (names[end] == ",")
            continue
        line = bisect.bisect_right(line_starts(source), tokens[start].offset)
        return [
            f"{CLIENT_SOURCE}:{line}: Client field starting with `{name}` is outside "
            "the shared state declarations. Keep transient state in a Feature registered "
            "in client/features.rs."
        ]
    return []
