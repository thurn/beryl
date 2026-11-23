---
name: ui-review
description: Play Beryl's game UI headless against the live engine and report the usability defects a player would hit (feedback, flow, state legibility, layout, prompts and AI activity), with measurements and repro steps. Use when asked to review, QA, playtest or audit the UI, or to check a UI change from the player's side before it ships.
---

# UI review

Find what is wrong with Beryl's UI the way a strong Magic player would find it on
their first evening with the app, but with a QA engineer's instruments. The
output is a ranked list of defects, each with steps to reproduce and evidence.

You are judging the experience, not the code. Read `rust/beryl3d/` freely to learn
what something is meant to do, but every finding must be something a player
sees, waits through, or cannot do.

## Running it

- **Expect a long session.** A thorough review takes one to two hours, most of
  it waiting on the searching agents.
- **Two reviewers find much more than one.** Tested against a set of known
  defects, a single review found about two thirds of them, and two
  independent reviews with different seeds found about five sixths. For a full
  audit, run two with separate live app processes and record both opponents
  and the game seeds when available.
- **Reviews tend to check that something exists and stop there.** The misses
  were things reviewers saw and called "shown" or "readable" without judging
  how they looked. They also skipped checks that took manual work, such as
  hover sweeps and multi-blocks. The rules below about verdicts, helpers and
  the ledger exist to close those gaps.

## The bar

Measure the UI against three references and report every gap:

1. **The player's intent.** At every moment a player wants to know: what just
   happened, whose move it is, what can I do, and what will this do. Anything
   that leaves one of those unanswered is a defect.
2. **MTG Arena and [UI Conventions](../../../docs/ui-conventions.md).** Arena
   is the reference for flow, motion, timing, table layout and wording
   (AGENTS.md, UI); Beryl keeps its own skin. Beryl differs from Arena only
   where UI Conventions lists a numbered exception, and follows each entry
   there. Any other difference from what Arena does at the same moment is a
   finding: find the moment with the `arena-video` skill and cite its
   recording id and time. The design philosophy below is the project owner's:
   judge by it what Arena and UI Conventions do not settle, and how well Beryl
   does what they do.
3. **Strong digital Magic clients.** [conventions.md](conventions.md) lists what
   good clients do for each part of the game. When Beryl does less, or asks
   more of the player, that is a finding. It is a defect if a player would be
   confused or blocked, and a design note otherwise. Where the checklist and
   Arena differ, Arena wins.

Do not dismiss something because it is "technically correct" per the rules or
"probably intended". A rules-correct UI that a player cannot follow is broken.

## Design philosophy

Beryl should feel seamless. Judge every flow by these principles, within what
Arena and UI Conventions settle:

- **The common case is one gesture.** The things a player does all game
  (playing a land, casting a spell, attacking, blocking, passing) should take
  the fewest decisions and clicks possible. Rare cases can take more steps, but
  the common case must never pay for them. Count the clicks and prompts a
  routine action takes, and report the count.
- **The client does the bookkeeping.** When a strong player would answer a
  question the same way nearly every time, the client answers it: an
  obviously indifferent order, an evident payment, a stop where nobody would
  act, a choice with one sensible answer. It shows what it chose, and it lets
  the player change that in place. A question the player must stop and answer
  is the last resort.
- **Ask once, in context.** A choice that is really needed is asked once,
  where the action happens, with the board in view and each option saying what
  it does. One intent should not become a chain of modal prompts, and options
  should not be abstract labels.
- **Arena, not invention.** A player who knows Arena should never have to
  learn a Beryl-specific way to do a standard thing. Report each departure from
  Arena that UI Conventions does not list as an exception, with Arena's moment
  beside Beryl's. Where the recordings show nothing, Beryl builds it from the
  motions and prompt shapes it already has, listed under "Not seen in Arena";
  a new look there is a finding.
- **Show, don't make them infer.** Whatever happens is shown happening, at a
  pace a player can follow. Whatever matters is visible without a hunt.
- **Arena's words.** Labels and prompts use Arena's wording exactly. Where
  Arena shows no label, on-screen text is a cost: flag text that could be half
  as long.
- **The opponent's cards read as clearly as yours.** Every spell, ability,
  draw, discard, mill, search and counter the opponent makes is shown to the
  player at least as plainly as Arena shows it from the bottom seat.
- **Motion is Arena's.** What moves, from where to where, how it eases and how
  long it takes match Arena at the same moment, as do what appears or leaves
  in place (UI Conventions, "Settled as Arena"). A motion Arena has and Beryl
  lacks is a finding; so is one Beryl adds.
