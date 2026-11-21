# The efficiency dashboard

This directory is a standalone Python project, `beryl_efficiency`, that shows
where agent sessions' time, the machine's CPU and LLM tokens go, per ticket. The
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
  or kept in `tests/synthetic.py`. `tests/legacy_sources.py` holds stand-ins for
  the readers that Ignite's hub replaced (a ticket snapshot, the run log, the
  CPU sampler's rows, the lease log and the verdict comments); tests use them to
  build stores in the shape of the stored history. Never commit a real
  transcript, session record, ticket dump or command line from a machine.
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
- `beads.py`: `rows_of`, which builds the `beads` and `labels` rows of one
  ticket record for `ignite.HubTickets`, and `main_checkout`, which `commits.py`
  uses. `ticket_ids.py` resolves native IDs and the aliases kept for imported
  tickets.
- `claude.py`: Claude Code transcripts (requests, tool calls, their results and
  the `ignite start <ticket>` commands a session ran, and the `enact` commands
  in old transcripts) and the desktop app's session records. `windows.py`
  derives ticket windows from those commands and sets the `bead` of each request
  and tool call.
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
- `cpu.py`: joins each lease to the top-level tool call running in its tree
  while it waited, which `timing.py` counts as queued for cores. It also gives
  each minute of a tree's CPU to a ticket. `facts.Facts.core_minutes` uses it
  per ticket, and `machine.py` is the `machine` query and the CPU metrics. The
  `cpu`, `cpu_minutes` and `leases` rows come from Ignite's records
  (`ignite.py`); rows the CPU sampler and the token pool's lease log wrote
  before the switch stay in the store.
- `cilog.py`: what `checks` and `lane_runs` rows share: `family` names a check's
  family, and the merge rules that add each row to the stored one. Ignite's
  records fill those tables; the run log and the `verdicts` that `tk export`
  supplied are history in the store. `ci.py` is the `ci` query (the tree script,
  check lane, family, check, with CPU by kind of process) and the CI metrics.
  `pushes.py` is the `pushes` query: each `ignite land` (and, in old
  transcripts, `enact push`) tool call split by the gate's `lane_runs` in the
  ticket's tree inside the call, with repeats after losing the race to
  `staging`, the steps the push's output says were left to slow CI
  (`tool_calls.deferred`), and the gate's failures by verdict; the `bead` query
  shows the same split for its pushes. A push run in the background ends at its
  `notifications` row (or `prompts` row), when the session was told it finished.
  `failures.py` is the `failures` query: per Steward ticket (`enact_ci_range`)
  its sessions' cost, bisection's CPU and the minutes `master` stayed behind.
- `ignite.py`: Ignite's hub as a source, read only on the machine whose cache
  holds the hub's database (`$IGNITE_HUB_DB`, else `hub/hub.db` in the Ignite
  home, `$IGNITE_HOME` or `~/.config/ignite`). Its records, one per command on
  any machine, replaced the run log, the token pool's leases and the CPU
  sampler: each becomes an `ignite_records` row, a `leases` row, `cpu` and
  `cpu_minutes` rows over the minutes it ran and, for a check family, `checks`
  and `lane_runs` rows (the gate as script `check`, tip verification as `ci`).
  The efficiency rows other machines' daemons posted (`metrics`, one export line
  per row) are imported as `import` imports a file. `PlanSamples` copies the
  Claude plan's usage samples (metrics source `plan_usage`) into `plan_usage`,
  which is never exported. Tickets, tips, hand-offs and workers are copied into
  the `ignite_*` tables. `HubTickets` also writes tickets as `beads`, `labels`
  and `ticket_aliases` rows in place of `tk export`. The hub supplies no verdict
  comments or session events, so those tables stop growing. The `ignite` source
  comes first in `cli.sources()`, ahead of the transcripts, because windows read
  the tickets; on a machine with no hub database none of these sources exist and
  ingest keeps what the store has. A machine is named by the first host seen for
  its ID (`ignite_machines`), so a rename does not split it, and hub ticket IDs
  are resolved with `ticket_ids.stored`. `ignite_metrics.py` registers the
  `ignite.*` series: cycle time, waiting for a worker, repair share, `master`
  lag, dispatch dollars, escapes per 100 landings, cache hits and the gate's
  instructions, seconds and wait. Enact's lock-wait log has no Ignite
  counterpart: `lock_waits` rows are history that `timing.py` still reads, and
  no query shows them.
- `commits.py`: the commits source, `git log` of `staging` and `master` run in
  the main checkout, with `master`'s reflog in `ref_moves` (kept after git
  expires it), which says when each commit reached `master`. `History` gives
  each commit's ticket (from the tickets' `enact_commits`, at query time), the
  ticket that first added each card file, commit ranges and lead times.
  `rework.py` applies the two rework rules of "Rework" and is cached by
  `Facts.rework`; rework is a share of another ticket's direct dollars, so it is
  shown beside direct cost and never summed into it. `cards.py` is the `cards`
  query and the card, rework and lead-time metrics.
- `changes.py`: the efficiency changes in `data/efficiency-changes.json`, each
  entry's commit (its `commit`, else the commit that added it, from the file's
  history on `staging`), the `changes` query, and the marks that `metrics`
  series carry: each metric's median over the seven days before and after the
  commit reached `staging`. Metrics date a sample by when its unit began, a
  ticket by its start, so that a ticket open across a change counts as before
  it. Its test also validates the file.
