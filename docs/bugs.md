# Bug history

Rules engine bugs identified and fixed, kept to understand how to mitigate
similar classes of bugs in future (e.g. via the type system). AI, UI and tooling
bugs do not belong here. Each entry gives a short description of the bug, how a
similar bug could be prevented, and the fixing commit when it was not found and
fixed in the same session. Newest first.

- Winds of Abandon counted each exiled creature for its controller but, when a
  replacement kept one from exile, took the correction from its owner's count,
  so a stolen creature could cost the wrong player a basic. Prevention: split
  the objects by the player that matters once, before they move, and derive
  every count from that one split rather than recomputing who it was.
