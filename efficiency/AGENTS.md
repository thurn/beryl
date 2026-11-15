# The efficiency dashboard

This directory is a standalone Python project, `beryl_efficiency`, that shows
where agent sessions' time, the machine's CPU and LLM tokens go, per bead. The
design is `docs/efficiency-dashboard.md`; read its "Design overview", "The
store" and "Isolation" before changing anything here.

## Rules

- Standard library only. The dev tools (black, pyre, pytest) are the ones
  `tools/pyproject.toml` pins.
- Nothing here imports from `tools/`, and nothing outside imports this package.
  Other tools' files are read by their documented formats. Where a rule must
  match the tools (such as where the cache is), copy it and say where from.
- Pyre is strict and code is formatted by black.
- The repository is public. Tests use small synthetic sources built in the test
  or kept in `tests/synthetic.py`. Never commit a real transcript, session
  record, bead dump or command line from a machine.
- Collection must cost agent sessions nothing: no hook, wrapper or prompt text
  in any session, and ingest never runs inside another command.

## Layout

- `src/beryl_efficiency/paths.py`: the cache (`$BERYL_CACHE`, else the
  platform's default), the store, the ingest lock and the host name.
- `store.py`: the SQLite schema. It grows only by appending to `MIGRATIONS`,
  because the store is the long-term record and a rebuild loses transcripts the
  providers have deleted.
- `ingest.py`: the ingest framework. A `LineSource` is files of lines read from
  their recorded offsets, a batch at a time, with a `finish` step for rows that
  depend on more than one line; a `FileSource` is small files read whole when
  their size or modification time changed; a `SnapshotSource` is a command's
  output, written only when its digest changed. Malformed lines are skipped and
  counted. Every ingest records its own wall and CPU seconds in the `ingests`
  table.
- `beads.py`: the beads source, from `bd list --all -n 0 --json` run in the main
  checkout.
- `claude.py`: Claude Code transcripts (requests, tool calls, their results and
  the Enact commands a session ran) and the desktop app's session records.
  `windows.py` derives bead windows from those commands and sets the `bead` of
  each request and tool call.
- `codex.py`: Codex logs (`~/.codex/sessions` and `archived_sessions`, `.jsonl`
  only; the `.zst` ones are not read), as rows in the same tables with provider
  `codex`. A request is a `token_count` line, keyed by its running total; its
  model and turn end come from `codex_marks`. Shell commands are read from the
  string literals in the `exec` script's `cmd:`, so a command built from
  variables is missed.
- `kinds.py`: command kinds from `data/efficiency-command-kinds.json`.
  `timing.py` splits each window's seconds into the categories of "Time
  accounting" and stores them in `window_time`. Kinds and splits are derived
  again for every transcript when the kinds table changes.
- `tokens.py`: dollars from `data/efficiency-prices.json`, the parts of each
  request's context, tool calls' added tokens and carrying cost, and each
  window's dollars by class, source, purpose and model in `window_dollars`. All
  of it is derived again when the price table changes.
- `cpu.py`: the CPU sampler's rows (`cpu`, `cpu_minutes`) and the CPU token
  pool's log of leases (`leases`), each lease joined to the top-level tool call
  running in its worktree while it waited, which `timing.py` counts as queued
  for cores. It also gives each minute of a worktree's CPU to a bead.
  `facts.Facts.core_minutes` uses it per bead, and `machine.py` is the `machine`
  query and the CPU metrics.
- `cilog.py`: CI's sources. The run log is added up as it is read into one
  `checks` row per check per local day per host and script and one `lane_runs`
  row per run of a check lane (a `LineSource` whose `merges` add each row to the
  stored one); no line is kept. The verdicts on the `ci-check` beads come from
  `bd export` into `verdicts`. `family` names a check's family. `ci.py` is the
  `ci` query (the tree script, check lane, family, check, with CPU by kind of
  process from the sampler) and the CI metrics.
- The query layer: `scope.py` parses `--since` and the cohort filters into a
  `Scope`; `facts.py` loads beads and windows with their `window_time` and
  `window_dollars` rows, which every query adds up, so that a roll-up's lines
  (epics, "(no bead)", "orchestration") sum to its total. `rollups.py` holds
  `summary`, `epic`, `time --by` and `dollars --by`; `bead.py`, `session.py` and
  `metrics.py` (the registry of named series, which later queries extend with
  `register`) hold the others.
- A query returns a typed dataclass whose numbers are already rounded; its
  renderer only formats them, and `--json` prints the same dataclass, so the
  text and the JSON cannot disagree. `render.py` has the `Result`, the field
  descriptions the page shows on hover (`FIELDS`), and the text tables. A sum
  that a model with no price makes a lower bound is marked `+`; an unknown value
  prints as `-`.
- `cli.py`: the command and its queries.

## Running and checking

- `just efficiency` lists the queries, and every query takes `--json`. The
  queries that report figures take `--since` (`24h`, `7d`, `all` or a date;
  roll-ups default to `7d`, drill-downs to `all`) and the cohort filters
  `--where key=value` (the label `key:value`), `--epic`, `--role`, `--tier`,
  `--host` and `--provider`. A window counts when it starts in the range, a
  request or tool call when it happened in it, and a bead when it closed in it;
  a closed bead's per-bead figures count all its windows. A query ingests first
  when the store's last ingest is over a minute old, unless given `--no-ingest`.
  The store is `$BERYL_CACHE/efficiency/store.sqlite`.
- The `efficiency` check lane (`tools/src/beryl_tools/efficiency.py`) runs
  black, pyre and pytest here. Only a change under `efficiency/` or to
  `data/efficiency-*.json` selects it, and such a change selects no other lane
  but `repo`. To run its steps by hand from this directory:
  `uv run --project ../tools python -m pytest`, and the same with
  `python -m black --check .` and `pyre --noninteractive check`.
