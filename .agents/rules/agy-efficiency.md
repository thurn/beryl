---
trigger: always_on
---
# Antigravity Token-Efficiency Rules

Antigravity auto-compacts only near 512k tokens and its prompt cache evicts
after roughly 5 minutes of inactivity. Every large read or cache miss multiplies
cost across the rest of the session, so keep context small and cache warm.

## Narrow reads and trimmed output

- Search before reading: locate the exact file and line numbers with `rg -n` via
  `run_command` before calling `read_file`.
- Always pass `StartLine` and `EndLine` to `read_file`, keeping each slice to at
  most 150 lines unless the whole file is already 150 lines or shorter. Do not
  call `read_file` without line bounds on long source files.
- When batching parallel reads as `AGENTS.md` asks, batch independent `rg`
  searches and narrow line-range reads — never speculative batches of whole-file
  `read_file` calls.
- Never re-read a file or line range that is already in your conversation
  history and has not changed.
- Each image opened with `read_file` adds roughly 2,500 carried tokens. Prefer
  one contact sheet (`just client film`) or 1–3 targeted PNGs instead of opening
  many individual frames.
- Trim verbose command output (`| head -n 80`, `--quiet`, `--status-level=fail`,
  or filtering to failures) instead of letting thousands of lines enter context.

## Offload broad surveys to `research`

- When you need to survey more than ~5 unfamiliar files, trace behavior across
  several crates, or scan long docs just to find which few files to edit, call
  `invoke_subagent` with `TypeName="research"` and `Model="flash"`.
- Ask the subagent for exact file paths, line ranges, symbol names, and a brief
  summary, then read only those narrow ranges in your own turn.
- Keep small, targeted lookups (1–3 known files) in your own turn.

## Keep the prompt cache warm during waits over 4 minutes

- On any `run_command` that may run longer than 4 minutes (such as `ignite
  test`, `ignite check`, `ignite land`, `ignite run`, or `ignite wait`), pass
  `NotificationTimeoutSeconds: 240`.
- When that 240-second notification arrives and the command is still running, do
  not poll its logs or restart it. Once you are past the first few turns of a
  session, set one `schedule` timer with `DurationSeconds: 240` and
  `TimerCondition` set to the background task's ID (so it cancels automatically
  when the command finishes), then stop calling tools and wait. Repeat once per
  240 seconds only while the command is still running.
- Never use `sleep` loops or background shell monitors.
