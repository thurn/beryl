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
- `beads.py`: the snapshot reader used by `tickets.py`. In the main checkout,
  `enact backend` supplies the `tk` executable. One `tk export` supplies tickets
  and verdicts. Ingest resumes `tk events --since` from the local store's
  cursor. `ticket_ids.py` resolves native IDs and the aliases kept for imported
  tickets.
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
- `locks.py`: Enact's lock-wait log (`.git/enact/lock-waits.jsonl` in the main
  checkout, and `lock-waits.1.jsonl` it is rotated to), one line per acquisition
  of Enact's ownership lock, into `lock_waits`. Each line is joined to the
  top-level tool call that ran its command (one that names its bead first), and
  `timing.py` moves the call's waited and sync seconds into the kinds
  `enact lock wait` and `enact sync`. `locks.report` is the `enact` query: those
  agent minutes, the log's lines by subcommand and the commands seen holding the
  lock; the `enact-lock-wait` finding ranks waits by holder. Lines from commands
  other than `enact.py` are not read. The ticket-server merge queue writes
  `staging-lease-waits.jsonl` in the same directory. Its waits join the push for
  the ticket recorded in the line and count as `enact lease wait`.
- `cilog.py`: CI's sources. The run log is added up as it is read into one
  `checks` row per check per local day per host and script and one `lane_runs`
  row per run of a check lane (a `LineSource` whose `merges` add each row to the
  stored one); no line is kept. The verdicts on the `ci-check` tickets come from
  `tk export` into `verdicts`. `family` names a check's family. `ci.py` is the
  `ci` query (the tree script, check lane, family, check, with CPU by kind of
  process from the sampler) and the CI metrics. `pushes.py` is the `pushes`
  query: each `enact push` tool call split by the fast check's `lane_runs` in
  the bead's worktree inside the call, with repeats after losing the race to
  `staging`, the steps the push's output says were left to slow CI
  (`tool_calls.deferred`), and the fast check's failures by verdict; the `bead`
  query shows the same split for its pushes. A push run in the background ends
  at its `notifications` row (or `prompts` row), when the session was told it
  finished. `failures.py` is the `failures` query: per Steward bead
  (`enact_ci_range`) its sessions' cost, bisection's CPU and the minutes
  `master` stayed behind.
- `commits.py`: the commits source, `git log` of `staging` and `master` run in
  the main checkout, with `master`'s reflog in `ref_moves` (kept after git
  expires it), which says when each commit reached `master`. `History` gives
  each commit's bead (from the beads' `enact_commits`, at query time), the bead
  that first added each card file, commit ranges and lead times. `rework.py`
  applies the two rework rules of "Rework" and is cached by `Facts.rework`;
  rework is a share of another bead's direct dollars, so it is shown beside
  direct cost and never summed into it. `cards.py` is the `cards` query and the
  card, rework and lead-time metrics.
- `changes.py`: the efficiency changes in `data/efficiency-changes.json`, each
  entry's commit (its `commit`, else the commit that added it, from the file's
  history on `staging`), the `changes` query, and the marks that `metrics`
  series carry: each metric's median over the seven days before and after the
  commit reached `staging`. Metrics date a sample by when its unit began, a bead
  by its start, so that a bead open across a change counts as before it. Its
  test also validates the file.
- `findings.py` is the `findings` query: the rules of "Findings", ranked in one
  group per unit, with `--rule` for one rule's items. `finding.py` holds what
  every rule shares (`Rule`, `Item`, the scope helpers), and the rules
  themselves are in `dollar_rules.py`, `time_rules.py` and `cpu_rules.py`, one
  function each with its reference in its docstring. A rule returns its items
  over the range; `findings.py` scales them to a week.
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
- `exchange.py`: moving rows between machines.
  `just efficiency export --since <date> [--out FILE]` writes this host's rows
  from every table with a `host` column as one gzip-compressed JSON-lines file
  (a header line, then `{"table", "row"}` lines; `ingests` is left out).
  `just efficiency import <file>` adds them in one transaction: a row already
  present is left alone, so importing a file twice, or two overlapping exports,
  changes nothing, except that `checks` and `lane_runs`, whose rows are growing
  totals, take the newer row. Table and column names in a file are checked
  against the schema, and a file from this host or a newer schema is refused. An
  export holds commands and paths, so it is copied between your own machines
  (`scp`, a USB drive) and never committed or published. `coverage` lists each
  host's newest row's date and the CPU tables that hold nothing for it, because
  Windows has no sampler or lease log.
