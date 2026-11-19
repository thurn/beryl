# Working in `tools/`

This adds to the root `AGENTS.md`, whose rules all apply here.

Ignite's daemon schedules the machine's CPU: it gives each command it runs a
number of cores in `IGNITE_CORES`, and a program that spreads its work over
threads sizes them to that (or to `os.cpu_count()` when run by hand). Long
background work that Ignite does not run, such as a tournament, runs at low
priority (`system.utility`) instead.

The adapters in `src/beryl_tools/ignite/` list each check family's checks and
report their results to Ignite (`ignite/checks/*.toml` names the adapter of each
family). An adapter imports only domain modules, never a command's own
scheduling or reporting, so that its input hashes cover what it runs.
`tools/tests/test_ignite_inputs.py` holds each family's declared inputs to the
modules its adapter imports.

A pytest check's inputs are its test file, the modules it imports and the files
`data/pytest-reads.json` says those modules read at run time. A test whose code
opens a tracked file that the record does not list fails with the entry to add,
so when new code reads a data file, add it there or run
`uv run --project tools python -m beryl_tools.ignite.python reads`.
