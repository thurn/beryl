# Efficiency dashboard

Project Beryl is built almost entirely by AI agent sessions: 1,828 commits
landed in its first week. This document designs a tool that shows where those
sessions' time, the machine's CPU and the LLM tokens go, for every unit of work,
so that the project can be made faster and cheaper on evidence.

The tool has two faces over one set of numbers:

- A command, `just efficiency`, whose output an agent can read.
- A local web page that draws the same results for a person, with detail hidden
  until it is opened.

Three requirements shape everything below:

- **Collection must cost agents nothing noticeable.** Almost every number is
  derived afterwards from logs that already exist. Only two things are added:
  one field on a log line that is already written, and one appended line each
  time a build or test command finishes using its share of the CPU.
- **The page shows nothing the command cannot print.** Every section of the page
  is a rendering of one query, and the same query is a command with a `--json`
  flag.
- **The page is quiet.** It shows a few numbers and bars. Words, tables and
  per-request detail appear only when the reader opens a section.

The examples are output of the shipped tool on the main development Mac, from
the dates given. Their figures change as work continues; the shape of each
result does not. [What the first week showed](#what-the-first-week-showed) says
what the first measurements found.

## Related information

- Epic `by-ig5e` (`ignite ticket show Tig5e`) tracks the work and records the
  decisions taken with the user on 2026-11-15.
- `docs/checks.md`, deleted when Ignite replaced Enact, described the fast
  check, slow CI, the run log and the CPU sampler that this design reads.
- `tools/src/beryl_tools/runlog.py` defines the run log, one line per check per
  run.
- `tools/src/beryl_tools/cpu.py` defines the CPU sampler and its row format.
- `tools/src/beryl_tools/tokens.py` defines the pool of CPU tokens that builds
  and tests draw from.
- `tools/src/beryl_tools/check_value.py` and `verdicts.py` define what a check
  costs, what it caught and how each failure is judged. They are behind the
  existing reports `just ci-value` (each check's catches per CPU second) and
  `just ci-report` (each check's runs, failures and seconds).
- `~/enact/docs/design.md` describes Enact, the tool that manages worktrees, the
  task queue and CI.
- Epic `by-bajq` (109 tickets) is the example project used throughout.

## Background: how work is organized

This section names the things the dashboard counts. A reader who knows the
project can skip it.

Tasks and sessions:

- A **ticket** is one task in the project's task tracker (`tk`). An **epic** is
  a ticket that groups child tickets into a project.
- Tickets carry labels. `tier:<name>` picks which of three Claude models works
  the ticket: `fable`, `opus` or `sonnet`, from largest and dearest to smallest
  and cheapest. `role:<name>` says what the session does: an `executor`
  implements, a `warden` reviews, a `steward` repairs CI. `lane:<name>` groups
  tickets that one session works through in order.
- A **provider** is the agent program that runs a session. There are two: Claude
  Code and Codex. Each keeps its own log of every session, called a
  **transcript** here.
- **Enact** starts one agent session per ticket or per group of tickets. It
  records on the ticket which session worked it (`enact_session`), which
  worktree it used (`enact_worktree`) and which commits it landed
  (`enact_commits`).
- An **orchestrator session** runs an epic: it starts worker sessions, waits for
  them and does no implementation itself. Enact calls it the vizier, and records
  its id on each ticket it started as `enact_vizier`.
- A **worktree** is a separate checkout under `.worktrees/` in which one session
  makes one change. Its directory name is usually the ticket's id.
- A **host** is a machine. Work happens on three: the main development Mac
  (named `jarvis` in ticket records), a second Mac and a Windows PC.

Checks:

- The **fast check** runs before every push and takes about a minute. **Slow
  CI** runs on the shared `staging` branch afterwards; `master` advances only
  when it passes.
- Both are split into **check lanes**, one per kind of file or test (`rust`,
  `ui`, `oracle` and so on). This document always says "check lane" for these,
  to keep them apart from the `lane:` label on tickets.
- A check lane is a sequence of **steps**, each one command, such as "run the
  Rust tests". A step reports the individual **checks** it ran, such as one Rust
  test or one screenshot.
- The **oracle** check lane plays card scenarios in XMage and Forge, two other
  Magic rules engines written in Java, and compares the results with Beryl's.
- Three other programs run checks on a schedule or on demand: the **browser
  sweep** runs every browser check every three hours, the **nightly** runs the
  slowest checks once a day, and **bisection** reruns checks on older commits to
  find which commit broke slow CI.
- The **run log** is a file with one JSON line per check per run. Its `script`
  field says which of those five programs made the run: `check` for the fast
  check, `ci` for slow CI, and one value each for the sweep, the nightly and
  bisection.
- Every check that has ever failed has its own closed ticket, labeled
  `ci-check`, on which each failure is recorded as a comment.
- A **Steward ticket** is filed automatically when slow CI fails. It names the
  range of commits that broke it (`enact_ci_range`) and a session with the role
  `steward` fixes it.

The machine:

- The **CPU sampler** is a background process that records, each minute, the CPU
  used by each worktree and each kind of work.
- A **CPU token** is permission to use one core. A build, test or check command
  takes a **lease** of several CPU tokens before it starts, and waits if none
  are free. This document calls such a command a **heavy command**. Where
  "token" appears without "CPU", it means an LLM token.

## What the first week showed

These figures are the tool's own, from `just efficiency` on the main development
Mac on 2026-11-16, over everything it holds (transcripts from 2026-11-04). They
show where the large costs are.

Token volume across 119,154 model requests, 2,226 closed tickets, with dollars
at API prices (`just efficiency dollars --by class --since all`):

```text
class        dollars       %       tokens
cache_read   5067.86   55.00  25033508327
cache_write  2224.80   24.00    261997464
output       1828.52   20.00     78716766
input          61.25    1.00     15280030
total        9182.42  100.00  25389502587
```

Almost all token volume is the model reading its own earlier context again.
Output is under a third of one percent of the tokens and a fifth of the dollars.
A request's context has a median of 182,118 tokens (90th percentile 383,754;
`just efficiency metrics request.context-tokens --since all`). The levers are
therefore the size of the context and the number of requests per ticket, not how
much the model writes. By model, Opus 5.5 is 68% of the dollars, Fable 5.2 23%,
Codex's two models 6% and Sonnet 5.5 3%.

By source, 31% of the dollars is the fixed prefix (the system prompt, tool
definitions and instruction files read again on every request), 20% output, 18%
carried results of shell reads and searches, and 14% the model's own earlier
output. Wake-up requests, which find nothing to do and wait again, are 1%.

Agent time totals 65,230 minutes (1,087 hours) across all windows, with a
further 122 hours waiting on a person and 39 hours idle
(`just efficiency time --by kind --since all`):

```text
257 h  model
103 h  background: orchestration wait (orchestrators waiting for workers)
 79 h  orchestration wait
 60 h  browser checks
 60 h  python scripts
 53 h  read and search (grep, sed, ls)
 45 h  just (recipes with no kind of their own)
 40 h  enact
 25 h  push, and 23 h more in pushes run in the background
 23 h  slow CI wait, and 17 h more in the background
 20 h  rust tests
```

Over the seven days to 2026-11-16 the same tool found:

- Slow CI ran 470 times for 350 core hours; 1,010,661 check runs; 176 catches,
  50 stale failures, 58 flaky and 65 mixed. The Rust lane used 282 core hours
  and caught 3 bugs, while the oracle lane used 25 core hours and caught 111.
- The fast check ran in 937 pushes; 13 repeats after losing the race to
  `staging` cost 547 seconds.
- Commands waited 2.6 hours for CPU tokens, and 18 cores were at least 90% in
  use in 35.7% of the minutes sampled.
- 78 slow-CI failures cost $316.75 in Steward sessions, and `master` stayed
  behind `staging` for 1,702 minutes because of them.
- Sessions waited 799 times for Enact's ownership lock: 136 agent minutes
  waiting and 22 syncing.
- The findings put the largest dollar saving in running tiers apart (`tier-gap`)
  and in runs of small reads (`small-reads`).

## Design overview

The tool reads existing logs into one local database, answers every question
with a named query over it, and renders each query's result as text, JSON or
HTML.

- **Sources** are files that other tools already write: session transcripts,
  ticket records, the git history, the run log, the CPU sampler's rows.
- **Ingest** reads only the bytes added to each source since the last ingest and
  writes rows to the store.
- **The store** is one SQLite file in the Beryl cache directory. It is the
  long-term record, because providers delete old transcripts.
- **Queries** are functions from a time range and filters to a result. A result
  is plain data: numbers, rows and the names of the queries that drill into it.
- **Renderers** turn a result into a text table, JSON or an HTML fragment.
  Because both the command and the page render the same result, they cannot
  disagree.
- **Findings** are rules that run queries and report the largest sources of
  waste, each with an estimate of what fixing it would save.

One query is mentioned often before its own section: `just efficiency coverage`
reports everything the tool does not know, such as hosts with no data and
commands it could not classify.

The code is Python using only the standard library. It lives in its own
directory with its own check lane, described under [Isolation](#isolation), so
that changing it can affect nothing else.

## Units

Efficiency is resources divided by output. This section fixes both.

The unit of output is the **closed ticket**. Every number can be shown per
ticket, and rolled up by epic, by label or over the whole project.

- A **cohort** is a set of tickets chosen by filters, such as "closed in the
  last 7 days, `tier:sonnet`, `role:executor`". Any ticket label can be a
  filter, so a label such as `exp:short-brief` on some tickets is enough to
  compare an experiment against the rest.
- **Cards** are a cohort: the tickets whose commits add a file under
  `rust/cards/`. For those tickets the tool also divides by the number of card
  files added, to give cost per card.
- Work done in a session that names no ticket is reported under "(no ticket)",
  so totals always add up to the whole.

Four resources are measured for each ticket:

- **Agent minutes**: wall-clock time during which a session was working on the
  ticket, split by what the session was doing. See
  [Time accounting](#time-accounting).
- **Core minutes**: CPU time used in the ticket's worktree. See
  [CPU accounting](#cpu-accounting).
- **Dollars**: every request's tokens multiplied by that model's published API
  price. See [Token accounting](#token-accounting). The figure is a common unit
  for comparing models, whether or not the account is billed that way.
- **Rework**: the cost of later fixes that the ticket's commits caused, shown
  beside the direct cost and never added into it. See [Rework](#rework).

Two further measures describe the flow of work and not its cost:

- **Lead time**: from a ticket's creation to its start (time in the queue), from
  start to close (time being worked), and from its last commit reaching
  `staging` to that commit reaching `master`.
- **Throughput**: tickets closed per day.

Medians are used for per-bead figures, with the 90th percentile available on
drill-down, because a few very large tickets dominate any mean.

## Sources

The sources use data that other tools already record.

- **Claude Code transcripts** (`~/.claude/projects/<project>/<id>.jsonl`, and
  `<id>/subagents/*.jsonl` for subagents). Each assistant message carries a
  message id, the model, a timestamp and a `usage` object with the four token
  classes. Each tool call and each tool result has a timestamp, the tool's name
  and its input, such as a shell command.
- **Claude desktop session records** (one JSON file per session in the desktop
  app's data directory). Each maps the session id that Enact records on a ticket
  (`local_<uuid>`) to its current transcript id (`cliSessionId`) and its earlier
  ones (`priorCliSessionIds`, with the time each ended). A session that Enact
  clears between tickets gets a new transcript each time.
- **Codex session logs** (`~/.codex/sessions/<date>/rollout-*.jsonl`, and
  `archived_sessions`; compressed `.zst` files are not read). The `session_meta`
  line gives the working directory and each `token_count` event gives the tokens
  of the last request and the running total. Tool calls and their outputs are
  timestamped.
- **Tickets**, read from Ignite's hub database (`ignite.py`), which replaced
  `tk export` and `tk events`. The hub's ticket rows supply IDs, parents,
  labels, creation, claim and close times, aliases and landed commits. Session
  boundaries come from the `ignite start <ticket>` commands in transcripts. The
  store keeps the tickets, events and verdicts that `tk` supplied before the
  switch.
- **Git history** of `staging` and `master`: each commit's time, subject and
  changed paths.
- **Ignite's records**, one per command on any machine, from the hub database.
  They replaced the run log, the CPU token pool's lease log and the CPU sampler
  below, which are no longer written; the rows they produced stay in the store.
  Each record becomes an `ignite_records` row, a `leases` row, `cpu` and
  `cpu_minutes` rows, and for a check family `checks` and `lane_runs` rows.
- **The run log** (history: the hour files in `ci/runs/` in the cache, and the
  old single file `ci/runs.jsonl`). Each line has `time`, `host`, `commit`,
  `script`, `lane`, `check`, `outcome` (`pass`, `fail` or `skip`), `seconds`,
  `cpu_seconds` and `load`. The first record of each step also has
  `step_seconds` and `step_cpu_seconds`, the whole step's wall and CPU time.
- **CPU sampler rows** (history: `cpu/samples.jsonl` in the cache). Each row
  covers about a minute and has `time`, `seconds`, `cores` (how many the machine
  has), `cpu` (CPU seconds keyed by who asked for the work, the kind of work and
  the worktree), `peak_cores` and `load`. The sampler's name for "who asked for
  the work" is the context; this document calls it the **owner**, to keep
  "context" for what a model reads. Owners include a session, the fast check and
  slow CI.
- **Check verdicts** (history: the hub does not supply them yet). On the ticket
  of a check that has failed, each failure is a comment holding a JSON object
  with the commit and a `verdict`. See [CI accounting](#ci-accounting) for the
  verdicts.
- **Enact's lock-wait log** (history: nothing in Ignite waits on one lock, so it
  has no counterpart and is no longer read; `.git/enact/lock-waits.jsonl` in the
  main checkout, rotated to `lock-waits.1.jsonl` at 8 MB). Enact writes one line
  per acquisition of its ownership lock: `time` (when the wait began), `pid`,
  `command`, `outcome` (`acquired` or `gave_up`), `waited_s`, `held_s`, `sync_s`
  (seconds syncing tickets with the remote while holding the lock), `syncs` and
  `holder` (the command holding the lock once a wait passed a second).

The design adds two things to what is collected, and three small data files.

- The run log gains a `worktree` field on each line: the name of the checkout
  the run was made in. Without it a fast-check run cannot be tied to a ticket.
- The CPU token pool appends one line per lease when the lease is released. See
  [CPU accounting](#cpu-accounting) for the format.
- `data/efficiency-prices.json` holds model prices,
  `data/efficiency-command-kinds.json` names kinds of shell command, and
  `data/efficiency-changes.json` lists changes made to improve efficiency. Each
  is described where it is used.

### Collection budget

The rule for anything new is that it may cost a heavy command one file append,
and may cost any other command nothing.

- No hook runs inside an agent session, and nothing is added to a prompt.
- No command is wrapped in a timer. Durations come from the timestamps the
  session transcript already has.
- Ingest never runs inside a session's command. It runs at background priority
  when a query needs fresher data, or from `just efficiency ingest`.
- The tool measures itself: `just efficiency coverage` reports the CPU seconds
  and wall seconds of each ingest and of the sampler, as a share of the
  machine's total.

## Attributing sessions to tickets

A session often works several tickets one after another, and an orchestrator
works none. This section says which ticket each second and each request belongs
to.

A **ticket window** is the span of one transcript during which the session was
working one ticket. Ingest finds windows from the session's own shell commands.

- A window for a ticket opens at the first tool call that runs
  `enact start <ticket>` or `enact claim <ticket>`. A later `start` or `claim`
  of the same ticket inside its open window changes nothing.
- A window closes when `enact close <ticket>` returns, at the opening of another
  ticket's window, or at the end of the transcript, whichever comes first.
- `enact close <ticket> --continue` closes that ticket and claims the next
  ticket in its lane. Enact prints the claimed ticket's id in the command's
  result, and the next window opens at that result.
- When that result names no ticket, because the session cut its output short,
  the next ticket the session closes or creates a worktree for opens its window
  at the close.
- Requests and tool calls before the first window in a transcript belong to that
  first window, since they are the session reading its instructions for it.
- Activity after a window closes and before another opens belongs to "(no
  ticket)".
- A close with no open window for that ticket is ignored and counted by
  `coverage`.
- A subagent's transcript belongs to the window that contains the tool call
  which started it. Its requests add to the ticket's dollars. Its time adds
  nothing: it runs inside the parent's `Agent` tool call, which is already
  counted, and is shown as a breakdown of that call.
- A ticket worked by several sessions, or by one session in several transcripts,
  has several windows. Its cost is their sum.

Here is one transcript that worked two tickets in a lane:

```text
07:03:45  enact start by-bajq.6.9             window by-bajq.6.9 opens
07:38:21  enact close by-bajq.6.9 --continue  it closes; the result names
                                              by-bajq.6.13, whose window opens
08:10:02  enact close by-bajq.6.13            window closes
08:10:02  onwards                             "(no bead)"
```

Sessions that never open a window are attributed in this order:

- An orchestrator session, recognised by `enact_vizier` on the tickets it
  started. Its whole cost goes to that epic under the heading **orchestration**,
  not to any one ticket.
- A session that ran `enact worktree create <name>` is listed under "(no
  ticket)" with that name.
- Any other session is listed under "(no ticket)" with its title.

Every row in the store carries the host and the provider (`claude` or `codex`).
A ticket whose metadata says it ran on a host with no data in the store is shown
as "no data", never as zero. See [Other machines](#other-machines).

## Time accounting

This section splits each ticket window so that every second falls in exactly one
category, and the categories sum to the window's length.

At any instant a window is in exactly one of six categories. Where more than one
could apply, the first in this list wins.

- **Waiting on a person**: a question put to the user is unanswered, or the
  session ended its turn and the next event is a message that a person typed.
  The tool call that asks the question counts here and not as a tool. There is
  no upper limit: a question left overnight is all waiting on a person.
- **Queued for cores**: a tool call is running and its command is waiting for a
  CPU lease. It is subdivided by kind of command. See
  [Command kinds](#command-kinds).
- **Tool**: a tool call is running, from the call to its result. It is
  subdivided by kind of command. An Enact command's seconds waiting for Enact's
  ownership lock and syncing tickets under it are the kinds `enact lock wait`
  and `enact sync`, taken from its kind in proportion (history: Ignite writes no
  lock-wait log, so only calls from before the switch have them): each line of
  the lock-wait log is joined to the top-level tool call that ran its command
  and was running for the whole of its wait and hold, preferring one that names
  its ticket. When the ticket server queues a push for the staging lease, its
  local log records the wait against the push's ticket. Those seconds move from
  `enact push` to `enact lease wait`.
- **Model**: the model is generating, from a tool result or a message to the
  next assistant message. This applies whether or not a background command is
  also running.
- **Waiting on background work**: the session ended its turn, and the next event
  is a notification that a command it started in the background, or another
  session, has finished. It is subdivided by the kind of that command.
- **Idle**: the session ended its turn and the next event is anything else, such
  as a scheduled wake-up.

**Agent minutes** are queued for cores, tool, model and waiting on background
work. Waiting on a person and idle are reported beside them and are not part of
agent minutes, because no agent or machine is working during them.

When several tool calls run at once, the overlapping seconds are divided equally
among the calls running at that instant, so a window's categories still sum to
its wall-clock length.

How the transcripts show these, as built:

- A turn ends at a request whose `stop_reason` is not `tool_use`, or at an
  interrupt (`[Request interrupted by user]`). The wait runs from the last
  record before the next prompt to that prompt, and is not counted while a tool
  call is still running.
- A prompt's `origin.kind` says where it came from: `human` is a person, `peer`
  is another session, and `task-notification` is a background command (its
  `<tool-use-id>` names the call, which gives the kind) unless its `subkind` is
  `scheduled-trigger`, which is idle. Older transcripts without `origin` are
  told apart by how the text starts.
- Model time is every second that no other category claims.
- Each window's split is stored in the `window_time` table, with a row per
  category, kind and part, so later queries sum rows instead of replaying
  transcripts.

Two kinds of tool time are split one level further, because the split decides
what to optimise:

- **Compile against run.** Cargo prints `Finished ... in 42.10s` when a build
  ends. Ingest reads that line from the tool result and counts that many seconds
  of the command as compile and the rest as run. This applies to every command
  that builds before it runs. A command that prints several such lines has their
  seconds added together.
- **Inside a push.** `enact push` rebases, runs the fast check and pushes. The
  `lane_runs` rows with the same worktree and a time inside the tool call give
  the seconds of each check lane. What remains is rebase, waiting and push. The
  `pushes` query and the `bead` query's push lines show this split; it is not in
  `window_time`.

The `time` lines of the `bead` query show one ticket's split, largest first. A
wait on background work carries the prefix `background:`, so `push` is the
seconds a session spent in the command and `background: push` the seconds it
spent waiting for a push it had started in the background to finish:

```text
time      background: fast check 9.1 | background: push 8.9 | model 5.8 | read and search 3.6
          enact 2.2 | rust tests 1.6 | fast check 0.8 | the rest 1.9
beside    waiting on a person 0.0 | idle 0.0
```

### Command kinds

A shell command is given a kind by an ordered table of patterns in
`data/efficiency-command-kinds.json`, so that a new command can be named without
changing code. The first matching pattern wins. This excerpt shows the format:

```json
[
  { "kind": "push", "pattern": "enact(\\.py)? push\\b" },
  { "kind": "slow CI wait", "pattern": "enact(\\.py)? ci --wait" },
  { "kind": "orchestration wait", "pattern": "enact(\\.py)? (next|wait)\\b" },
  { "kind": "rust tests", "pattern": "just test-rust|cargo (nextest|test)\\b" },
  { "kind": "build", "pattern": "cargo (build|check|clippy)\\b" },
  { "kind": "read and search", "pattern": "^(rg|grep|sed|cat|ls|head|tail)\\b" }
]
```

- A command is split into the parts chained with `&&`, `||` or `;`, and each
  part is matched with its leading `cd <path>`, environment assignments and
  wrappers such as `timeout 600` removed, so a path that happens to contain a
  word such as `enact` does not decide the kind.
- A here-document's body, such as a commit message passed with
  `git commit -F - <<'EOF'`, is data and is left out before the command is
  stored or split: the store keeps a command's first 300 characters, and a long
  message used to push an `ignite land` after it out of them, so a three-minute
  landing was counted as `git`. The store keeps the operator as
  `<<(EOF body left out)`. Commands stored with their bodies before this were
  read again once from the transcripts that still exist.
- A command that chains several heavy commands is given the kind of the first
  heavy one, and is flagged as chained, since its parts cannot be timed apart.
  An entry is heavy when it has `"heavy": true`. A command with no heavy part
  takes the kind of its first part that matches.
- Tools other than the shell are their own kinds: `Read`, `Edit`, `Agent` (a
  subagent), and so on.
- Commands that match nothing are kind **other**. The `coverage` query lists the
  most expensive unmatched commands, so the table can be extended. The share of
  tool time that is "other" should stay under 5%.

## Token accounting

Dollars are computed per request and then summed. This section defines the
price, and then splits a request's cost by what was in its context, because the
context is where the money goes.

### Price

Each request has four token classes: uncached input, cache write, cache read and
output. `data/efficiency-prices.json` gives a price per million tokens for each
class of each model, copied from the provider's published price list on the date
it records.

```json
{
  "tables": [
    {
      "as_of": "2026-11-15",
      "source": "the address of the provider's price page",
      "models": {
        "example-model": {
          "input": 3.0, "cache_write": 3.75, "cache_write_1h": 6.0,
          "cache_read": 0.3, "output": 15.0
        }
      }
    }
  ]
}
```

- Claude Code writes to the 1-hour cache, which costs more than the 5-minute
  one. The transcript's `usage.cache_creation` says how many cache write tokens
  went to each, and each part is priced at its own rate. Both are still the one
  class "cache write".

- A model may also have `above`, its prices for a prompt over a number of
  tokens, as Haiku 5.5 has.

- The earliest table also prices requests older than it, since the store holds
  requests from before the file was written.

- A request by a model that is not in the table has an unknown price. Its tokens
  are still counted, its dollars are null, and `coverage` lists the model. A sum
  that includes such requests is marked as a lower bound.

- A price change adds a new dated table and keeps the old one. A request is
  priced by the table in force on its date, so past figures do not move.

- The transcript repeats one assistant message on several lines, one per content
  block. Ingest counts each message id once.

### Parts of the context

A request's **context** is everything the model reads for it: its uncached
input, cache write and cache read tokens together. Ingest divides each request's
context into three parts that do not overlap and that sum to the context.

- The **fixed prefix** is the context of the first request in a transcript: the
  system prompt, the tool definitions and the project's instruction files. It
  includes the session's first prompt. Its size is measured once per transcript
  and is the same for every request in it. A session that is cleared starts a
  new transcript, which has its own prefix.
- **New content** is what arrived since the previous request: tool results and
  user messages. Its size is this request's context, less the previous request's
  context, less the previous request's output, and never less than zero. Where
  the context shrank by less than half, new content is zero and carried content
  is whatever remains after the prefix.
- **Carried content** is the rest: everything after the prefix that an earlier
  request had already read, including the model's own earlier output.

```text
request 30   context 181,000   output 400
request 31   context 189,600
  new content      189,600 - 181,000 - 400 = 8,200
  fixed prefix      81,000   (measured at request 1)
  carried content  189,600 - 81,000 - 8,200 = 100,400
```

Two rules complete the split:

- **Who added it.** A request's new content is assigned to the tool calls whose
  results arrived since the previous request, in proportion to the size of their
  results in bytes. A user message is assigned to "user".
- **Compaction.** When a request's context is less than half of the previous
  request's context, the provider has replaced the conversation with a summary.
  At that request the whole context beyond the fixed prefix is new content
  assigned to "compaction", and everything carried before it stops being
  carried.

### Carrying cost

The **carrying cost** of a tool result is what it costs to read that result
again on every later request. It is the number of tokens the result added,
multiplied by the number of later requests in the same transcript up to the next
compaction, multiplied by the model's cache-read price per token.

```text
tool result: `just test-rust` output, 8,200 tokens, at request 31 of 164
later requests, with no compaction before the transcript ends: 133
tokens read again: 8,200 x 133 = 1,090,600
carrying cost: 1,090,600 x the cache-read price per token
```

Carrying cost is an estimate attached to the tool call that produced the result,
wherever the later requests fall. It is used to rank tool results and by the
findings. It is not a fourth way of dividing a ticket's dollars: those are
always the actual dollars of the requests in the ticket's windows.

This is the number that explains expensive tickets. A long test log printed
early in a long session costs far more than the test run itself.

### Three ways to split dollars

Dollars for any ticket or cohort can be split in three independent ways. Each
split sums to the total.

- **By class**: cache read, cache write, output, uncached input.
- **By source**: output, plus the three parts of the context. A request's
  input-side dollars (everything but output) are divided among its fixed prefix,
  its carried content and its new content in proportion to their token counts.
  Carried content is further divided by the kind of tool that added it.
- **By purpose**: **wake-up requests** against working requests. A wake-up
  request is one that makes at least one tool call, all of the kinds
  "orchestration wait" or "slow CI wait": the session woke, found nothing to do
  and went back to waiting.

The `dollars --by source` query gives the split by source. This is one epic's:

```text
dollars by source  since the start (all)  --epic by-bajq

source                                             dollars       %      tokens
fixed prefix                                        322.54   29.00  1187411286
carried: read and search                            276.05   25.00  1177764238
carried: model output                               177.02   16.00   775458664
output                                              152.71   14.00     7866026
carried: user                                        54.31    5.00   255625208
carried: enact                                       26.12    2.00   104389657
carried: python scripts                              22.70    2.00    97869530
new: user                                             9.98    1.00     2141335
```

For Codex, the log gives input, cached input and output per request, and its
input figure includes the cached part. Cached input is treated as cache read,
input less cached input as uncached input, and the same three splits apply.

## CPU accounting

Core minutes come from the CPU sampler, which attributes each minute's CPU to an
owner, a kind of work (compile, test, browser, oracle) and a worktree.

Each minute of a worktree's CPU is given to one ticket by these rules, in order:

- The ticket whose `enact_worktree` names that worktree and which had a window
  open at that minute.
- Otherwise, the ticket whose `enact_worktree` names that worktree and whose
  start and close times contain that minute. This covers a command still running
  after the session ended its turn. If several tickets qualify, the one that
  started most recently gets it.
- Otherwise it is **shared** CPU, reported under the worktree's name.

The worktree decides, not the owner: the CPU of a fast check that ran in a
ticket's worktree belongs to that ticket.

Shared CPU also includes everything outside a ticket's worktree: slow CI, the
browser sweep, the nightly, bisection, editors and the desktop apps. It is
reported per day, by owner and kind, and also divided by tickets closed that
day. The sampler's own CPU and each ingest's CPU are listed as their own lines.

The machine being busy is measured two ways, since a busy machine is what stops
more sessions running in parallel.

- **Saturation** is the share of minutes in which the average cores in use (the
  row's CPU seconds divided by its seconds) were at least 90% of the cores the
  machine has.
- **Queued for cores** is exact: the CPU token pool writes one line when a lease
  is released, with how long the command waited to get it. A wait that never got
  a lease also writes a line, with `cores` 0 and an `outcome` of `gave_up` or
  `abandoned` (its process died waiting, as a step killed at the fast check's
  limit does). The narrower-lease table leaves those lines out.

```json
{"time": "2026-11-15T14:25:49Z", "class": "agent", "worktree": "by-bajq.6.9",
 "command": "test-rust", "cores": 6, "wanted": 12, "waited_s": 41.2,
 "held_s": 97.0}
```

- `cores` is how many CPU tokens the lease got and `wanted` is how many the
  command would have used on an idle machine. The difference shows commands
  running narrower than they could.
- `class` is the pool's priority class: `push` for the fast check of a push,
  `agent` for a command an agent ran by hand, `ci` for slow CI and `queue` for a
  check that Enact's merge queue runs.
- `waited_s` joins to the tool call that was running in that worktree at that
  time. Those seconds move from the tool category to "queued for cores".
- The line is written under the pool's existing lock, by the process that
  already holds it, so it adds no new contention.

From these the tool reports a **parallel capacity estimate** for a cohort: the
machine's cores divided by the mean cores a session of that cohort uses, rounded
down. A ticket's cores are its core minutes divided by its agent minutes. For
the card cohort this says how many card-writing sessions the machine can run at
once before they queue, which is the figure that matters for running thirty in
parallel. The `machine` query shows the saturation, the time queued and the
estimate for the whole project; the `cards` query shows the estimate for cards.

```text
machine  since 2026-11-09T03:40:04 (7d)
cores 18   in use 14.4 mean, 18.0 in the busiest minute   saturation 35.7% of 691 minutes   (68 minutes sampled over the cores, counted as all busy)
queued for cores  2.62 h waiting, 23.73 core h
...
cpu       183.33 core h: beads 84.75 | shared 109.12 (206.16 per day, 0.06 per bead closed)
shared    system/system 32.52 | desktop/desktop 15.61 | session/tool 10.90 | worktree ci-runner 9.44
own cpu   sampler 0.002 cores (0.01%) | ingest 315.1 s (0.038%)
capacity  cores per session 0.9 mean, 1.6 p90 over 83 beads; parallel capacity 19 sessions
```

- A sampled minute whose CPU seconds exceed its cores counts as all busy, and
  the query says how many minutes it did that to.
- `queued for cores` gives hours of waiting and core hours, which is each wait
  multiplied by the tokens the command wanted. Under it the query lists the
  commands by class, and then those that got fewer tokens than they wanted.

## CI accounting

CI is the largest shared cost, and the question is not only what it costs but
what each part of it buys. This section defines the cost of a check, its value,
and the costs that follow a failure.

Every check run is already in the run log with its CPU seconds, and every
failure already has a **verdict**:

- **catch**: the failure was fixed by changing product code. The check found a
  real bug.
- **stale**: the failure was fixed by changing the check itself.
- **flaky**: the check failed and then passed on the same code.
- **mixed**: the fix changed both the check and product code, so the failure is
  neither clearly a catch nor clearly stale. `by verdict` in the CI tools can
  correct it.
- **miss**: slow CI failed a check that the fast check had not chosen to run for
  the commit at fault. A miss is recorded in addition to the catch or stale
  verdict that the fix later brings.

The store keeps one `checks` row per check per local day per host, script and
check lane, with runs, passes, failures, skips, CPU seconds and wall seconds. It
does not copy the run log's individual lines, which already exceed 700 MB. Runs
of a check lane are the exception, because each fast-check run must be tied to a
push: the store also keeps one `lane_runs` row per run of a check lane, with its
worktree, commit, tree, start time, wall seconds, CPU seconds and the number of
checks and failures.

### Cost and value of checks

Checks form a tree that the page and the command drill into: script, check lane,
**family**, check. A family groups related checks, and is derived from the
check's name by these rules:

- A name beginning `rust:` belongs to the family `rust:<binary>`, the text up to
  the first `::`.
- A name beginning `oracle:` belongs to `oracle:<engine>`, where the engine is
  the text after the last `/`.
- Any other name belongs to the family named by the text before its first `:`,
  or by the whole name if it has none.

```text
rust:beryl_cli::longthink::stops_cost_nothing   family rust:beryl_cli
oracle:Subtlety/generated 9/forge               family oracle:forge
oracle:Subtlety/generated 9/xmage               family oracle:xmage
ui-test:Enter on the start screen starts a game family ui-test
verb:draw/yours/phone                           family verb
```

At every level of the tree the same figures are shown:

- **CPU seconds** spent in the range, and the median for one run.
- **Runs**, and the share of runs in which it was skipped.
- **Catches**, **stale failures**, **flaky failures** and **misses**.
- **CPU per catch**: CPU spent divided by catches. A family with no catch shows
  "never", with the date of its first run so that a new check is not judged too
  early.
- **Flake cost**: the CPU and wall seconds of the reruns that flaky failures
  caused. The cost of the sessions that worked the tickets filed for flaky
  checks is shown beside it, in dollars.

Failures count under the script that recorded them, so a catch by the fast check
is not counted in slow CI's tree. This differs from `just ci-value`, which
credits a check with catches from any script and counts the checks of a group
that fails together once per commit. The two agree on slow CI's runs and CPU
seconds. The run log is kept added up per day, so a check's CPU for one run is a
mean, not a median.

```text
slow CI (ci)  since 2026-11-09T03:40:07 (7d), checks from 2026-11-08
ci runs 470   cpu 349.57 core h (87.4% estimated from wall seconds)   check runs 1010661   skipped 1.4%
failures   catch 176 | stale 50 | flaky 58 | mixed 65 | miss 82   core h per catch 1.99
flake cost 2122.0 cpu s, 1266.7 wall s in reruns; 20.00 dollars on beads to delete flaky checks

lane            core h    runs  skip %  catch  stale  flaky  mixed  miss  core h per catch        flake cpu s  flake $
rust            281.55  360173    0.00      3      6      3     11     0  93.85                          3.70     4.39
oracle           24.72  256323    5.00    111      0     15      7    61  0.22                        1878.50     1.87
  oracle:xmage   14.20  124517    7.70     95      0      9      3    56  0.15                           4.20     1.87
  oracle:forge    8.69  131799    2.30     16      0      4      4     5  0.54                           1.00     0.00
app              10.44     448    5.90      1      0      1      0     0  10.44                         83.90     0.00
ui-test           9.99  103373    0.00     14     40     32     38    19  0.71                          10.60     6.30
ui-smoke          0.97     330    5.20      0      0      0      0     0  never since 2026-11-13         0.00     0.00
```

The CPU is estimated from wall seconds where the run log has no measurement, and
the query says what share. A lane's sub-lines are its families. The `ci` query
also lists the oracle scenarios by engine, with those played, those skipped and
those remembered from earlier runs.

### What kind of process the CPU went to

The run log says which check used the CPU. The sampler says which kind of
process used it: `compile`, `rust_test`, `oracle` (the Java virtual machines
that run XMage and Forge), `browser`, `tool` and so on. The two are joined by
owner and minute.

- For each script and check lane, the tool shows CPU by kind of process. This
  answers "how much of slow CI is Java" and "how much of the fast check is
  compiling" directly.
- For the oracle check lane, the cost is also split by engine from the check
  names, and into scenarios that ran against scenarios that were skipped because
  an earlier run had recorded their outcome.

```text
cpu by kind of process
  compile          6.54 core h   53.4%
  tool             3.61 core h   29.5%
  oracle           1.44 core h   11.7%   (Java: xmage 0.89, forge 0.55)
  other            0.50 core h    4.1%
  rust_test        0.16 core h    1.3%
  browser          0.00 core h    0.0%
```

This is the `ci` query for one check lane, `rust`, over seven days.

### The fast check

The fast check runs on every push, so its cost is paid by each ticket.

- **Per push**: wall seconds, CPU seconds, seconds queued for cores, and which
  check lanes ran.
- **Left out**: the steps that the fast check did not finish because it reached
  its time limit and handed them to slow CI, by step.
- **Repeats**: pushes that lost the race to `staging`, meaning another push
  landed first, and had to rebase and check again, with what the repeats cost.
- **Failures before a push** are counted by verdict in the same way as slow CI,
  so a fast check lane's catches can be set against its cost.

### What a slow-CI failure costs

A failure of slow CI has costs that no check's own figures show.

- **The Steward ticket**: the agent minutes, core minutes and dollars of the
  session that fixed it.
- **Bisection**: the CPU of the reruns that found the commit at fault.
- **Time behind**: the time during which `master` stayed behind `staging`
  because of the failure, from the failing run's end to the next passing run's
  end.
- **Misses**: each miss is listed with its check, since it argues for changing
  what the fast check selects.

These are summed per failure and per day. The Steward's cost is also shown as
rework on the tickets at fault, as the next section describes.

## Rework

A ticket's direct cost says what it took to land. Rework says what it cost
afterwards. Only cases that can be traced without guessing are counted.

Rework is a second view of costs that are already counted once as some other
ticket's direct cost. Project totals sum direct costs only, so nothing is
counted twice.

**Slow-CI breakages.** A Steward ticket names a commit range. The tickets with
commits in that range are the candidates.

- The Steward's fixing commits are the commits the Steward ticket landed. A
  Steward ticket is never itself a candidate.
- The candidates at fault are those whose commits in the range changed at least
  one file that the fixing commits also changed.
- If no candidate qualifies, all candidates are at fault.
- The Steward ticket's whole cost is divided equally among the tickets at fault.

**Fixes to cards.** A later commit whose subject begins with `fix` and that
changes a card file is rework for the ticket whose commit added that file.
Commits landed by a Steward ticket are counted only under the first rule.

- The fixing ticket may have done other work too. The part of its cost that is
  rework is its cost multiplied by the card files its `fix` commits changed,
  divided by all files its commits changed. A file counts once however many
  commits changed it.
- That part is divided equally among those card files, and each share goes to
  the ticket that added the file.

A ticket is never charged with rework from its own commits. A ticket's rework
can grow after it closes. Its direct cost cannot.

The example compares cards written on two tiers. Rework is shown per card in
dollars, and `fixed later` is the share of cards that a later `fix` commit
changed.

```text
cards by tier  since 2026-11-09T03:40:04 (7d)
per card: direct dollars, rework dollars and agent minutes

tier    beads  no data  cards  direct  rework  agent min  fixed later
opus       34        0     52   12.50    0.33       71.9  56%
sonnet     14        0     21    1.21    0.03       41.7  19%
total      48        0     73    9.25    0.24       63.2  45%
```

That comparison, direct cost with rework beside it, is what decides whether a
smaller model is really cheaper for a kind of work.

## Efficiency changes

When someone changes the project to save time or tokens, the dashboard should
show whether it worked. Such a change is recorded as an entry in
`data/efficiency-changes.json`, in the same commit that makes it. An entry names
what the change was, the metric it was meant to move and the direction:

```json
{
  "label": "Slow CI reuses one worktree and keeps its target directory",
  "metric": "slow-ci.cpu-seconds-per-run",
  "expect": "down"
}
```

- The change's commit is the one that added the entry to the file, which the
  tool finds from the file's history. An entry may instead carry a `commit`
  field naming an earlier commit on `staging`, for changes made before the file
  existed.
- `metric` names one of a fixed set of series, which `just efficiency metrics`
  lists. Examples: `bead.dollars`, `bead.agent-minutes`, `push.wall-seconds`,
  `fast-check.cpu-seconds`, `card.dollars`, `request.context-tokens`,
  `machine.queued-core-hours-per-day`.
- A metric may carry a cohort filter, such as `bead.dollars[tier=sonnet]`.
- The dashboard marks the commit on that metric's trend line, and shows the
  median of the samples in the seven days before the moment the commit reached
  `staging` and in the seven days after it, with the number of samples on each
  side.
- A sample is dated by when its unit began: a ticket by its start time, a push
  or a CI run by its start, a request by its time, a per-day figure by its day.
  A ticket that started before the change and closed after it counts as before.
- With fewer than five samples on either side, the entry is shown as "too few
  samples" and no direction is given.
- A change whose metric did not move in the expected direction is listed as
  such. This is information, not a failure.
- A test validates the file: each `commit` given is on `staging`, each metric is
  known and each label is one sentence.

The list is seeded with the optimisations already made in the project's first
week, found from the commit history.

## Findings

A finding is a named rule that points at waste and estimates what removing it
would save. Findings give an agent a place to start, and they keep each reader
from repeating the same analysis.

Every finding has the same shape:

```json
{
  "rule": "carried-logs",
  "unit": "dollars",
  "summary": "12805 tool results over 2,000 tokens cost $1,007.40 a week to read again; cut to 2,000 tokens they would save $573.06, most from read and search",
  "saving_per_week": { "dollars": 573.06 },
  "cost_per_week": 1007.4,
  "reference": "the same tool results at 2,000 tokens each",
  "evidence": "efficiency findings --rule carried-logs --since 7d",
  "see": "efficiency dollars --by source --since 7d",
  "examples": ["by-q44s", "by-1pi", "by-w8gv"]
}
```

- `saving_per_week` is the cost over the last seven days above the rule's
  **reference**, the level the cost would fall to if the waste were removed. It
  is in one unit: dollars, agent hours or core hours.
- `evidence` is the command that prints the numbers behind it, and `see` the
  wider query it draws on.
- Findings are listed in three groups, one per unit, each sorted by saving. No
  unit is converted into another.
- Findings never file tickets. An agent is told to work one.

The rules, each with its unit and its reference:

- **wake-ups** (dollars): the cost of wake-up requests. Reference: zero.
- **fixed-prefix** (dollars): the cost of the fixed prefix, by role. Reference:
  the same requests with a prefix at the 10th percentile of that role's sessions
  in the range.
- **carried-logs** (dollars): the carrying cost of tool results over 2,000
  tokens, by command kind. Reference: the same results at 2,000 tokens each.
- **small-reads** (dollars): the cost of requests in runs of three or more
  consecutive requests whose only tool calls are of kind "read and search".
  Reference: the cost of the last request of each run.
- **tier-gap** (dollars): for two cohorts that differ only in tier, the direct
  cost plus rework of the dearer one. Reference: the same number of tickets at
  the cheaper cohort's direct cost plus rework. Reported only when each cohort
  has at least twenty tickets.
- **outlier-beads** (dollars): tickets costing more than five times their
  cohort's median. Reference: the cohort's median for each.
- **queued** (agent hours): time queued for cores, by command kind. Reference:
  zero.
- **compile-share** (agent hours): compile time inside tests and checks.
  Reference: the same commands at the 10th percentile compile time of commands
  of the same kind in the range.
- **repeated-command** (agent hours): time in a command kind in tickets that ran
  it more often than the 90th percentile of their cohort. Reference: the same
  tickets at the cohort's median count, at each ticket's own mean duration for
  that command.
- **waiting-on-person** (agent hours, although outside agent minutes): time
  spent waiting on a person. Reference: zero.
- **chained-commands** (agent hours): time in commands flagged as chained. This
  rule reports a measurement gap, not waste, and its saving is shown as unknown.
- **narrow-leases** (core hours): for leases that got fewer cores than they
  wanted, the cores missing multiplied by the seconds held. Reference: zero.
- **never-caught** (core hours): CPU spent on check families with no catch after
  at least 100 runs. Reference: zero.
- **flake-cost** (core hours): the reruns caused by flaky failures, by check.
  Reference: zero.
- **misses** (core hours): for each miss, the bisection and the slow-CI runs
  that failed because of it. Reference: zero.

How the rules are built:

- `just efficiency findings` lists the groups; `--rule <rule>` prints one rule's
  items (one per role, kind of command, check or failure) with their count,
  cost, reference and saving. A finding's `evidence` is that `--rule` command,
  and its `see` names the wider query it draws on. A rule whose saving is zero
  is left out.
- The range defaults to the last seven days. Over any other range every figure
  is divided by the range's length in weeks.
- A cohort for outlier-beads and repeated-command is the tickets with the same
  role and tier labels, and needs at least five closed tickets. Tier-gap
  compares tiers within one role by their mean cost per ticket.
- Compile-share counts only kinds the command kinds table marks heavy, because
  `cat` or `grep` over a build log prints cargo's `Finished ... in` lines too.
  Repeated-command compares only kinds that the cohort's median ticket runs at
  least once; otherwise a ticket that ran a rare command once would count all of
  it.
- Small-reads counts the read tools (Read, Grep, Glob) as well as shell reads
  and searches, since a run of either is the same waste.
- Never-caught looks at slow CI, the fast check, the sweep and the nightly by
  family, and counts a `mixed` verdict as a catch. Misses counts the slow-CI
  runs with a failure on any commit in the Steward ticket's range.

## The store

The store is one SQLite file in the Beryl cache directory. It is private to the
machine and is never committed.

The tables, each keyed so that ingesting the same bytes twice changes nothing.
Every table with a `host` column also has a `provider`, except those from the
run log, the CPU sampler and git, which are the machine's own (Ignite's records
fill those tables now).

- **requests**: one row per model request, with its transcript, time, model, the
  four token classes (cache writes also by the 1-hour part), dollars in total
  and by class, context size, prefix size, new content size, stop reason and the
  ticket.
- **tool_calls** and **tool_results**: one row per tool call, with start and end
  times, the tool, the command kind, the first 300 characters of the command,
  the size of its result, whether it was chained, its compile seconds and the
  steps `enact push` left to slow CI; and one per result, with its size and
  compile seconds.
- **prompts**, **notifications** and **enact_events**: one row per prompt with
  its origin (a person, another session, a background command or a scheduled
  wake-up), per notification that a background command finished, and per Enact
  command a session ran. Windows and the time split are derived from them.
- **sessions** and **transcripts**: the desktop app's session records (title,
  model, the session it was spawned from) and each transcript's span, parent and
  session.
- **windows**: one row per ticket window, or per span of a session with no
  ticket (kind `none`, or `orchestration`), with its `label`.
- **window_time** and **window_dollars**: the split of each window's seconds by
  category, kind and part, and of its dollars by class, source, purpose and
  model. Every query adds these up, so they are derived again when the command
  kinds or the price table change.
- **tickets** and **labels**: the latest state of each ticket or ticket, with
  its Enact metadata and labels. **ticket_aliases** resolves the native and
  prefixed spellings, preserving the existing identifier for migrated tickets
  and their historical records. Either spelling works in `bead` and `epic`
  queries and the `--epic` filter.
- **ticket_session_events** and **ticket_ci_events**: session boundaries and
  slow-CI results from Enact. Transcript commands supply session boundaries when
  available, and events supply missing boundaries and CI results.
- **commits**: each commit on `staging` and `master`, its parents, subject,
  changed paths and added paths. The ticket that landed it is read from the
  tickets' `enact_commits` at query time.
- **ref_moves**: each move of `master` from its reflog, which is the only record
  of when a commit reached `master`, since Enact fast-forwards it. Rows are kept
  after git expires the reflog.
- **checks**, **lane_runs** and **verdicts**: the run log added up, each run of
  a check lane, and the verdict on each failure, as described under
  [CI accounting](#ci-accounting).
- **cpu**, **cpu_minutes** and **leases**: the sampler's rows by minute, owner,
  kind and worktree; its rows by minute alone; and one row per CPU lease, joined
  to the tool call that waited for it.
- **lock_waits** (history; nothing writes it since the switch to Ignite, and no
  query shows it apart from the time split): one row per acquisition of Enact's
  ownership lock or wait for the server's staging lease, joined to the tool call
  that ran the command.
- **codex_marks**: the lines of a Codex log that give a request its model and a
  turn its end.
- **sources**, **ingests** and **settings**: for each source file, the byte
  offset already ingested and a fingerprint of its first bytes; each ingest's
  wall and CPU seconds, bytes and rows; and the command kinds and price tables
  each derived row was computed with, which say when it must be computed again.

Ingest behaviour:

- Each source file is read from its recorded offset. A file whose first bytes no
  longer match its fingerprint was replaced, and is read again from the start.
- A last line with no newline is left for the next ingest.
- Only one ingest runs at a time, under a lock file. A second one waits for the
  first and then finds nothing to do.
- Ingest runs at background priority and takes no CPU tokens.
- A query runs an ingest first when the store is more than 60 seconds old. The
  flag `--no-ingest` skips that, and the page shows the age of its data.
- A malformed line is skipped and counted. `coverage` reports the count per
  source. Ingest never stops for one bad line.
- `just efficiency ingest --rebuild` recreates the store from the sources. It
  warns first that transcripts older than the providers' retention are gone and
  that their rows will be lost.

Privacy:

- The store holds commands and file paths. It holds no message text, no file
  contents and no tool output beyond its size.
- The repository is public, so tests use synthetic transcripts and tickets. No
  real session data is ever committed.

## The command

`just efficiency <query>` prints a compact text table. `--json` prints the query
result as JSON, which is exactly what the page renders. All queries take
`--since` (for example `24h`, `7d`, `all`, or a date) and cohort filters
(`--where key=value` for the label `key:value`, `--epic`, `--role`, `--tier`,
`--host`, `--provider`). Roll-ups default to the last 7 days and drill-downs
(`epic`, `bead`, `session`) to everything. A window counts when it starts in the
range, and a ticket when it closed in it; a closed ticket's per-bead figures
count all of its windows.

```text
just efficiency summary --since 24h
just efficiency epic by-bajq
just efficiency ticket by-bajq.6.9
just efficiency session local_ce2b4aa8
just efficiency time --by kind --where tier=sonnet
just efficiency dollars --by source --epic by-bajq
just efficiency cards --by tier
just efficiency machine --since 7d
just efficiency ci --lane oracle
just efficiency pushes --since 24h
just efficiency failures --since 7d
just efficiency changes
just efficiency findings
just efficiency findings --rule small-reads
just efficiency metrics request.context-tokens
just efficiency coverage
```

- `summary` is the first screen: tickets closed, the four per-bead medians with
  their trends, the time split, the dollar split and the epics.
- `epic`, `bead` and `session` are the drill-down levels. `session` lists each
  request and tool call in order.
- `time` takes `--by` with any of: `kind`, `model`, `tier`, `role`, `lane`,
  `epic`, `day`. `dollars` takes those and `class`, `source` and `purpose`.
- `machine` shows cores in use, saturation, queued core hours and shared CPU.
- `ci` shows the tree described under [CI accounting](#ci-accounting).
- `pushes` shows [the fast check](#the-fast-check) per push, and `failures`
  shows [what each slow-CI failure costs](#what-a-slow-ci-failure-costs).
- `coverage` reports what is missing or unknown: hosts with no data, models with
  no price, unmatched commands, skipped lines, closes with no open window, and
  the tool's own cost.
- `metrics` lists the metric names, and `metrics <name>` prints one series by
  day. `changes` lists the efficiency changes with the medians around each.
  `findings` ranks waste, and `findings --rule <rule>` prints one rule's items.
- `sql '<query>'` runs a read-only SQL query against the store, for questions no
  named query answers.
- `serve` starts the page. `ingest` updates the store. `export` and `import`
  move rows between machines.

Text output is written for a model to read as well as a person. This is the
`bead` query, on 2026-11-16:

```text
$ just efficiency ticket by-bajq.6.9
by-bajq.6.9  Spell copies are never scripted onto the stack, so script checks fail for Fork and Brain Freeze
closed 2026-11-15 07:38   role:executor   tier:opus   claude on jarvis
agent min 34.0   core min -   dollars 3.82   rework 0.00
requests 78   median context 145990   fixed prefix 79696   wall min 34.0
beside    waiting on a person 0.0 | idle 0.0
lead min  queued 3.0 | worked 34.6 | to master 72.0   commits 2   cards 0

time      background: fast check 9.1 | background: push 8.9 | model 5.8 | read and search 3.6
          enact 2.2 | rust tests 1.6 | fast check 0.8 | the rest 1.9
dollars   by class: cache_read 2.24 | cache_write 1.07 | output 0.52
          by source: fixed prefix 1.95 | carried: read and search 0.77 | output 0.52
          carried: model output 0.23 | carried: enact 0.13 | carried: other 0.05 | carried: git 0.04
          the rest 0.13
          by purpose: working 3.82
          by model: claude-opus-5-5 3.82

push 2026-11-15T07:17  536 s, not split (no run-log lines)

session                                     title                                         kind  start                     minutes
local_ce2b4aa8-b291-4649-9a86-f3862695d15f  ✅ [by-bajq.9.5] Review the decision protocol  bead  2026-11-15T07:04:31.826Z    34.00

more: efficiency session local_ce2b4aa8-b291-4649-9a86-f3862695d15f --ticket by-bajq.6.9
```

Every result ends with a `more:` line naming the queries that drill into it. The
JSON carries the same list, and the page turns each into a section that can be
opened. The JSON also carries `fields`, a description of each field that the
page shows on hover. Every number in the text is a number in the JSON, already
rounded; a sum that includes a model with no price is marked `+`, and an unknown
value prints as `-`.

## The page

`just efficiency serve` starts a server that listens on `127.0.0.1` only and
serves one page. Python renders all HTML and draws the charts as inline SVG.

- The server answers `GET` requests only and changes nothing except the store,
  through ingest.
- Each address is a query: `/bead/by-bajq.6.9?since=7d` renders the same result
  as `just efficiency ticket by-bajq.6.9 --since 7d`.
- The same address with `.json` appended returns the JSON.
- The page needs no build step and no JavaScript beyond one small script that
  fetches a section's content the first time it is opened.

What the first screen shows, top to bottom:

- A time range control with three choices: 24 hours, 7 days, all. It applies to
  everything on the page.
- Tickets closed in the range.
- Four numbers, each a per-bead median with a small trend line under it: agent
  minutes, core minutes, dollars, and rework as a share of direct cost.
- One horizontal bar showing where agent time went, and one showing where
  dollars went, by source. Segments are labelled only when wide enough. The rest
  are named on hover.
- The epics active in the range, one line each: id, title, tickets closed out of
  total, dollars, agent hours. "(no ticket)" and "orchestration" are lines too.
- Six words that lead to the other views: Cards, Machine, Enact, CI, Changes,
  Findings.

Rules that keep the page quiet:

- A number is shown without a sentence around it. Its column or row heading is
  the only label.
- Nothing is explained on screen. Each heading has a definition that appears on
  hover, taken from the query's own description.
- Every line that has more detail opens in place when clicked. Opening an epic
  shows its time bar, its dollar bar and its tickets. Opening a ticket shows its
  bars, a strip of its timeline and its sessions. Opening a session shows its
  requests and commands.
- A trend line that has an efficiency change on it shows a mark at that commit.
  The mark's label and the before and after medians appear on hover.
- Each opened section has one small control that copies the equivalent command,
  so the reader can hand the same view to an agent.
- Unknown values are shown as a dash, never as zero.
- The page follows the system's light or dark appearance and uses one accent
  colour per category, the same colour for a category everywhere.

The ticket view's timeline strip is one bar, as wide as the ticket's wall-clock
time, coloured by category from left to right. Under it, a line shows the
context size at each request. Together they show at a glance a session that sat
in a long test run, or one whose context climbed to the limit and was compacted.

## Ticket pages on the hub

Each ticket's page on Ignite's dashboard has an efficiency section, between its
comments and its changelog, that shows the same accounting for that ticket
without opening this page.

- `just efficiency spend --since <date> --out <file>` writes each ticket window
  of this host that ended since the date as one JSON line: the window's ticket,
  session and times, and two lists in time order. `time` holds the window's
  seconds split as in [Time accounting](#time-accounting), each run of one
  category, kind and part as one segment; a tool call's queued seconds come
  first, then its compile seconds. `dollars` holds the window's requests, each
  under the kind of the tool calls it made, or `reply` when it made none.
- A list longer than 120 segments is coalesced: runs are gathered into buckets
  of at least 1/120 of the window, each named after its largest part, with its
  three largest parts kept for the hovercard. Waits on a person and idle
  stretches are never merged, since the page draws them as breaks.
- Each segment names a colour group (model, checks, tools, Ignite, background,
  agents, queued, and the breaks person and idle), so the hub colours segments
  without knowing this project's command kinds.
- A segment names a command only when it is tool time and the command is of its
  own kind, so a segment coalesced from thinking and a short `grep` never shows
  the grep. When the time accounting, the command kinds table or the segments'
  format changes, every window is written again, whatever `--since` says, so the
  hub's copy follows.
- Ignite's daemon runs the command after each export and posts its lines to the
  hub, which keeps the latest post of each window. The page draws the time strip
  and the dollar strip side by side, with a timeline view, a view by kind and,
  for an epic, a merged view of its whole subtree. Segments carry the command
  that took most of their time; the hub shows it only on its private listener.

## Other machines

Tickets are also worked on a second Mac and on a Windows PC. The store on each
machine holds only what ran there.

- Every row carries its host. A query over tickets that ran elsewhere reports
  them as "no data" and says how many, so no total is silently short.
- `just efficiency export --since <date>` writes the store's rows for this host
  to one compressed file. `just efficiency import <file>` on another machine
  adds them. Rows are keyed by host and source, so importing a file twice
  changes nothing.
- An export contains commands and paths, as the store does, and is passed
  between the user's own machines. It is never committed or published.
- Ingest finds the providers' directories from the user's home directory on each
  platform and never assumes a path separator.

## Isolation

The dashboard must be changeable without risk to anything else, and changes to
anything else must not run its checks.

- Its code, its tests and its page live in one directory that nothing else
  imports. It reads other tools' files by their documented formats and does not
  import their modules.
- It has one check lane of its own. The lane runs only when a file in that
  directory, or one of its three data files, changes. A change confined to those
  files runs no other check lane except the repository-wide formatting checks.
- The lane's steps are formatting, type-checking and unit tests. It has no
  browser step, because headless browser slots are scarce and shared.
- The two additions to other tools, the `worktree` field in the run log and the
  lease line in the token pool, are separate small changes in those tools, each
  with its own test there.

What the lane's tests cover, all against small synthetic sources:

- **Ingest**: windows are found, parallel tool calls are divided, repeated
  message lines are counted once, and a second ingest reads zero bytes.
- **Sums**: for every ticket in the fixture, the six time categories sum to the
  window length, the three context parts sum to each request's context, and each
  of the three dollar splits sums to the ticket's dollars.
- **Parity**: for every query, each number in the rendered HTML and in the text
  output equals the number in the JSON.
- **Pages**: every page and every section renders as well-formed HTML for the
  fixture, including the empty store.
- **Data files**: the price table, the command kinds and the efficiency changes
  are valid.

For looking at the page while developing it, `just efficiency shot <address>`
renders one address to a PNG with a headless browser. It is a development aid
and is not part of any check.

## Alternatives considered

- **Hooks inside each agent session** that report tokens and timings as they
  happen. Rejected: they add work to every session, they need changes to every
  provider's configuration, and the transcripts already hold the same facts.
- **Wrapping commands in a timer.** Rejected for the same reason. The one place
  where a command reports something itself is the CPU lease, which only the pool
  knows.
- **A route inside the game's own web UI.** Rejected: it would put developer
  tooling into the game's bundle, its check lanes and its screenshots.
- **A static report file.** Rejected: detail below ticket level would have to be
  left out or embedded in full, and the report would be stale when opened.
- **Raw token counts instead of dollars.** Rejected as the main unit: a raw sum
  is 98.7% cache reads and cannot show a move from a large model to a small one.
  Raw counts remain available by class.
- **Filing tickets automatically from findings.** Rejected for now by decision:
  the rules will be imprecise at first, and weak tickets would cost tokens to
  work.

## Manual QA

Start with one check that crosses every boundary, before testing anything
broadly.

- On the main development Mac, run `just efficiency ingest`, then
  `just efficiency ticket by-bajq.6.9`. The ticket is closed, ran in a Claude
  session on this machine, and landed two commits. Expect non-zero agent
  minutes, dollars and requests, a time split whose parts sum to the agent
  minutes, and a `more:` line.
- Run `just efficiency serve` and open the address it prints. Find by-bajq, open
  it, open by-bajq.6.9. The four numbers must equal the command's output.

Then check each area.

Totals and attribution:

- `just efficiency summary --since all` followed by `just efficiency coverage`.
  The dollars of all epics, "(no ticket)" and "orchestration" must sum to the
  total. Coverage must list the second Mac and the Windows PC as hosts with no
  data, with a count of their tickets.
- Pick a session that worked several tickets in a lane. `efficiency session`
  must show one window per ticket with no overlap, each opening where the one
  before it closed.
- Pick an orchestrator session. Its cost must appear under its epic's
  "orchestration" line and under no ticket.
- Run `just efficiency sql 'select count(*) from requests'`, run
  `just efficiency ingest` again, and repeat. The count must not change.

Time:

- For a ticket that pushed, the `bead` query's push line must split the push
  into the fast check and the rest. The fast check's seconds must match
  `just ci-report` for that worktree and time.
- For a ticket that ran Rust tests after an edit, the tests must show a compile
  part and a run part.
- Start a heavy command while the machine is saturated, then find it with
  `efficiency session`. It must show seconds queued for cores, and
  `efficiency machine` must count them.
- Find a session that asked the user a question. The wait must appear as waiting
  on a person and must not be part of its agent minutes.

Tokens:

- For one transcript, sum the four token classes by hand from its `usage`
  objects, counting each message id once. `efficiency session` must match.
- Remove a model from the price table. `coverage` must name it, and sums that
  include it must be marked as lower bounds. Restore it.
- Find a session whose context was compacted. Its context line on the page must
  drop, and carrying costs must stop at the drop.
- For one ticket, `efficiency dollars --by class`, `--by source` and
  `--by purpose` must each sum to the ticket's dollars.

CI:

- `just efficiency ci --since 7d` must report the same number of slow-CI runs
  and the same total CPU seconds as `just ci-value --days 7`, and the same
  catches per check lane.
- Open the oracle check lane. Forge and XMage must appear separately, and the
  "oracle" line of CPU by kind of process must be close to their sum.
- Find a check with a `flaky` verdict in `just ci-report`. It must appear under
  flake cost with its reruns.
- Find a closed Steward ticket. Its cost must appear under "what a slow-CI
  failure costs" and as rework on at least one ticket in its commit range.

Changes and findings:

- `just efficiency changes` must list each seeded change with a median before
  and after and a sample count. The page must show a mark on the matching trend.
- Add an entry with an unknown metric name. The check lane must fail. Remove it.
- `just efficiency findings` must print three ranked groups. Run the `evidence`
  command of the top finding in each and confirm it prints the numbers the
  finding cites.

The page:

- With the range set to 24 hours, 7 days and all, every number must change
  together and no section may keep an old range.
- Open and close sections several levels deep. Nothing may move except the
  section opened. Use the copy control on a section, paste the command into a
  terminal and compare the output with the section.
- Stop the server, delete the store and start it again. The page must render an
  empty state, start an ingest and fill in, without an error.
- View the page in light and dark appearance and at a narrow window width. Bars
  and numbers must stay readable and nothing may overlap.

Isolation and cost:

- Make a change only inside the dashboard's directory and run `just check`. Only
  its own check lane and the repository-wide formatting checks may run.
- Make a change only in the Rust workspace and run `just check`. The dashboard's
  check lane must not run.
- After a day of normal work, `just efficiency coverage` must show the sampler
  and all ingests together under 1% of the machine's CPU for the day.
