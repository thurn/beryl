# Working in `tools/`

This adds to the root `AGENTS.md`, whose rules all apply here.

Ignite's daemon schedules the machine's CPU: it gives each command it runs a
number of cores in `IGNITE_CORES`, and a program that spreads its work over
threads sizes them to that (or to `os.cpu_count()` when run by hand). Long
background work that Ignite does not run, such as a tournament, runs at low
priority (`system.utility`) instead.

The public app commands in `src/beryl_tools/cli.py` reuse the native command
parsers and runners in `src/beryl_tools/beryl3d/`. Look in `app.py` for desktop
build, packaging and smoke, `phone_launch.py` for phone build and launch, and
`phone.py` and `ios_smoke.py` for headless replays.

The adapters in `src/beryl_tools/ignite/` list each check family's checks and
report their results to Ignite (`ignite/checks/*.toml` names the adapter of each
family). An adapter imports only domain modules, never a command's own
scheduling or reporting, so that its input hashes cover what it runs.
`tools/tests/test_ignite_inputs.py` holds each family's declared inputs to the
modules its adapter imports.

On Windows, Ignite lists and runs pytest in the native Python environment. Pyre
uses the default WSL distribution, which needs Linux `uv` on its path or in
`~/.local/bin` and access to the drive holding the leased worktrees. The adapter
installs the locked dependencies in a separate Linux environment and runs Pyre
from each project's directory in the lease.

A pytest check's inputs are its test file, the modules it imports and the files
`data/pytest-reads.json` says those modules read at run time. A test whose code
opens a tracked file that the record does not list fails with the entry to add,
so when new code reads a data file, add it there or run
`uv run --project tools python -m beryl_tools.ignite.python reads`.
