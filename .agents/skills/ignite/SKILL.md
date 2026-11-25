---
name: ignite
description: Start work through Ignite. Use /ignite with a ticket or small task to work it here, or with a plan or large task to design and file an epic for the worker pool. Use /ignite sweep to triage stale open tickets. Also use when the user asks to plan, file or sweep work for Ignite's pool or to find its fleet settings.
---

# Ignite

Ignite is the build, check and delivery system (`docs/ignite.md`). The daemon
runs workers as headless CLI processes and starts them as capacity allows. The
dashboard's Fleet page and `ignite fleet` show the fleet's machines, capacity
and provider mix. The operator, incident commander or justiciar can change
machine and provider settings there. Run `ignite prime` first for the ticket
workflow.

Pick the mode from the argument:

| Argument                               | Mode                         |
| -------------------------------------- | ---------------------------- |
| a ticket ID, or a task of a few steps  | [Do it here](#do-it-here)    |
| a plan file, or a task of many tickets | [Plan](#plan)                |
| `sweep [older than N days] [dry]`      | [Sweep](#sweep)              |

Titles, for every session except a headless CLI worker (session ID starting with
`cli-`), which has none: `⚒️` while working (`🚒` while working a regression
ticket), `🔍` while waiting on the person's answer (only when a person is there),
`✅` done, `⛔` stopped unfinished. Every final turn ends on one of the last
three. A turn that ends to wait on a running command (an ignite verb or a
background process) keeps `⚒️`, and is woken when the command ends.

## Do it here

`ignite start <ticket>`, or `ignite start "<summary>"` for a task with no
ticket, then the work, then `ignite land -m "<note>"`, following what each
command prints and `ignite prime`.

## Plan

Settle the design, file the whole graph in one `ignite ticket batch`, check it,
and stop. The operator sets the epic's Allocation when the pool should start.

### Before filing

The plan must run with no person. List every point that would stop unattended
work:

- a decision left to the person;
- a manual step: a sign-off, QA or a UI review done by a person;
- credentials, accounts, payments or access that agents lack;
- outside parties;
- anything irreversible or outward-facing that is not yet authorized;
- a machine, operating system or device that no pool serves;
- waiting for time or data to pass;
- a bar of perfection on a handful of samples, which agents tune to and never
  reach. Use held-out inputs, a pass rate, and a fallback after a fixed number
  of rounds instead.

Ask the person only about what an agent cannot do or may not decide, one
question at a time, each with your recommendation. Settle everything else
yourself: take the least-blocking default and write it, with its reason, in the
epic. Write the design and the contract between the tickets into the epic too,
and never file a ticket whose work is to figure out the design. A point that
cannot be resolved is cut from scope or stops the plan. Never file a ticket
that needs a person later.

When the task is a sentence or two, ask a few clarifying questions at the
start, because the person may not be there later.

### The graph

- **One epic** holds the summary, a link to the plan, the decisions,
  whole-plan acceptance and a short code map (modules, entry points, test
  commands). No ticket's text states a number of workers. The epic does not
  close by itself: when its other tickets are done, the hub files a ticket that
  measures the acceptance, so write it as something a session can measure.
- **Milestones** are child epics when the plan has phases, each depending on the
  one before. A large or uncertain plan may file the first milestone's tickets
  and one planning ticket for each later milestone.
- **A contract ticket comes first** when several tickets build on shared types.
  It lands the types and stubs the others fill in.
- **Each ticket is one session's work.** Its body is a context packet: the plan
  excerpt, the files and functions to change, the helpers to reuse, the tests to
  extend and how to run them, and what need not be read. Size tickets by code,
  not by topic.
- **A dependency only where a ticket needs another's result**, placed on the
  ticket that needs it, never on its epic. A reported-only ticket (a benchmark,
  a measurement) gets no dependents. Two tickets that would rewrite the same
  code get an ordering edge.
- **Every ticket gets a tier label**: `tier:high` for design and anything that
  races or caches, `tier:light` for ports, declarations and mechanical work.
  These run on every provider (`[pool.models]` in `ignite.toml`). A tier named
  after a model, such as `tier:opus` or `tier:hydrogen`, runs only on that
  model; use one only when the work needs that model, as a model trial's arms
  do. Never `tier:fable` unless the person asked for it. No tier or lane label
  on an epic, because children inherit an epic's labels.
- **Pins only for hard requirements.** Add `machine:<name>` when work requires
  a specific machine and `provider:<name>` when it requires a specific coding
  agent CLI. Each pin applies to descendants that lack their own pin of that
  kind. Leave other tickets unpinned so any eligible worker can take them.
- **`lane:<name>` only for a true chain**, where each ticket needs the previous
  one's result. Sharing files is insufficient reason to add a lane.
- **Reviews**: a ticket labelled `review` per area for large or risky work
  (data loss, concurrency, security, many files), depending on what it reviews;
  the label `review:inline` for a change that deserves a second look; nothing
  for a routine change. Work that builds on reviewed code depends on the review.
- **Audits and docs**: a plan with milestones gets a ticket labelled `audit` at
  each boundary and one labelled `docs` at the end that depends on everything
  else, unless the person asks for less.

Existing tickets belong to whoever filed them. Before moving one, or adding an
edge to or from it, read it whole with its parent and dependents
(`ignite ticket show <id> --comments`, `ignite ticket children <parent>`), and
read the graph again just before the change. Ask the person before moving a
ticket out of another open epic, or before blocking a ticket that another epic
waits on. A plan once moved a ticket on a stale view and deadlocked an epic it
never meant to touch.

### Estimate the work

Give every work ticket an `estimate_minutes` in agent minutes. Include the time
to understand the code, make the change, verify it and fix likely issues. Use
Agent time from the Efficiency section of a comparable completed ticket when
available, then adjust for what differs. Use round numbers. A guess of 15, 45 or
120 minutes for three different tickets is more useful than giving them equal
weight. Epic and milestone totals come from their work tickets.

A review, spike or planning ticket gets its own estimate. If it will also define
later work, put a separate rough guess for that work in `unfiled_minutes`.
Estimate the likely number and size of follow-ups from what you know now. For
example, a 30-minute review expected to produce about ten 45-minute fixes
carries `estimate_minutes: 30` and `unfiled_minutes: 450`. Write the assumption
in the ticket so its worker can revise it. Include substantial final
verification as a sized ticket in the plan.

Tell the worker filing follow-ups to give each one an estimate and update the
reserve in the same batch. Use a version precondition on the ticket holding the
reserve. Set `unfiled_minutes` to the best guess for work still to be filed, or
to `null` once all expected work is accounted for. File the follow-ups before
closing the planning ticket. If the review finds no work, clear the reserve and
record why.

### Filing

1. Write the batch file in the scratchpad: `{"ops": [...]}`, one `create` per
   ticket, epic first. Later operations name earlier ones as `$<index>`:

   ```json
   {"ops": [
     {"op": "create", "fields": {"title": "Replay import", "type": "epic",
       "allocation": 0, "body": "...", "acceptance": "..."}},
     {"op": "create", "parent": "$0", "fields": {"title": "Replay types",
       "body": "...", "acceptance": "...", "estimate_minutes": 45,
       "labels": ["tier:high"]}},
     {"op": "create", "parent": "$0", "deps": [{"on": "$1"}],
       "fields": {"title": "Read replays", "estimate_minutes": 90,
       "labels": ["tier:light"]}}
   ]}
   ```

2. `ignite ticket batch <file>`. It succeeds or fails as a whole.
3. Run `ignite plan check <epic>`. Check the estimated effort and size any
   tickets listed under `needs estimate`. Fix the graph and file-reference
   findings with `ignite ticket update` and `ignite ticket dep`, and run it
   again.
4. Keep the epic parked at zero. The operator can set a positive weight in
   the epic's Allocation field or with `ignite ticket update <epic> --allocation <weight>`.
   Record any requested starting weight in the epic. Add `--max-workers` only
   for an epic whose tickets conflict when too many run at once.
5. Summarize the parked epic, milestones, estimated agent hours (including
   unfiled work) and anything that would stop it.

## Sweep

`/ignite sweep [older than N days] [dry]` triages the open tickets that nothing
will work.

- Review unclaimed, unfinished tickets that are parked by their allocation or
  pool, and unclaimed standalone tickets that may have been left behind.
  `older than N days` narrows the list to tickets of that age.
- Give each one a verdict with one line of evidence:
  - **done**: cite the commit or the code that did it;
  - **obsolete**: cite what replaced it;
  - **keep**;
  - **person**: it needs a decision or action only a person can take.
- To decide, read the ticket, its comments and its epic, search git history for
  its ID and the files it names (`git log --all --grep <id>`,
  `git log -- <file>`), and ask whether a strong engineer joining today would
  still do this work. A ticket whose siblings have all closed is usually done
  or obsolete.
- For more than about eight tickets, fan out read-only subagents on Sonnet,
  about five tickets each, sent together. They only read and propose verdicts;
  this session decides every close.
- Show the table before acting. Then close done and obsolete tickets with the
  evidence as the reason (`ignite ticket close <id> "<reason>"`). On a kept
  ticket, comment what in it is now wrong, such as renamed files or steps
  already done. Ask the person about each `person` ticket now, one at a time,
  and label the unresolved ones `needs:person`. For kept work that should run,
  clear a ticket's zero in the Allocation editor or move the ticket under a
  suitable active epic as needed. Leave new plans for the operator to activate.
- `dry` shows the table and closes, comments on and activates nothing.
