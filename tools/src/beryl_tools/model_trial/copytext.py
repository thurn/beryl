"""The text of a trial copy: the source's text, rewritten where it acts on the source by ID, then
the source's epic's body and design, then the rules that keep the copy's work off the real
ticket, the fleet and the newer commits. The copy never names the source or the trial.
"""

from __future__ import annotations

import re
from typing import Any

THIS = "<this ticket's ID>"

# Steps of the source's text that act on the source or on the player's machine, rewritten or
# removed for the copy.
ATTACH: re.Pattern[str] = re.compile(r"(ignite ticket attach\s+)(\S+)")
# A sentence (or list item) that runs `just bugs done`, which archives the player's local
# bug report.
BUGS_DONE: re.Pattern[str] = re.compile(r"[^.\n]*`?just bugs done[^.\n]*(\.|\n|$) ?")


def rewrite(text: str, source_id: str) -> str:
    """`text` with every step that acts on `source_id` pointed at the copy, and `just bugs done`
    removed. Any other mention of the source's ID becomes "this ticket"."""
    text = ATTACH.sub(
        lambda m: m.group(1)
        + (THIS if m.group(2).strip("`") == source_id else m.group(2)),
        text,
    )
    text = BUGS_DONE.sub("", text)
    return mention(source_id).sub("this ticket", text)


def mention(ticket_id: str) -> re.Pattern[str]:
    """`ticket_id` as a whole ID: not part of a longer ID or of a child's ID."""
    return re.compile(
        rf"(?<![A-Za-z0-9.]){re.escape(ticket_id)}(?![A-Za-z0-9]|\.[0-9])"
    )


def planted(text: str, plant: dict[str, str] | None) -> str:
    """`text` with a planted premise written in."""
    if plant is None:
        return text
    return text.replace(plant["find"], plant["replace"], 1)


def rules(start_commit: str, interface: bool) -> str:
    """The starting and ending rules, word for word as the trial's epic gives them, with the
    additions that keep the copy apart from the real ticket."""
    captures = (
        "\n- This ticket changes what the player sees. Before you close it, attach to this "
        f"ticket, with `ignite ticket attach {THIS}`, the captures its acceptance asks for, at "
        "the same moments. If the acceptance asks for none, attach a capture of the changed "
        "state before and after your change."
        if interface
        else ""
    )
    return f"""## How to start and end this ticket

This ticket's change is kept on a branch for later review and is not landed by you. Right
after `ignite start`, run `git reset --hard {start_commit}` in your tree, and never run
`ignite sync`: the work must start from that commit. Then work it as you would any ticket:
commit in your tree and get `ignite check` to pass. Then, instead of `ignite land`, run
`git branch trial/{THIS} HEAD` and
`ignite close -m "<the landing note you would have written>"`. After that
`git branch`, do not list other branches.

- Never run `git fetch`, and never read `origin/staging`, `origin/ignite/staging` or any commit
  that is not an ancestor of `{start_commit}` or one of your own.
- Never comment on, attach to, update or close any ticket but this one, and never file a
  ticket that duplicates this one.
- Never switch a check on or off or change a flaky record (`ignite flaky`, `ignite check on`,
  `ignite check off` and the like). Where a procedure ends by doing so, stop before that step
  and say in your closing note that you did.
- Never run `just bugs done`: it changes the player's local bug report.{captures}
"""


def body(
    source: dict[str, Any],
    epic: dict[str, Any] | None,
    start_commit: str,
    plant: dict[str, str] | None,
    interface: bool,
) -> str:
    """The copy's body. The acceptance goes in the copy's own acceptance field."""
    sid = str(source["id"])
    parts = [planted(str(source.get("body") or ""), plant).rstrip()]
    notes = str(source.get("notes") or "").strip()
    if notes:
        parts.append(f"## Notes\n\n{planted(notes, plant)}")
    if epic is not None:
        context = "\n\n".join(
            s
            for s in (
                str(epic.get("body") or "").strip(),
                str(epic.get("design") or "").strip(),
            )
            if s
        )
        if context:
            parts.append(f"## Context from the epic\n\n{context}")
    text = rewrite("\n\n".join(parts), sid)
    if epic is not None:
        text = mention(str(epic["id"])).sub("the epic", text)
    return f"{text}\n\n{rules(start_commit, interface)}"


def acceptance(source: dict[str, Any], plant: dict[str, str] | None) -> str:
    return rewrite(
        planted(str(source.get("acceptance") or ""), plant), str(source["id"])
    )
