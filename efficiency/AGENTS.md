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
- The store is the long-term record. `store.py`'s schema grows only by appending
  to `MIGRATIONS`, because a rebuild loses transcripts the providers have
  deleted. Rows written by the tools that Ignite replaced stay in the store, and
  tables and queries keep the name `bead` for a ticket.
- A query returns a typed dataclass whose numbers are already rounded. Its
  renderer and the page only format them, and `--json` prints the same
  dataclass, so the text, the page and the JSON cannot disagree.
- Derived rows are derived again for every transcript when their table changes:
  kinds and time splits when `data/efficiency-command-kinds.json` changes, and
  dollars when `data/efficiency-prices.json` changes.
- Rework is a share of another ticket's direct dollars, so it is shown beside
  direct cost and never summed into it.
- An export (`just efficiency export`) holds commands and paths. Copy it between
  your own machines and never commit or publish it.
- A page request never waits for an ingest: the server answers from the store as
  it is.
- Nothing assumes `/` as the path separator or a location that exists on only
  one operating system. `paths.py` finds the cache and the providers'
  directories on each.

## Code map

The modules are in `src/beryl_efficiency/` and the tests in `tests/`. Each
module's header comment says what it reads and writes.

- `paths.py` and `store.py`: the cache, the store, the ingest lock and the host
  name, and the SQLite schema.
- `ingest.py`: the ingest framework. A `LineSource` reads files of lines from
  their recorded offsets, a `FileSource` reads small files whole when they
  change, and a `SnapshotSource` stores a command's output when its digest
  changes.
- Sources: `ignite.py` reads the hub's records of commands, leases, CPU, checks,
  tickets and workers, only on the machine that holds the hub's database. It
  comes first in `cli.sources()` because windows read the tickets. `claude.py`,
  `codex.py` and `agy.py` read the providers' transcripts. `commits.py` reads
  `git log` of `staging` and `master`. `beads.py` and `ticket_ids.py` build
  ticket rows and resolve ticket IDs.
- Derived rows: `windows.py` (ticket windows, from the `ignite start` commands a
  session ran), `kinds.py` and `timing.py` (each window's seconds by command
  kind), `tokens.py` (dollars), `cpu.py` (leases and CPU minutes per ticket),
  `plan.py` (each ticket's share of the subscription plan), `rework.py` and
  `cilog.py` (what the `checks` and `lane_runs` rows share).
- The query layer: `scope.py` parses `--since` and the cohort filters,
  `facts.py` loads tickets and windows with the rows every query adds up, and
  `metrics.py` is the registry of named series, to which `ignite_metrics.py`
  adds the `ignite.*` ones.
- Queries: `rollups.py` (`summary`, `epic`, `time` and `dollars`), `bead.py`,
  `session.py`, `machine.py`, `ci.py`, `pushes.py`, `failures.py`, `cards.py`,
  `changes.py`, `spend.py` and `findings.py`. A finding's rule is one function
  in `dollar_rules.py`, `time_rules.py` or `cpu_rules.py`, built on the types in
  `finding.py`.
- Output: `render.py` (the `Result`, the field descriptions and the text
  tables), `cli.py` (the command), `exchange.py` (`export` and `import` between
  machines), and the local page in `serve.py`, `page.py`, `views.py` and
  `ci_views.py`. `shot.mjs` renders an address of the page to a PNG and is in no
  check.

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
