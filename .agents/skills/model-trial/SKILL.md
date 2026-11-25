---
name: model-trial
description: Set up, run and report a model trial with `just model-trial`, in which a worker on one model works copies of real tickets that the pool works on another, and the two results are compared blind. Use when asked to compare models or efforts on real work, to start a new trial, or when working a trial's block or report ticket.
---

# Model trials

A model trial measures how two ways of running a worker (the **arms**: a tier, a provider and
an effort each) compare on the same real tickets. The **source arm** is the real ticket: the
pool works and lands it as usual. The **copy arm** works a root ticket with the same text from
a frozen `staging` commit and ends on the branch `trial/<copy id>` without landing. A source and
its copy are a **pair**. `just model-trial grade` measures both arms, `review` has a blind
reviewer compare them, and `report` prints the table. A spec may name several copy arms: each
source then gets one copy per arm, all from the same commit, and each copy forms a pair of its
own with the source. `pair file` files the other arms' copies after the first arm's, by cloning
its text, and fills in any filed source that lacks one, so an arm added to a running trial
covers the blocks already run.

A trial is its epic. The epic's notes hold the **trial spec** under "## Trial spec", every
command takes `--trial <epic>`, and the records are JSON lines in
`$BERYL_CACHE/model-trial/<epic>/` on the machine the epic is pinned to. `tools/AGENTS.md`
describes the tool's files. The epic's children are block tickets, each filing the next pairs,
and a report ticket at the end.

## Setting up a trial

Running a trial is more than 30 minutes of work other than writing code, so the root
`AGENTS.md` requires the user's clear yes before anything is filed. Ask him, one question at a
time:

1. **The two arms.** For each, the `tier:` (a key of `[pool.models.<provider>]` in
   `ignite.toml`), the `provider:` and the `effort:`. Pin both to one provider: workers of
   other providers take tickets of every tier.
2. **The cap**, in dollars at list price, summed over the copies' sessions and the reviewer
   runs. The source's sessions are work the pool would do anyway and do not count. As a
   starting point, the first trial estimated a Sonnet copy at high effort at about $1.50 and
   two Opus reviewer runs at high effort at about $5 per pair. `block_reserve_usd` is what one
   more block may cost; a block files nothing when the spend plus this would pass the cap.
3. **The reviewer**: `model` (a key of `[pool.models.claude]`), `effort` and `runs_per_pair`
   (two, with A and B swapped, so that a rubric line counts only when both runs agree).
4. **The ticket pool**: which epics or labels to draw from, the block size, the most pairs,
   how many false premises to plant per block, and how many pairs he will judge blind.

Write the answers into a draft spec file (leave `tickets` empty) and run
`just model-trial candidates --spec <file> --epic <id>... --label <label>...`. It lists the
open tickets that could be the source arm, with each one's kind (engine, interface, tooling or
other) and whether it is ready, and the rest with the reason each was left out. Show him the
list. He picks the tickets and their order; the tool never picks. Leave out, even when
`candidates` allows them:

- a ticket whose main output is other tickets, such as a review, because the copy would file
  them twice;
- a lead whose worker closes its followers, because the copy would close real tickets;
- a ticket that waits on another ticket's results, or that needs a machine with no worker.

Order the list so that each block mixes kinds. Then finish the spec. Its fields are
`arms.source` and one or more copy arms, each under its own name such as `arms.copy` (each
`tier`, `provider`, `effort`; the first copy arm is the default one), `block_size`, `max_pairs`
(zero for the whole list), `cap_usd`, `block_reserve_usd`, `planted_per_block`, `reviewer`
(`model`, `effort`, `runs_per_pair`), `judge_pairs` and `tickets`. Missing optional fields get
defaults, which `tools/src/beryl_tools/model_trial/spec.py` holds.

Run `just model-trial new --title "<title>" --spec <file> --dry-run` and show him the epic,
block and report tickets it would file. After his clear yes, run it without `--dry-run`, then
run the `pair add` command it prints, which records the listed tickets and parks each one
(allocation zero) so that the pool does not work it before its block. `pair add` refuses a
ticket that cannot be an arm and says why; tell him which.

