---
name: ignite
description: Start work through Ignite. `/ignite <ticket or small task>` does it in this session; `/ignite <plan or large task>` settles the design and files an epic of tickets for the worker pool; `/ignite sweep` triages stale open tickets; `/ignite pool <n>` adds worker sessions; `/ignite dispatch` starts this machine's dispatcher. Use when the user invokes /ignite or asks to plan, file, sweep or staff work for Ignite's pool.
---

# Ignite

Ignite is the build, check and delivery system (`docs/ignite.md`). Workers are
desktop-app sessions that the dispatcher hands tickets to; nothing here runs a
loop. Run `ignite prime` first: it holds the workflow every ticket follows
(start, test, land, release), and this skill does not repeat it.

Pick the mode from the argument:

| Argument                               | Mode                         |
| -------------------------------------- | ---------------------------- |
| a ticket ID, or a task of a few steps  | [Do it here](#do-it-here)    |
| a plan file, or a task of many tickets | [Plan](#plan)                |
| `sweep [older than N days] [dry]`      | [Sweep](#sweep)              |
| `pool <n>`                             | [Pool](#pool)                |
| `dispatch`                             | [Dispatcher](#dispatcher)    |

Titles, for every session: `⚒️` while working, `🔍` while waiting on the
person's answer (only when a person is there), `✅` done, `⛔` stopped
unfinished. Every final turn ends on one of the last three.

## Do it here

`ignite start <ticket>`, or `ignite start "<summary>"` for a task with no
ticket, then the work, then `ignite land -m "<note>"`, following what each
command prints and `ignite prime`.

## Plan

Settle the design, file the whole graph in one `ignite ticket batch`, check it,
and stop. The pool takes the tickets; this session never works them or waits on
them.

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
epic. A point that cannot be resolved is cut from scope or stops the plan. Never
file a ticket that needs a person later.

When the task is a sentence or two, ask a few clarifying questions at the
start, because the person may not be there later.

### The graph

- **One epic** holds the summary, a link to the plan, the decisions,
  whole-plan acceptance and a short code map (modules, entry points, test
  commands). No ticket's text states a number of workers.
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
- **Every ticket gets a tier label**: `tier:opus` for design and anything that
  races or caches, `tier:sonnet` for ports and declarations, `tier:light` for
  mechanical work. Never `tier:fable` unless the person asked for it. No tier
  or lane label on an epic, because children inherit an epic's labels.
- **`lane:<name>` only for a true chain**, where each ticket needs the previous
  one's result. Tickets that merely touch the same code stay apart; the
  dispatcher already prefers the worker that last touched those files.
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

### Filing

1. Write the batch file in the scratchpad: `{"ops": [...]}`, one `create` per
   ticket, epic first. Later operations name earlier ones as `$<index>`:

   ```json
   {"ops": [
     {"op": "create", "fields": {"title": "Replay import", "type": "epic",
       "body": "...", "acceptance": "...", "workers": 1}},
     {"op": "create", "parent": "$0", "fields": {"title": "Replay types",
       "body": "...", "acceptance": "...", "labels": ["tier:opus"]}},
     {"op": "create", "parent": "$0", "deps": [{"on": "$1"}],
       "fields": {"title": "Read replays", "labels": ["tier:sonnet"]}}
   ]}
   ```

2. `ignite ticket batch <file>`. It succeeds or fails as a whole.
3. `ignite plan check <epic>`. It only reports: the longest chain, edges between
   tickets that name no file in common, tickets that name no files, lanes whose
   tickets do not depend on each other. Fix what is wrong with
   `ignite ticket update` and `ignite ticket dep`, and run it again.
4. Set the epic's share: `ignite ticket update <epic> --workers <n>`, with the
   number the person gave, or 1. An epic with no number is never worked, so
   this step is what starts the pool on it. Add `--max-workers` only for an
   epic whose tickets conflict when too many run at once.
5. Give a short summary: the epic, the ticket count, the milestones and what
   would stop it. Do not wait for approval.

## Sweep

`/ignite sweep [older than N days] [dry]` triages the open tickets that nothing
will work.

- A ticket is stale when it is open and unclaimed, under an inactive epic (one
  with no `--workers`) or under none. Age alone does not make it stale; `older
  than N days` only narrows the list.
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
  and label the unresolved ones `needs:person`. Then activate what is kept:
  give its epic a `--workers` share, or file it under an active epic.
- `dry` shows the table and closes, comments on and activates nothing.

## Pool

`/ignite pool <n>` adds `n` workers to this machine's pool. Run it only in the
dispatcher's own session, in a turn a person started: the desktop app lets only
such a turn start sessions, and its tools for clearing and prompting a session
only work on sessions the caller started.

1. Read this session's permission mode (`get_session` on `self`). Refuse unless
   it never prompts (`bypassPermissions` or `auto`): workers inherit it, and a
   prompt stalls a worker that nobody watches.
2. Start `n` sessions, at most ten in this message, one `start_session` call
   each: `initiation: "user_asked"`, `context: "fresh"`, `use_worktree: false`,
   `cwd` the project's main checkout by its full path, title
   `✅ ignite worker <k>`, prompt "Ignite pool worker: reply only `ready`."
3. Register them in one call: `ignite pool add <id>... --app claude`.
4. Say how many joined. The limit of ten is per message, not a lifetime cap:
   the person adds more by sending `/ignite pool <n>` again.

In Codex, a worker is a new chat per ticket, which the dispatcher opens at each
hand-off, so the pool needs no standing chats.

## Dispatcher

`/ignite dispatch` starts this machine's dispatcher: one small session per
machine and per app, whose whole job is the tool calls `ignite dispatch` asks
for. Run it in a turn a person started.

1. Make its folder outside the repository, `~/.ignite/dispatcher/claude/`, so
   its context holds none of the project's instruction files. Put in it:
   - `CLAUDE.md`, a copy of [`dispatcher.md`](dispatcher.md) beside this skill;
   - `.claude/settings.json`: `{"autoCompactWindow": 120000}`;
   - `.claude/skills/ignite`, a symbolic link to this skill's folder, so that
     `/ignite pool` works there.
2. Start it with `start_session`: `initiation: "user_asked"`,
   `context: "fresh"`, `cwd` the folder's full path, model Sonnet, title
   `🔮 dispatcher <machine>`, prompt "Follow CLAUDE.md." Then read it back with
   `get_session` and set the model with `set_session_model` if it is not Sonnet.
3. Register it: `ignite pool add <id> --app claude --dispatcher`.
4. Tell the person to open the dispatcher session, check that its permission
   mode never prompts, and send `/ignite pool <n>` there.

The Codex equivalent: the folder `~/.ignite/dispatcher/codex/` with
`AGENTS.md`, a copy of `dispatcher.md`, and the skill linked at
`.agents/skills/ignite`. Codex has no `autoCompactWindow`; the daemon tells a
Codex dispatcher to open a new chat as the dispatcher once its context passes
the same size. Register the folder as a Codex project by its full path, open a
chat there with `create_thread` on the app's small, fast model, and register its ready
thread ID with `ignite pool add <id> --app codex --dispatcher`.