- **One button system.** Every action button shares one design and sits where
  Arena puts it, in Arena's order for that prompt.

A flow that works but breaks one of these principles is a finding. It is Major
when it affects something players do every turn, and Design otherwise.

## Native harness

```bash
just b3d play --for 20m
just b3d live tree --app <pid>
just b3d live png --app <pid>
just b3d live do 'press button "Keep"' --app <pid>
just b3d frames
just b3d live quit --app <pid>
```

- **Start** with `just b3d play --for <duration>` (at most 1h; `--opponent
  <agent>` picks another, `mcts-hidden` by default). It opens the windowless app
  on a fresh game, waits until the tree shows the `game` region and prints the
  process id, the socket and the commands below. The app ends itself after
  `--for`, so keep the review inside it.
- **Read the table** with `live tree`: each line is a locator, with its place,
  box and states, and the first line is the app's clock (`t=<ms>ms`). Look at
  pictures with `live png` and read the PNG it names, before and after every
  action.
- **Act** with `live do '<action>'`: `press`, `hover`, `drag` and the other
  actions in the [b3d skill](../b3d/SKILL.md). Use the tree to locate targets
  and the picture to judge which targets a player can discover and reach.
- **Measure** with the tree's clock for waits (take the `t=` before and after an
  action, and until the next prompt appears), and `just b3d frames` for frame
  times, late frames and the worst frames of the run.
- **Quit** with `live quit`. Do it after a failed check too.

Use `just b3d list` to find recordings for states live play cannot reach.
`data/beryl3d/fixtures.json` defines the named fixtures. `just b3d check`
reports interface faults in a recording. Use `tree`, `diff`, `png` and
`film` to inspect its actions at specific times and test it in both layouts.

## Coverage

Keep a ledger as you go and do not stop until it is full:

- **Every decision kind.** The prompt shapes and purposes in
  `rust/crates/beryl_protocol/src/protocol/` describe what the engine can ask. For each kind, meet it live if you can (or else in a
  fixture), answer it at least two different ways, and note how it was asked.
- **Every piece of state.** `ObjectView`, `PlayerView` and `StackItem` in the
  protocol module list what the engine tells the UI. For every field of those types,
  find how the screen shows it. If you cannot find it, that is a finding. For
  each field that is drawn, give the verdict that **Legible state** below asks
  for, citing paired screenshots. "Shown" and "readable" are not
  verdicts.
- **Every routine action.** Count the gestures and prompts each of these takes,
  and compare each count with Arena's:
  - play a land;
  - cast a plain spell, and each kind of spell with an extra choice that your
    decks offer (X, kicker or another optional cost, modes, targets);
  - activate an ability;
  - attack with one creature and with several;
  - block with one creature, and with two on one attacker;
  - pass the turn.
- **Every multi-pick.** Meet each decision that takes several picks with at
  least three eligible choices. That includes assigning several of your
  creatures to one of theirs, as when two block one attacker. Set the state up
  in play, or use a fixture from `data/beryl3d/fixtures.json` that names it.
- **Every row swept.** Move the pointer across each row of cards, in an early
  state and on a crowded board.
- **Every opponent.** The start screen lists agents. Play at least one full game
  against the default and one against the slowest, searching agent. Waiting on
  the AI is part of the experience.
- **Every control.** Click, hover and drag every distinct kind of thing on
  screen once: each zone, each badge, each button, each card in each zone, both
  players' things, including things that look inert.
- **Long enough to get crowded.** Play past turn 8 at least once, so the board
  is busy and combat, triggers, counterspells and removal have come up.

Save the replay or record the game seed when available. Include the opponent,
turn and exact action with each finding.

## The loop

At every step, do this:

1. **Look.** Take a screenshot. Before you act, write down what the screen tells
   you: whose turn, what step, what just happened, what you can do now, and what
   you expect your action to do.
2. **Check the state.** Use `live tree` to compare the visible board and
   prompt with the resolved elements and their states. For a wrong motion,
   find the same case in a recording or card spec and inspect its script
   steps with `just b3d timeline`.
3. **Act as a player would.** Use the pointer, at visible points, and the
   gestures the UI seems to invite (drag for cards, click for buttons). If the
   way to do something is not obvious, record that before you read the code to
   find it.
4. **Watch what happens next.** Use `live do` to act. Capture the next
   frame with `live png` and use the frame log to measure an unexpected wait.
5. **Judge.** Did it do what you predicted? How long until the screen first
   responded, and until you could act again? Did anything move, appear or vanish
   that you did not cause?

## What to probe