- `findings.py` is the `findings` query: the rules of "Findings", ranked in one
  group per unit, with `--rule` for one rule's items. `finding.py` holds what
  every rule shares (`Rule`, `Item`, the scope helpers), and the rules
  themselves are in `dollar_rules.py`, `time_rules.py` and `cpu_rules.py`, one
  function each with its reference in its docstring. A rule returns its items
  over the range; `findings.py` scales them to a week.
- The query layer: `scope.py` parses `--since` and the cohort filters into a
  `Scope`; `facts.py` loads tickets and windows with their `window_time` and
  `window_dollars` rows, which every query adds up, so that a roll-up's lines
  (epics, "(no ticket)", "orchestration") sum to its total. `rollups.py` holds
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
  from every table with a `host` column as one JSON-lines file, gzip-compressed
  unless its name ends in `.jsonl` (a header line, then `{"table", "row"}`
  lines; `ingests` is left out). Ignite's daemons post the plain form to the
  hub. `just efficiency import <file>` adds them in one transaction: a row
  already present is left alone, so importing a file twice, or two overlapping
  exports, changes nothing, except that `checks` and `lane_runs`, whose rows are
  growing totals, take the newer row. Table and column names in a file are
  checked against the schema, and a file from this host or a newer schema is
  refused. An export holds commands and paths, so it is copied between your own
  machines (`scp`, a USB drive) and never committed or published. `coverage`
  lists each host's newest row's date and the CPU tables that hold nothing for
  it, because a host may hold no Ignite records.
- Windows: `paths.py` finds the providers' directories from the home directory
  (`%USERPROFILE%\.claude\projects`, `%USERPROFILE%\.codex\sessions`),
  `paths.project_name` builds a provider's project folder from the checkout path
  (every non-alphanumeric character becomes `-`), and `paths.inside` compares
  working directories as text with either separator, so nothing assumes `/`.
- `spend.py` is the `spend` command: each ticket window of this host as JSON
  lines of time and dollar segments in order, coalesced to at most 120 per list,
  each with a colour group, for the efficiency section of Ignite's ticket pages.
  Ignite's daemon runs it after each export and posts its lines to the hub. A
  Claude window also carries `plan_percent`, its estimated share of the plan's
  weekly limit, and the same for every window of the plan; the windows of each
  plan window's latest period are sent again at every run, because its factor
  changes until the period ends.
- `plan.py`: each ticket's estimated share of the Claude subscription plan. Per
  period of a plan window (one label between two resets) it fits percent per
  dollar, by least squares through the origin, from the rise between samples
  against the Claude dollars in the same span (only one family's dollars for a
  window such as `Weekly · Fable`), and reports the fit's standard error. A
  request is priced by its period's factor, or the previous period's while its
  own has fewer than two samples (`borrowed`). The summary prints the per-ticket
  median and p95 percent and the pool's percent per hour on its busiest day.
- `cli.py`: the command and its queries.
- The page: `serve.py` is `just efficiency serve`, a standard-library server on
  127.0.0.1 that answers `GET` only. An address is a query
  (`/bead/<id>?since=7d` is `efficiency bead <id> --since 7d`, `/` is
  `summary`), `.json` appended gives its JSON and `section=1` the section alone.
  `page.py` renders a query's result as HTML from its JSON as it is (no figure
  is computed there), with one script that fetches a line's section the first
  time it opens. A category's colour comes from its place in the command kinds
  table, so it is the same in every bar and strip. `views.py` and `ci_views.py`
  add the views the header leads to (`cards`, `machine`, `ci`, `changes`,
  `findings`) and the sections they open (`metrics`, `pushes`, `failures`, one
  finding's items) to `page.RENDERERS`, built from the same parts. A view's
  trend lines are the metrics its result names under `more`, as lines open from
  the start whose section the script fetches at once; a trend marks each
  efficiency change on its metric, and the summary's figures carry the marks of
  the metrics in `rollups.FIGURE_METRICS`. `time` and `dollars` answer `.json`
  only. The server answers each connection on its own thread, because a browser
  holds connections open that it sends nothing on. A request never waits for an
  ingest: the server answers from the store as it is and keeps each answer until
  the next ingest. While the page is open, a thread runs `efficiency ingest` as
  a separate background-priority process whenever the store is over five minutes
  old, then computes the recently asked answers again. `/status.json` feeds the
  header's "updated" or "updating" note, which offers a reload once new data is
  in. `just efficiency serve` alone runs at normal priority.
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
  request or tool call when it happened in it, and a ticket when it closed in
  it; a closed ticket's per-bead figures count all its windows. A query ingests
  first when the store's last ingest is over a minute old, unless given
  `--no-ingest` or another ingest is already running, in which case it answers
  from the store as it is. The store is `$BERYL_CACHE/efficiency/store.sqlite`.
- Ignite's check families `black:efficiency`, `pyre:efficiency` and
  `pytest:efficiency/...` check this project. To run the same commands by hand
  from this directory: `uv run --project ../tools python -m pytest`, and the
  same with `python -m black --check .` and `pyre --noninteractive check`.
