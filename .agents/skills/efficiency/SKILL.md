---
name: efficiency
description: Find out where agent time, CPU, LLM tokens (dollars) or CI cost went for a ticket, epic, session, cohort or day, and find savings opportunities, with `just efficiency` and its local page. Use when asked what a ticket or epic cost, why a session was slow or expensive, what CI or pushes cost, which models or roles spend the most, or what to cut to save time or tokens.
---

# Efficiency tools

`just efficiency <query>` answers questions about cost from a SQLite store built
from the logs that sessions and tools already leave behind. Nothing records
anything at the time; ingest reads these sources afterwards:

- Claude Code transcripts and the desktop app's session records, and Codex logs
  (`~/.codex/sessions`, `.jsonl` only). Ticket windows (the stretch of a session
  spent on one ticket) come from the `ignite start` commands a session ran (and
  the older Enact commands in transcripts from before the switch).
- Ignite's hub, on the machine that holds its database: tickets, one record per
  command Ignite ran on any machine (checks, builds, cores and CPU), tips,
  hand-offs, workers, and the efficiency rows other machines' daemons posted.
- On machines without the hub database, Ignite's local ticket mirror and the
  daemon's check pass times, completed tip rounds and plan usage samples.
- `git log` and `master`'s reflog from the main checkout.

Rows from before the switch, from the old run log, CPU sampler, token pool,
lock log and `tk`, stay in the store as history.

The store is `$BERYL_CACHE/efficiency/store.sqlite`
(by default `~/Library/Caches/beryl/efficiency/` on macOS and
`%LOCALAPPDATA%\beryl\efficiency\` on Windows). The hub's store includes
fleet records and posted measurements. Other machines collect local
measurements and read fleet ticket metadata from their mirrors.
The design, with every definition, is `docs/efficiency-dashboard.md`; the code
is `efficiency/` (`efficiency/AGENTS.md`).

## Ingest

A query ingests first when the last ingest is over a minute old, unless another
ingest is running. Ingest can take a minute or more on a loaded machine, so pass
`--no-ingest` when the store is recent enough, which it usually is (the page's
server ingests every five minutes while open). To update it by hand:

```bash
just efficiency ingest
```

Never run `ingest --rebuild`: it recreates the store from the sources, and the
providers have deleted older transcripts, so history is lost.

## Queries

Every query takes `--json`, which prints exactly the numbers the text shows.
Queries that report figures take `--since` (`24h`, `7d`, `all` or a date) and
cohort filters: `--where key=value` (the label `key:value`), `--epic`, `--role`,
`--tier`, `--host`, `--provider claude|codex`. Roll-ups default to `7d`; `epic`,
`bead` and `session` default to `all`. Every result ends with a `more:` line
naming the queries that drill further; follow those.

```bash
just efficiency summary --since 24h              # beads closed, medians, time and dollar split, epics
just efficiency epic by-ig5e                     # one epic's beads with minutes and dollars
just efficiency ticket by-ig5e.16                  # one bead: time, dollars, pushes, sessions
just efficiency session local_58d77823           # one session's requests and tool calls in order (a prefix works)
just efficiency time --by kind --tier sonnet     # --by kind|model|tier|role|lane|epic|day
just efficiency dollars --by source --epic by-ig5e  # --by class|source|purpose|model|tier|role|lane|epic|day
just efficiency cards --by tier                  # cost per card file added, with rework beside it
just efficiency machine --since 7d               # cores in use, saturation, time queued for cores
just efficiency ci --lane oracle                 # check cost by script, family, check (--script check is the landing gate, ci is tip verification)
just efficiency pushes --since 24h               # the gate per landing: seconds, CPU, repeats
just efficiency failures --since 7d              # what each failure on staging cost (repair ticket, culprit search, time behind)
just efficiency findings --since 7d              # waste ranked by what removing it would save per week
just efficiency findings --rule carried-logs     # one rule's items in full
just efficiency metrics bead.dollars             # a named series by day (no name lists them)
just efficiency changes                          # entries in data/efficiency-changes.json, medians before and after
just efficiency coverage                         # what the store holds and misses
just efficiency sql "select count(*) from requests where model like 'claude-opus%'"
```

`findings` is where to start looking for savings. Its rules include
`small-reads`, `carried-logs`, `fixed-prefix`, `wake-ups`, `outlier-beads`,
`tier-gap`, `queued`, `compile-share`, `repeated-command`,
`chained-commands`, `never-caught` and `flake-cost`;
`just efficiency findings --help` lists them all. Each item names its evidence
query and example tickets.

`tier-gap` (move tickets to a cheaper model) is not a useful finding to report:
the user already knows it is an option. When asked for savings, leave it out and
recommend from the other rules.

`sql` is read-only and is for questions no named query answers. The main tables
are `beads`, `labels`, `sessions`, `windows`, `requests`, `tool_calls`,
`tool_results`, `window_time`, `window_dollars`, `checks`, `lane_runs`, `cpu`,
`cpu_minutes`, `leases`, `lock_waits` and `commits`. Read the schema in
`efficiency/src/beryl_efficiency/store.py` before writing a query.

## Other machines

Each row carries its host. To bring another machine's rows in, run on that
machine:

```bash
just efficiency export --since 2026-11-01
```

Then copy the file across yourself (`scp`) and run
`just efficiency import <file>` here. Importing the same rows twice changes
nothing. An export holds commands and paths: never commit or publish it.

## The page

```bash
just efficiency serve                     # http://127.0.0.1:8765, --port N, --no-ingest
just efficiency shot /bead/by-ig5e.16     # PNG under $BERYL_CACHE/efficiency/shots/
```

A page address is a query: `/bead/<id>?since=7d` is
`efficiency ticket <id> --since 7d`, `/` is `summary`, and `.json` appended
returns the JSON. Agents don't open windows, so look at the page with `shot`
(`--dark`, `--width N`; append `#open` to the address to open every line one
level deep). `shot` uses an installed Playwright package and a headless
Chromium or WebKit browser. `serve` runs until it is stopped, so start it in
the background if you need it at all; the text queries answer the same
questions.

