---
name: cifix
description: Beryl's guide to working a ticket about a check — a regression that tip verification found on staging, a flaky or hung check, or a check that costs much and never catches anything. Adds what is Beryl's to `ignite guide regression` and `ignite guide flaky-checks`: where to reproduce, what a fix must add, the order of remedies, protected checks and what must still cover a deleted interface check. Use when working such a ticket, or when asked to fix a failing, flaky or costly check, or to run /cifix.
---

# Fixing a check or what it caught

Ignite files tickets about checks by itself: a regression ticket when tip
verification finds a check failing on `staging`, naming the commit or range it
blames, and an improvement ticket for a check that flakes, hangs or costs much
and never catches anything. `ignite start` on such a ticket names the guide to
read first. `ignite guide regression` and `ignite guide flaky-checks` hold the
general rules, and `ignite why <check>` prints a check's record: its runs,
costs, failures and where it runs. This skill adds only what is Beryl's.

Each check family is declared in `ignite/checks/*.toml`, with the reason for
each setting in a comment beside it. Read the family's declaration before
changing how its checks run.

## A failure: a regression ticket

The check really failed on `staging`, so something broke. `staging` is not a
branch: it is the ref `refs/ignite/staging` on the remote, fetched as
`origin/ignite/staging`, so its tip is `git rev-parse origin/ignite/staging`
after `git fetch origin refs/ignite/staging:refs/remotes/origin/ignite/staging`.
A fix lands with `ignite land`, never by a push to `refs/ignite/staging`.

1. **Reproduce** it with `ignite test <check>` in your tree, at the culprit
   commit the ticket names and at the staging tip. Read the ticket that landed
   the culprit commit.
2. **Find the cause.** When the product broke, fix the product: a rules engine
   bug also gets an entry in `docs/bugs.md` (root `AGENTS.md`, Conventions), and
   a UI break adds the broken state as a fixture or spec moment (root
   `AGENTS.md`, Checks). When the fix changes only the test, delete it or
   redesign it to prevent the same invalid failure, using "A check to change"
   below. Adjusting the assertion alone leaves the cause in place. Record the
   remedy on the ticket. Card and rules definitions and the oracle scenarios
   they produce count as product code. Fix a failure caused by a machine's
   state, such as a full disk or a stale cache, in `tools/` or in the family's
   declaration so it cannot recur.
3. **Verify** that `ignite test <check>` passes at the staging tip, using
   the replacement's name when you replaced the check. If you deleted it,
   verify that its family no longer lists it.

## A check to change: flaky, hung or costly

The check passes but costs too much: a flake, a hang, or instructions spent for
nothing caught.

1. **Find the check.** When the ticket names a whole step that reported no
   checks of its own, find which test or command inside it failed from the
   ticket's output and `ignite why`.
2. **For a flake, find the cause.** Suspect, in this order: a race in the
   product or in one feature's test helper (several flaky checks in one feature
   usually share it); for a step that has no reason to be flaky, such as a
   formatter, a lint, `file-length` or pytest on pure functions, the
   environment (a timeout under load, files another process changes during the
   run, a temporary path or cache shared between trees, a download); then the
   check alone (a wall-clock threshold, a sleep, a first animation frame, a hash
   map's order, two processes writing one file, free disk, a port, an app process). Try to reproduce it under load: run it 10 to 20 times beside a full
   `just test-rust` or `just b3d test`. Load of 20 to 40 is this machine's normal
   state, so a check has to pass under it.
3. **Choose one remedy, in this order,** and record which and why on the
   ticket:
   - **Fix the product** when the flake is a race in Beryl itself, such as a
     view that arrives before the state it shows. Players hit it too.
   - **Fix the environment** when a step that should be deterministic flaked
     because of a shared file, a timeout, the disk or the network. Keep the
     step; never delete a lint.
   - **Replace it with a cheaper deterministic test** that keeps most of what
     the check protects. Use a Rust test, a recorded fixture or a story with
     a controlled clock. Count work instead of timing it.
     Keep a timeout only to catch a hang, at about ten times the loaded
     duration. Then delete the old check.
   - **Delete it** when it has never caught a bug.

   Loosening a threshold, adding a retry and raising a limit are not remedies.
4. **Never cut a protected check**: the families and checks listed under
   `protected` in `ignite.toml` (native app startup, Clippy, Pyre, the
   source rules and the oracle families).
5. **An interface check** follows `rust/beryl3d/AGENTS.md`. When you delete
   one, verify that the recording or story still exercises the behavior and
   that the generic interface rules cover it.
6. **Verify.** Run a replaced or changed check 10 times under the same load as
   step 2 and record the passes on the ticket. A deleted check needs no runs.

Then land with `ignite land -m "<cause and remedy>"`. When one cause settles
several tickets, land once and close the others with
`ignite close -m "<cause>, landed in <commit>"`.