The trial epic is filed parked (allocation zero) and pending approval. It stays that way until
he opens it: say so, and give him the command
(`ignite ticket update <epic> --status open --allocation 1`). When a block files a pair whose
source sits under a parked epic, `pair file` opens that epic (after checking that its other
listed tickets are parked on their own) and records what it changed, so nothing on the list
starts before its block.

## Running a block

A block ticket names its trial (its parent epic) and its number. Read the trial spec in the
epic's notes first: the block size, cap, reserve and number to plant come from it. Run heavy
commands through `ignite run --` and wait on them. Never wait for a pair to finish.

The hub refuses a dependency from a ticket in the trial's epic on a ticket outside it, and
every source and copy is outside it, so the next block cannot depend on this block's pairs.
Instead `pair file --then <next>` defers the next ticket for two hours, and each block and the
report ticket start with `pair wait`, which defers them again while any arm is unfinished.

0. **Wait.** Run `just model-trial pair wait --trial <epic> --defer <this ticket>`. It exits 0
   when both arms of every filed pair have ended (closed, or released by their first
   session). If it exits 3, it has already deferred this ticket: run
   `ignite release -m "<the arms it listed are not finished>"` and stop. Defer before you
   release, never after. The hub does not count the release of a ticket that is deferred to a
   later time, but it counts any other, and holds a ticket back from the pool once two
   sessions have released it. A ticket left undeferred after its release can also be given to
   a second worker straight away.
1. **Grade and review.** Run `just efficiency ingest`, then
   `ignite run -- just model-trial grade --trial <epic>` and
   `ignite run -- just model-trial review --trial <epic>`. They cover every pair that has
   finished and skip what they have done before.
2. **Unfinished arms.** A copy from an earlier block that was released or stopped unfinished
   is closed here with `ignite ticket close`, and is graded as unfinished. A source released
   or stopped unfinished is a real ticket and stays open; its arm is graded as unfinished. A
   ticket still being worked is left alone and shows as pending.
3. **Suspect pairs.** `grade` marks a pair `suspect` when it finds a sign that one arm saw
   the other: the other arm's ID, title, tree or branch, words that name the trial, comments
   on other tickets, or distinctive lines both diffs share that the ticket does not dictate.
   Read the evidence of every suspect pair not yet decided, and the sessions themselves if it
   is unclear. Void the pair if one arm could have used the other's work, or clear it if the
   match has an innocent cause (the obvious name for a helper, say), with
   `just model-trial pair contamination <pair> --trial <epic> --void|--clear --note <why>`.
4. **Audits.** For each planted pair whose source has landed and that has no audit, read what
   the source landed against the planted sentence (`pairs.jsonl` holds the original, the
   planted one and the expected response). If the false sentence led the session to build
   something that should not exist or to break something, file a fix ticket under the
   source's parent. Either way, comment on the source saying which sentence was planted and
   what is true instead. Record the result with
   `just model-trial pair audit <pair> --trial <epic> --harm none|fixed:<ticket> --note <text>`.
5. **Report.** Run `just model-trial report --trial <epic> --comment <epic>`.
6. **The cap.** If the spend the report prints plus `block_reserve_usd` is over `cap_usd`,
   comment on the epic that the cap was reached, close this ticket and every later block
   ticket with that reason, and stop. The report ticket then becomes ready.
7. **Mark.** Run `just model-trial pair next --trial <epic> --block <N>`. It marks the next
   `block_size` listed tickets without filing anything, never passing `max_pairs`. A listed
   ticket that can no longer be an arm (by the same reasons as `candidates`, such as a saved
   branch of earlier work that its session would resume while the copy starts from nothing)
   is skipped, given back its allocation so that the pool works it as usual, and the next one
   is taken in its place. Record each skipped ticket in a comment on the epic. If it marks
   none, the list is used up: close this ticket and every later block ticket saying so, and
   stop. Then read each marked ticket and the code it names.
8. **Copy titles.** Give each marked pair's copy a title with
   `just model-trial pair title <pair> --trial <epic> "<title>"`: the same request as the
   source's title in other words, with no word that marks it as a copy or a trial. A copy with
   its source's title would be found by any title search, and either session could then read
   the other's work. `pair file` refuses a pair without one. With several copy arms, give each
   other arm's copy its own title with `pair title <pair> --arm <arm> "<title>"`, different
   from the source's and every other copy's. `pair file` files no copy that has no title and
   says which titles it is missing, so a source filed before an arm was added gets that arm's
   copy at the next `pair file` once you title it.
