---
name: ui-review
description: Play Beryl's game UI headless against the live engine and report the usability defects a player would hit (broken flows, dead ends, silent AI turns, misleading highlights, unstable layout, hidden or unreadable state, confusing prompts), with measurements and repro steps. Use when asked to review, QA, playtest or audit the UI, or to check a UI change from the player's side before it ships.
---

# UI review

Find what is wrong with Beryl's UI the way a strong Magic player would find it on
their first evening with the app, but with a QA engineer's instruments. The
output is a ranked list of defects, each with steps to reproduce and evidence.

You are judging the experience, not the code. Read `ui/src/` freely to learn
what something is meant to do, but every finding must be something a player
sees, waits through, or cannot do.

## The bar

Measure the UI against two references and report every gap:

1. **The player's intent.** At every moment a player wants to know: what just
   happened, whose move it is, what can I do, and what will this do. Anything
   that leaves one of those unanswered is a defect.
2. **Strong digital Magic clients.** [conventions.md](conventions.md) lists what
   good clients do for each part of the game. When Beryl does less, or asks
   more of the player, that is a finding. It is a defect if a player would be
   confused or blocked, and a design note otherwise.

Do not dismiss something because it is "technically correct" per the rules or
"probably intended". A rules-correct UI that a player cannot follow is broken.

## Harness

`just ui-live` builds `beryl_cli`, serves the app's real game commands
(`beryl_cli serve`), serves the UI against them (`?live=`), and holds a headless
WebKit page open. It prints three lines: `app`, `drive` (a URL) and
`screenshots` (a folder). Start it in the background and keep it running for
the whole review:

```bash
just ui-live > "$SCRATCH/live.log" 2>&1 &
until grep -q "ui-live: screenshots" "$SCRATCH/live.log"; do sleep 1; done; cat "$SCRATCH/live.log"
```

Drive the page by POSTing the body of an async JavaScript function to the
`drive` URL. It runs in Node with Playwright's `page` and these helpers in
scope, and returns JSON:

```bash
curl -s "$DRIVE" -d 'await page.getByRole("button", {name: "Start", exact: true}).click(); return await shot("started")'
```

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
harness when you are done: `kill %1`, or kill the `by ui-live` process.

The page is 1600×900. Restart with `--width 1280 --height 800` once to check a
small window. The default debug build gives the agents the same speed as
`just dev`, which is what the player sees.

Recorded fixtures reach rare states quickly: `/?fixture=<name>&at=<n>#/game`,
named in `ui/fixtures.json`. Use them only for states that live play does not
reach. They replay views with no real timing, so judge flow and timing live.

## Coverage

Keep a ledger as you go and do not stop until it is full:

- **Every decision kind.** `Kind` in `ui/src/generated/protocol.ts` lists what
  the engine can ask. For each kind, meet it live if you can (or else in a
  fixture), answer it at least two different ways, and note how it was asked.
- **Every piece of state.** `ObjectView`, `PlayerView` and `StackItem` in the
  same file list what the engine tells the UI: tapped, summoning sick, damage,
  counters, attachments, the permanent that exiled a card, combat roles, the
  stack and its targets, each zone, life, mana. For each one, find how the
  screen shows it. If you cannot find it, that is a finding.
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
4. **Watch what happens next.** Wrap the action in `probe.decision` and a
   motion probe, and take screenshots at about 100 ms, 500 ms, 2 s and then
   every few seconds until the next decision. Note what the screen showed
   between them.
5. **Judge.** Did it do what you predicted? How long until the screen first
   responded, and until you could act again? Did anything move, appear or vanish
   that you did not cause?

## What to probe

Each item below is a general failure mode with a way to test it. Apply each one
everywhere it could occur, not just at the first place you think of.

- **Feedback and waiting.** Any gap of more than about 300 ms between an action
  and a visible response is a finding. So is any wait of more than about a
  second with no sign of what the game is doing. Report the measured time.
- **Transient states.** Screenshot every intermediate screen during waits
  (loading, empty and placeholder states). Click what they offer. A screen that
  says something false, or offers a control that strands the player, is a
  serious defect even if it only lasts a few seconds.
- **Things that happen without you.** When several views arrive between two of
  your decisions (`views` > 1), the game moved on without you. Check that each
  event in those views was visible as it happened, in order, and long enough to
  read. It is not enough that the end state is right. The opponent's actions
  matter most.
  Something you can only work out afterwards from a changed zone count is
  invisible.
- **Stopping and skipping.** At each decision, ask: would a strong player want
  to stop here? A stop where nobody would act is a needless question. Skipping
  ahead where a player would want to look, respond, or at least see what
  happened is worse. The player should always be able to tell when control
  passes from one player to the other.
- **Highlights mean something.** Find out what each glow, outline or dim state
  is meant to mark, from the screen or the code. Then check it on every element
  against the decision's candidates. Identical cards in identical situations
  must look identical. Anything marked must work when used, and anything usable
  must be marked.
- **Multi-pick decisions.** Wherever the player picks several things (attackers,
  blockers, targets, cards to discard or bottom), check that all eligible things
  are marked at once. Pick them in different orders, unpick some, and pick
  everything.
- **Prompts.** For each prompt, note what you were asked, in how many separate
  steps, and whether every option says what it does. Flag these: options that
  differ in ways the label does not say, prompts that hide the
  board or the card they are about, cut-off text, and prompts the client could
  answer itself (one sensible answer, or answers that make no difference).
  Compare each with conventions.md.
- **Stillness.** Run the motion probe while you sweep the pointer slowly across
  every row of cards (your hand, each battlefield row, each pile), and around
  each new view. Only what you point at, or what changed in the game, should
  move. If other elements move or the page scrolls, report the measured
  distances.
- **Reachability.** Run `probe.covered()` in several states, especially with a
  full hand and a busy board. Every zone and every card must be visible or
  reachable, and every pile (graveyard, exile, library count) must open for
  browsing where the rules let a player look.
- **Legible state.** For each piece of state in the coverage ledger, find two
  objects that differ only in that state. Crop both from a screenshot at normal
  size and compare. Could a player tell them apart at a glance, across the
  table? Also check that showing the state keeps the card readable. Check that
  relations between objects
  (attachments, exiled-by links, targets, attackers and blockers) can be read
  from a still screenshot on a crowded board.
- **Arrows and links.** For every arrow or connector, ask someone who has not
  seen the action to say from a still frame what connects to what. Try it with
  objects close together and with several arrows at once.
- **Layout.** Check that each zone's contents are centered or aligned in their
  area as the design intends, and that symmetric things are symmetric (your
  side and the opponent's). Measure them, rather than eyeballing. Nothing on
  the game screen should scroll.
- **Truncation.** Run `probe.truncated()` on every prompt, overlay, tooltip and
  badge you meet, with long card names.
- **Abuse.** Double-click, click while waiting, drag to wrong places, press
  Escape, reload mid-game, use the browser's back button, and start a new game
  mid-game. The UI must never stick or lie.
- **Errors.** Check `errors` after each step.

## Report

Write the report as Markdown in your final message. Lead with a one-line verdict
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
with the coverage ledger: decision kinds met (live or by fixture), state
checked, agents played, and anything you could not reach.
