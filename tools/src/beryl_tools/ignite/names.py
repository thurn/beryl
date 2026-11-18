"""Writing a test's own name as an Ignite check name and back.

Ignite's check names may hold no whitespace or control character, but test names often do:
pytest's parametrized ids (`test_x[Grizzly Bears-complete]`). `check_part` writes each
whitespace or control character, and `%`, as `%XX` (UTF-8, as in a URL), and leaves every
other character alone, so a name that had none of them is unchanged and keeps its history.
`decode_part` gives the exact name back.
"""

from __future__ import annotations

import urllib.parse


def check_part(name: str) -> str:
    """`name` with whitespace, control characters and `%` written as `%XX`."""
    out: list[str] = []
    for char in name:
        if char == "%" or char.isspace() or not char.isprintable():
            out.append("".join(f"%{byte:02X}" for byte in char.encode()))
        else:
            out.append(char)
    return "".join(out)


def decode_part(part: str) -> str:
    """The name that `check_part` made `part` from."""
    return urllib.parse.unquote(part)