- Windows: `paths.py` finds the providers' directories from the home directory
  (`%USERPROFILE%\.claude\projects`, `%USERPROFILE%\.codex\sessions`),
  `paths.project_name` builds a provider's project folder from the checkout path
  (every non-alphanumeric character becomes `-`), and `paths.inside` compares
  working directories as text with either separator, so nothing assumes `/`.
- `cli.py`: the command and its queries.
- The page: `serve.py` is `just efficiency serve`, a standard-library server on
  127.0.0.1 that answers `GET` only. An address is a query
  (`/bead/<id>?since=7d` is `efficiency bead <id> --since 7d`, `/` is
  `summary`), `.json` appended gives its JSON and `section=1` the section alone.
  `page.py` renders a query's result as HTML from its JSON as it is (no figure
  is computed there), with one script that fetches a line's section the first
  time it opens. A category's colour comes from its place in the command kinds
  table, so it is the same in every bar and strip. `views.py` and `ci_views.py`
  add the views the header leads to (`cards`, `machine`, `enact`, `ci`,
  `changes`, `findings`) and the sections they open (`metrics`, `pushes`,
  `failures`, one finding's items) to `page.RENDERERS`, built from the same
  parts. A view's trend lines are the metrics its result names under `more`, as
  lines open from the start whose section the script fetches at once; a trend
  marks each efficiency change on its metric, and the summary's figures carry
  the marks of the metrics in `rollups.FIGURE_METRICS`. `time` and `dollars`
  answer `.json` only. The server answers each connection on its own thread,
  because a browser holds connections open that it sends nothing on. A request
  never waits for an ingest: the server answers from the store as it is and
  keeps each answer until the next ingest. While the page is open, a thread runs
  `efficiency ingest` as a separate background-priority process whenever the
  store is over five minutes old, then computes the recently asked answers
  again. `/status.json` feeds the header's "updated" or "updating" note, which
  offers a reload once new data is in. `just efficiency serve` alone runs at
  normal priority.
- `just efficiency shot <address> [--dark] [--width N]` renders an address to a
  PNG under `$BERYL_CACHE/efficiency/shots/` with `shot.mjs`, which uses the
  main checkout's `ui/node_modules` Playwright and takes one of the machine's
  shared browser slots. Appending `#open` opens every line with more detail, one
  level deep; every open line is waited for. It is for looking at the page while
  changing it, and is in no check.

## Running and checking

- `just efficiency` lists the queries, and every query takes `--json`. The
  queries that report figures take `--since` (`24h`, `7d`, `all` or a date;
  roll-ups default to `7d`, drill-downs to `all`) and the cohort filters
  `--where key=value` (the label `key:value`), `--epic`, `--role`, `--tier`,
  `--host` and `--provider`. A window counts when it starts in the range, a
  request or tool call when it happened in it, and a bead when it closed in it;
  a closed bead's per-bead figures count all its windows. A query ingests first
  when the store's last ingest is over a minute old, unless given `--no-ingest`
  or another ingest is already running, in which case it answers from the store
  as it is. The store is `$BERYL_CACHE/efficiency/store.sqlite`.
- The `efficiency` check lane (`tools/src/beryl_tools/efficiency.py`) runs
  black, pyre and pytest here. Only a change under `efficiency/` or to
  `data/efficiency-*.json` selects it, and such a change selects no other lane
  but `repo`. To run its steps by hand from this directory:
  `uv run --project ../tools python -m pytest`, and the same with
  `python -m black --check .` and `pyre --noninteractive check`.