Each item below is a general failure mode with a way to test it. Apply each one
everywhere it could occur, not just at the first place you think of.

- **Feedback and waiting.** Any gap of more than about 300 ms between an action
  and a visible response is a finding. So is any wait of more than about a
  second with no sign of what the game is doing. Report the measured time.
- **Transient states.** Treat loading, empty, waiting and error states as
  screens in their own right. Capture them, check that what they say is true,
  and use every control they offer.
- **Things that happen without you.** When several views arrive between two of
  your decisions, the game moved on without you. Check that each
  event in those views was visible as it happened, in order, and long enough to
  read. It is not enough that the end state is right: each opponent action and
  automatic event must be visible as it happens.
- **Stopping and skipping.** At each decision, ask: would a strong player want
  to stop here? A stop where nobody would act is a needless question. Skipping
  ahead where a player would want to look, respond, or at least see what
  happened is worse. The player should always be able to tell when control
  passes from one player to the other.
- **Highlights mean something.** Find out what each glow, outline or dim state
  is meant to mark, from the screen or the code. Then check it on every element
  against the decision's candidates. Anything marked must work when used, and
  anything usable must be marked.
- **Multi-pick decisions.** Wherever the player picks several things (attackers,
  blockers, targets, cards to discard or bottom), test selecting, deselecting,
  selecting everything and the count limits, and check the highlights again
  after each change.
- **Prompts.** For each prompt, note what you were asked, in how many separate
  steps, and whether every option says what it does. Flag these: options that
  differ in ways the label does not say, prompts that hide the
  board or the card they are about, cut-off text, and prompts the client could
  answer itself (one sensible answer, or answers that make no difference).
  Compare each with conventions.md.
- **Stillness.** Hover across each row of cards and compare tree bounds
  before and after. For a replay, use `diff` and `film` to see unrelated
  objects that shift or get covered. Report the measured distances.
- **Reachability.** Use `check` on recordings with a full hand and a busy
  board. In the live game, try each card and zone. Every item must be visible or
  reachable, and every pile (graveyard, exile, library count) must open for
  browsing where the rules let a player look.
- **Legible state.** Judge how each state is drawn, not only whether it is.
  For each state in the coverage ledger, capture the same card, or an
  identical one, with and without it, at the size it has on the table. Your
  verdict must answer three questions:
  1. What exactly distinguishes the two?
  2. Would a player see that difference at a glance from across the table,
     even in a pile of similar cards?
  3. Is the card drawn faithfully in both states, with its art, frame and
     proportions intact?

  Then compare with how Arena shows the same state. A weaker or stranger signal
  than Arena's is a finding. Every relation between objects that the protocol
  carries (each field that names another object) must be readable from a still
  frame on a crowded board, however the UI draws it.
- **Layout.** Measure each region's alignment, spacing and symmetry against its
  container and its counterpart, rather than eyeballing. Nothing on the game
  screen should scroll.
- **Accessibility basics.** Check text contrast and size at normal viewing
  distance, keyboard access to the main actions (pass, confirm, cancel), and
  visible focus.
- **Truncation.** Inspect prompts, overlays, tooltips and badges with long card
  names. Run `check` on recordings that show the same text.
- **Abuse.** Double-click, click while waiting, drag to wrong places, press
  Escape, quit and resume a game, and return to the start screen during play.
  Check that the current game and the visible controls still agree.
- **Errors.** Inspect the app log path printed by `play` and the errors from
  `live`.

## Report

Write the report as Markdown in your final message. Subagents may not be able
to write files, so the message is the report. Lead with a one-line verdict
and the seeds you played. Then list findings, most severe first, grouped by
severity:

- **Blocker**: the player is stuck, misled about the game state, or cannot do
  something the rules allow.
- **Major**: the player is confused, or has to work hard for something routine.
- **Minor**: a visible flaw that does not stop or mislead.
- **Design and convenience**: a place where a good client would do better,
  including decisions the client could make for the player.

Give each finding:

- a title naming the symptom
- the steps to reproduce it (seed, opponent, turn, and what you did)
- the expected and the actual behavior, with measurements where you have them
- the screenshot paths

Merge findings that share a root cause, but list every place it shows up. End
with the coverage ledger:
- decision kinds met (live or by fixture);
- the verdict for each piece of state;
- the gesture and prompt count for each routine action, next to Arena's;
- multi-picks met, with their eligible counts;
- rows swept;
- agents played;
- anything you could not reach, and why.

Do not count a harness failure (a crashed driver, a gesture the harness
dropped) as a UI defect. Report it separately.
