---
name: ui-review
description: Play Beryl's game UI headless against the live engine and report the usability defects a player would hit (feedback, flow, state legibility, layout, prompts and AI activity), with measurements and repro steps. Use when asked to review, QA, playtest or audit the UI, or to check a UI change from the player's side before it ships.
---

# UI review

Find what is wrong with Beryl's UI the way a strong Magic player would find it on
their first evening with the app, but with a QA engineer's instruments. The
output is a ranked list of defects, each with steps to reproduce and evidence.

You are judging the experience, not the code. Read `ui/src/` freely to learn
what something is meant to do, but every finding must be something a player
sees, waits through, or cannot do.

## Running it

- **Expect a long session.** A thorough review takes one to two hours, most of
  it waiting on the searching agents.
- **Two reviewers find much more than one.** Tested against a set of known
  defects, a single review found about two thirds of them, and two
  independent reviews with different seeds found about five sixths. For a full
  audit, run two, each in its own checkout: `just ui-live` records fixtures into
  the checkout, so two runs in one checkout collide.
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
2. **The design philosophy below.** It is the project owner's, and it outranks
   everything else here.
3. **Strong digital Magic clients.** [conventions.md](conventions.md) lists what
   good clients do for each part of the game. When Beryl does less, or asks
   more of the player, that is a finding. It is a defect if a player would be
   confused or blocked, and a design note otherwise. The checklist describes
   what clients do. Where it accepts something the philosophy rejects, such as
   an extra prompt or a stop, follow the philosophy.

Do not dismiss something because it is "technically correct" per the rules or
"probably intended". A rules-correct UI that a player cannot follow is broken.

## Design philosophy

Beryl should feel seamless. Judge every flow by these principles:

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
- **Convention over invention.** MTG Arena is the primary reference for how
  standard things look and behave, then MTGO. A player who knows Arena should
  never have to learn a Beryl-specific way to do a standard thing. Beryl may
  depart from Arena only where a player would see the result as clearly
  better. Report each departure and say whether it is better.
- **Show, don't make them infer.** Whatever happens is shown happening, at a
  pace a player can follow. Whatever matters is visible without a hunt.
- **Few words.** On-screen text is a cost. Where an icon, a highlight or the
  board itself can say it, words are noise. Flag text that could be half as
  long.
- **The opponent's cards read as clearly as yours.** Every spell, ability,
  draw, discard, mill, search and counter the opponent makes is shown to the
  player as plainly as their own: the card or ability comes up large enough to
  read, and each card it moves travels visibly to where it goes.
- **Everything appears with animation.** Nothing pops in or blinks out: cards,
  choices, prompts and counters arrive from where they come from (a mode
  option out of its card, a drawn card out of the library) and leave toward
  where they go.
- **Picking how to play a card is picking a card.** Modes, alternate costs
  (evoke, overload), cycling and ability choices appear as full-size cards
  with art in the middle of the screen, out of the source card, never as a
  text prompt.
- **One button system, bottom right.** Every action button shares one design
  and sits in the bottom-right corner, with Resolve. No buttons in the middle
  of the screen.

A flow that works but breaks one of these principles is a finding. It is Major
when it affects something players do every turn, and Design otherwise.

## Harness

`just ui-live` builds `beryl_cli`, serves the app's real game commands
(`beryl_cli serve`), serves the UI against them (`?live=`), and holds a headless
WebKit page open. It prints three lines: `app`, `drive` (a URL) and
`screenshots` (a folder). Start it in the background and keep it running for
the whole review. Use `--port 0` and the printed URLs so parallel reviews
get separate ports. Keep track of the job you started.

```bash
just ui-live --port 0 > "$SCRATCH/live.log" 2>&1 &
until grep -q "ui-live: screenshots" "$SCRATCH/live.log"; do sleep 1; done; cat "$SCRATCH/live.log"
```

Drive the page by POSTing the body of an async JavaScript function to the
`drive` URL. It runs in Node with Playwright's `page` and these helpers in
scope, and returns JSON:

```bash
curl -s "$DRIVE" -d 'await page.getByRole("button", {name: "Start", exact: true}).click(); return await shot("started")'
```

- `probe.act(name, action, {samples?, wait?})` is how to take every action. It
  takes a screenshot before, runs `action` (an async function), takes
  screenshots 100 ms, 500 ms and 2 s after it ends, waits for the next decision
  (up to `timeout`, 120 s by default) and takes one more. It returns the
  screenshot paths, the latency to the first view and to the decision
  (`firstViewMs`, `decisionMs`), what moved, and the game events that arrived
  meanwhile. Read the screenshots it returns, and quote its latencies rather
  than timing things yourself.
- `probe.compare(name, [locators or selectors])` crops each element and lays
  the crops side by side in one labelled image. Use it to judge whether states
  can be told apart, or whether things that should look alike do.
- `probe.sweep(locator, {dwell?})` moves the pointer slowly across every element
  the locator matches, left to right, the way a player browses a row. For each
  stop it reports anything other than the hovered card that moved, and anything
  that became covered. Run it on every row of cards: your hand, each
  battlefield row, the stack and each pile.
- `shot(name)` saves a screenshot and returns its path. Read the PNG to see it.
  Take one before and after every action, and more during waits.
- `probe.pointAt(locator)` moves the mouse onto the visible part of an element
  without scrolling. Use it, or `page.mouse`, for hovering and for drags
  (`page.mouse.down()`, `move(x, y, {steps: 15})`, `up()`). Playwright's `hover`
  and `click` scroll their target into view first, which a player's pointer
  never does.
