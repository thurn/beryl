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
  each request and tool call; `bead.py` is the `bead <id>` query.
- `cli.py`: the queries `just efficiency` runs; `render.py`: results and their
  text tables.

## Running and checking

- `just efficiency` lists the queries. `just efficiency ingest`,
  `just efficiency sql '<query>'`, `just efficiency coverage` and
  `just efficiency bead <id>` take `--json`. A query ingests first when the
  store's last ingest is over a minute old, unless given `--no-ingest`. The
  store is `$BERYL_CACHE/efficiency/store.sqlite`.
- The `efficiency` check lane (`tools/src/beryl_tools/efficiency.py`) runs
  black, pyre and pytest here. Only a change under `efficiency/` or to
  `data/efficiency-*.json` selects it, and such a change selects no other lane
  but `repo`. To run its steps by hand from this directory:
  `uv run --project ../tools python -m pytest`, and the same with
  `python -m black --check .` and `pyre --noninteractive check`.