## Not findings

The user schedules work on purpose, so these are never reported as
opportunities, in answers about velocity or in findings:

- Time a ticket waits before anyone schedules it: a ticket in an epic nobody
  has activated, or one set aside for a person.
- Hours when no ticket session is running, such as while the pool is paused.
- Sessions stopped by a usage limit (the weekly limit or a model's limit), and
  advice to avoid hitting one.

When asked about velocity (wall-clock time to shipped tickets), measure only
work the user has scheduled, usually the tickets of active epics, and only the
time while the pool was working them.

## Cross-checking findings against fixes

The store looks back over days, and the code changes faster than that. A finding
built on last week's data may already be fixed, or already have a ticket. Check
each finding before reporting it:

1. **Date the evidence.** Note when the waits, runs or commands behind the
   finding happened, by their earliest and latest timestamps. The store keeps
   UTC. `git log` prints the committer's local time unless you pass
   `--date=iso-strict`, so convert before you compare.
2. **Look for fixes since then.** Run `git fetch origin staging`, then
   `git log origin/staging --since=<earliest evidence> -- <paths>` over
   the code behind the finding: the tool, check or script the waits ran. Also grep the
   subjects and bodies (`-i --grep=<term>`). Read the body of each likely fix.
   It says what someone saw and what changed, so you can tell whether it covers
   this finding or only a nearby one.
3. **Look for tickets.** Search the open and in-progress tickets, descriptions
   as well as titles: `ignite ticket list --json`, filtered for the tool's name, the
   command and the symptom. A finding can also come from a ticket's own text:
   for example, the acceptance criteria that a tool writes into the tickets it
   files can send workers to run a slow command.
4. **Read the code as it is now** (`git show origin/staging:<path>`),
   not the main checkout, which may lag behind. Confirm that the cause you inferred
   still exists, such as a lock, a priority class or a retry loop.
5. **Re-measure only with enough data.** If a fix landed, compare the same query
   before and after it, and say how many hours or cases the "after" side holds.
   A few hours of data is not enough to call a fix a success or a failure. Say
   so, and report the finding as "fixed, not yet measured".

Report each finding as one of: **fixed** (name the commit), **covered** (name
the ticket and say what it leaves out), or **open** (no fix or ticket). Only an
open finding is a new opportunity. Offer to file a ticket for it.

### Pitfalls found while cross-checking

- A command run in the background ends, in `tool_calls`, when it was launched.
  Its real end is the session's `notifications` row for that call, or the next
  `prompts` row of kind `background` that names it.
- Before the switch, Enact's vizier sessions can appear inside ticket windows.
  Before blaming workers, drop sessions whose commands include
  `enact.py next --epic`.
- `just efficiency sql` ingests first unless given `--no-ingest`, which can take
  minutes on a loaded machine.

## Reading the numbers

- **"no data"**: tickets that ran on another host or provider whose rows aren't
  in this store (Cygnus, Codex on the MacBook). `summary` shows how many, and
  `coverage` lists them by host. Their figures are missing, not zero; import
  that machine's export to fill them in.
- An unknown value prints as `-`, never `0`. A dollar sum marked `+` is a lower
  bound, because a model in it has no price in `data/efficiency-prices.json`.
- **Agent minutes** are model time, tool time, time queued for cores and time
  waiting on background work. Waiting on a person and idle time (the session
  waiting for anything else, such as a scheduled wake-up) are shown beside agent
  minutes, never in them. Tool and queued time are split by command kind, from
  `data/efficiency-command-kinds.json`; a command that matches no pattern is
  "other", and `coverage` lists the longest ones.
- **Core minutes** are the CPU used in a ticket's tree, from Ignite's records
  (before the switch, from the CPU sampler).
- **Dollars** split three ways, each summing to the total: by class (cache read,
  cache write, output, uncached input); by source (output, the fixed prefix of
  each request, new content, and content carried from earlier turns, split by
  the tool that added it); and by purpose (working against wake-up requests,
  which found nothing to do and went back to waiting).
- **Rework** is what later tickets spent fixing a ticket's work: a
  regression ticket's cost when the ticket's commit broke a check on staging, and later `fix` commits
  to a card it added. That money is already another ticket's direct cost, so
  rework is shown beside direct cost and never added to it.
- In roll-ups, "orchestration" is the historical cost of an Enact run's
  orchestrator session, charged to its epic. "(no ticket)" covers other work
  outside ticket windows. The lines sum to the total.
- A window counts toward `--since` when it starts in the range and a ticket when
  it closed in it; a closed ticket's own figures include all its windows.