- `probe.motion(selector?)` starts recording every tracked element's box each
  frame. It returns a function: call it after your action, and it waits for N ms
  (1000 by default), then reports what moved or resized, how far and over how
  many frames, and which boxes scrolled.
- `probe.covered(selector?)` lists elements that something else sits on top of.
- `probe.truncated()` lists visible text cut off by its box.
- `probe.timeline({since})` gives the live game's ground truth: every command
  the page sent, and every view the engine emitted, with its turn, step,
  priority, whether the human was asked something, and the game events in it
  (casts, zone changes, taps, damage, attacks, and so on). `since` is a
  `performance.now()` time on the page.
- `probe.decision(action?, timeout?)` runs `action` (an async function), then
  waits for the next view that asks the human something or ends the game. It
  reports how long that took, how many views arrived, and what is being asked.
- `errors` lists the page's errors so far. Any entry is a finding.

A change to `ui/scripts/drive.mjs` needs a restart, and a change under `ui/src/`
reloads the page (which loses the game, as it would in the app). Stop the
harness when you finish, including after a failed check. Interrupt its
terminal session with Ctrl-C, or send SIGTERM to your `by ui-live` process
so it cleans up its children. Verify that its printed app and driver ports
no longer have listeners.

The page is 1600×900. Restart with `--width 1280 --height 800` once to check a
small window. The default debug build gives the agents the same speed as
`just dev`, which is what the player sees.

Recorded fixtures reach rare states quickly: `/?fixture=<name>&at=<n>#/game`,
named in `ui/fixtures.json`. Use them only for states that live play does not
reach. They replay views with no real timing, so judge flow and timing live.

Practical notes from earlier reviews:

- **Locators.** Use `exact: true` with role names, or the cards' `data-card`
  and `data-slot` attributes. Loose names match card text too. Menu entries may
  not be exposed as buttons by role, so `page.locator("button", {hasText})`
  can find what `getByRole` misses.
- **Slow agents.** A searching agent can take several minutes per decision.
  Your shell tool may time out before `curl` returns, so for long waits pass
  `{wait: false}` to `probe.act`, then poll `probe.decision(null, 100000)`.
- **Time on the slow agent.** You do not need to finish a game against the
  slowest agent by hand. Play it until you have measured its waits on each
  kind of moment (the opening, your turn, its turn, a response), then finish
  coverage with faster opponents.

## Coverage

Keep a ledger as you go and do not stop until it is full:

- **Every decision kind.** `Kind` in `ui/src/generated/protocol.ts` lists what
  the engine can ask. For each kind, meet it live if you can (or else in a
  fixture), answer it at least two different ways, and note how it was asked.
- **Every piece of state.** `ObjectView`, `PlayerView` and `StackItem` in the
  same file list what the engine tells the UI. For every field of those types,
  find how the screen shows it. If you cannot find it, that is a finding. For
  each field that is drawn, give the verdict that **Legible state** below asks
  for, citing a `probe.compare` image. "Shown" and "readable" are not
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
  in play, or use a fixture from `ui/fixtures.json` that names it.
- **Every row swept.** Run `probe.sweep` on each row of cards, in an early
  state and on a crowded board.
- **Every opponent.** The start screen lists agents. Play at least one full game
  against the default and one against the slowest, searching agent. Waiting on
  the AI is part of the experience.
- **Every control.** Click, hover and drag every distinct kind of thing on
  screen once: each zone, each badge, each button, each card in each zone, both
  players' things, including things that look inert.
- **Long enough to get crowded.** Play past turn 8 at least once, so the board
  is busy and combat, triggers, counterspells and removal have come up.

Use a seed (the start screen's field) for each game and record it, so every
finding can be reproduced.

## The loop

At every step, do this:

1. **Look.** Take a screenshot. Before you act, write down what the screen tells
   you: whose turn, what step, what just happened, what you can do now, and what
   you expect your action to do.
2. **Check against the truth.** Compare that with `probe.timeline()` and the
   open decision. Every disagreement is a finding: an event you could not see,
   a highlighted card that the decision does not offer, an offered option with
   no visible way in, a step or turn shown wrongly.
3. **Act as a player would.** Use the pointer, at visible points, and the
   gestures the UI seems to invite (drag for cards, click for buttons). If the
   way to do something is not obvious, record that before you read the code to
   find it.
4. **Watch what happens next.** Take the action through `probe.act`, including
   the first click on the start screen and clicks that open menus (pass
   `{wait: false}` when no decision will follow). Look at every screenshot it
   returns, and note what the screen showed between them.
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
  your decisions (`views` > 1), the game moved on without you. Check that each
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
- **Stillness.** `probe.sweep` each row of cards, and watch `probe.act`'s
  motion report around each new view. Only what you point at, or what changed
  in the game, should move. If other elements move, get covered, or the page
  scrolls, report the measured distances.
- **Reachability.** Run `probe.covered()` in several states, especially with a
  full hand and a busy board. Every zone and every card must be visible or
  reachable, and every pile (graveyard, exile, library count) must open for
  browsing where the rules let a player look.
- **Legible state.** Judge how each state is drawn, not only whether it is.
  For each state in the coverage ledger, `probe.compare` the same card, or an
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
- **Truncation.** Run `probe.truncated()` on every prompt, overlay, tooltip and
  badge you meet, with long card names.
- **Abuse.** Double-click, click while waiting, drag to wrong places, press
  Escape, reload mid-game, use the browser's back button, and start a new game
  mid-game. The UI must never stick or lie.
- **Errors.** Check `errors` after each step.

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
