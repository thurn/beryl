# Working in `tools/`

This adds to the root `AGENTS.md`, whose rules all apply here.

Ignite's daemon schedules the machine's CPU: it gives each command it runs a
number of cores in `IGNITE_CORES`, and a program that spreads its work over
threads sizes them to that (or to `os.cpu_count()` when run by hand). Long
background work that Ignite does not run, such as a tournament, runs at low
priority (`system.utility`) instead.

The public app commands in `src/beryl_tools/cli.py` reuse the native command
parsers and runners in `src/beryl_tools/client/`. Look in `app.py` for desktop
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

`src/beryl_tools/model_trial/` is `just model-trial`, which runs model trials.
In a trial, the pool works real tickets on one model (the source arm, which
lands as usual), and a worker on another model works a copy of each from a
frozen commit (the copy arm, which ends on a branch `trial/<copy id>` and never
lands). A spec may name several copy arms, each compared with the source as a
pair of its own. The tool then grades and compares the arms. A trial is its
epic. The epic's notes hold the trial spec (the arms, the block size, the budget
and the ordered ticket list), and every command takes `--trial <epic>`. The
records are JSON lines in `$BERYL_CACHE/model-trial/<epic>/` (`pairs.jsonl`,
`grades.jsonl`, and each pair's downloaded images), and are never committed.
`io.py` holds every call to `ignite`, git and the efficiency store, so the rest
of the package is pure and its tests pass made-up ticket JSON. `grade` reads
each arm's commands from the efficiency store's `tool_calls` table, which keeps
only each command's first 300 characters and no output. It reads output and the
model's messages from the session's Claude or Antigravity (`agy`) transcript,
whose path the store's `sources` table records, when the transcript is on the
grading machine. An arm whose transcript is not on it gets `minutes_partial`,
and the report leaves it out of the minute medians. Dollars and tokens come from
the store's `requests` rows tagged with the ticket during the arm's claim, from
every machine. An arm whose machine has a gap of over 30 minutes in those rows,
or no row at all, gets `cost_partial` and is left out of the dollar medians. A
landed source passed the landing gate; a copy passed when the Ignite daemon's
`verdicts` table (in `daemon.db`, passes only, kept 30 days) holds the tree hash
of its branch head, or when every failure of its last `ignite check` (the
daemon's `handles` replies) failed the same way at its start commit, as recorded
in `baselines.jsonl` by `model-trial baseline`. Grade a trial within 30 days of
its copies closing. `lane_runs` keeps no commit or tree hash for a run, so it
cannot give a verdict.

`review` runs a headless `claude -p` reviewer on each graded pair that is not
void, twice with A and B swapped, and appends its answers to `reviews.jsonl`.
`blind.py` decides what the reviewer and the user's page see: the ticket text,
each side's diff, a snapshot of its files made with `git archive` (so that no
commit message or ref names a ticket), its closing note, comments, filed tickets
and images. It scrubs model names, session IDs, the pair's ticket IDs, costs and
times from that text, and a test checks it. The prompt is `reviewer_prompt.txt`,
and its hash is the rubric version that each review row records, so editing the
prompt starts a new rubric. `report` prints the table (`--comment <ticket>` also
posts it, and two `--trial`s compare trials), `list` prints every trial in the
cache, and `judge` serves the user's blind page on 127.0.0.1 and appends the
user's picks to `verdicts.jsonl`.

To run another trial, follow the `model-trial` skill: it asks the user for the
arms, budget and tickets, and `just model-trial new --title <t> --spec <file>`
then files the trial epic, its block tickets and the report ticket from the
templates in `model_trial/templates/`. Tests: `tools/tests/test_model_trial.py`,
`test_model_trial_report.py` and `test_model_trial_new.py`.