9. **Plant.** In `planted_per_block` pairs of your choice, change one sentence of the source's
   text so that it states something untrue about the code (a helper that does not exist, a
   behaviour the code does not have), with
   `just model-trial pair plant <pair> --trial <epic> --find <sentence> --replace-with
   <sentence> --expect <what a good response looks like>`. A session that checks the ticket
   against the code must be able to catch it, and the ticket must still be doable. The source
   lands, so pick a sentence whose false version would not lead a session that believed it
   into harmful work. The sentence goes into the real ticket, so both arms read the same words.
10. **File.** Run `just model-trial pair file --trial <epic> --then <next>`, where `<next>` is
    the ticket that depends on this one (`ignite ticket dep list <this ticket>`), which filing
    defers. Filing sets
    both tickets of each pair to P1, so that the trial runs as fast as its slowest source. It
    checks each source again, skips one that can no longer be an arm (no longer open and
    unclaimed, say) and gives it back its allocation; record each one skipped on this ticket.
11. **Close** this ticket with `ignite close -m`, listing each pair's source, copy and token.

## Writing the final reading

The report ticket runs when the last block's pairs have closed, or when the cap or the end of
the list ended the trial.

1. Wait as in step 0 of "Running a block". Then run `just efficiency ingest`, and `grade` and
   `review` through `ignite run --`. Close any
   copy released or stopped unfinished, decide any suspect pair, and audit any planted pair
   whose source landed since the last block, as in "Running a block".
2. Check that `ignite ticket list --label trial:copy --json` shows every copy of this trial
   closed.
3. Run `just model-trial report --trial <epic> --comment <epic>` for the final table.
4. Add a second comment on the epic, written for the user and following "Writing for readers"
   in the root `AGENTS.md`:
   - what the table shows for each kind of ticket, with the number of pairs behind each
     statement;
   - which differences the sample is too small to show (see below);
   - the pairs worth reading, with their branch names: the widest gaps, and each planted
     premise with what its audit found;
   - the differences between the arms the design could not remove: the copy is told its work
     will not land, and only the source passes the landing gate and tip verification and may
     retry after a rejection, so the source is graded on its first landed commit;
   - a proposed wording for the tier rule (the model tier bullet in the root `AGENTS.md` and
     in `.agents/skills/ignite/SKILL.md`), or a statement that the trial gives no reason to
     change it. Do not edit either file, or any setting in `ignite.toml`: routing is the
     user's choice;
   - that `just model-trial judge --trial <epic>` shows him `judge_pairs` pairs blind, and that
     `report` afterwards prints how often the reviewer agreed with him.
5. Start `just model-trial judge --trial <epic> --port 0` once with `BERYL_CACHE` pointed at a
   temporary copy of the trial's records, check that it serves the pairs and records a
   verdict, then stop it.
6. The copies' sessions may have filed real tickets about work that never landed. List them
   from `grades.jsonl`, and close each one that duplicates a ticket the source's session
   filed or that is only about the copy's own unlanded code.
7. Check that `tools/AGENTS.md` and this skill describe the command as it ended up. Add to this
   skill what the trial taught about running the next one: what to change in the spec, which
   kinds of ticket made poor pairs and why, and what a pair really cost. Land that edit, or
   close the ticket with `ignite close -m` if there is nothing to change.

To compare with an earlier trial, `just model-trial report --trial <A> --trial <B>` prints both
side by side and flags any difference in rubric or model IDs.

## What a trial cannot show

- About 24 pairs can show a large difference (one arm failing 30% of tickets where the other
  fails 5%) and cannot show one under about 15 points. Fewer pairs, or a split by kind, show
  less. Say how many pairs stand behind every statement.
- It says nothing about how each arm's code holds up under weeks of later changes.
- Its answer expires with the next model release, which is why the tool is kept: rerun it with
  a new spec rather than reading an old trial as current.
- The reviewer is a model too. Its picks mean more once the user's blind verdicts show how
  often it agrees with him.
