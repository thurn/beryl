---
name: specs
description: Inspect, drive and verify a card's the client spec moments from both seats. Use when implementing or fixing a card's UI, reproducing a card-specific report or extending what specs generate. The native tools print prompts, element trees and check results, and draw filmstrips.
---

# Card specs

A spec collects the moments a card's UI needs to show. The recorder plays a
real session and saves views, animation scripts and replies as
`<card>/<moment>`. Check both seats. A card's controller and opponent both
need to see what changed, where it went and which choice caused it.

## Reproduce and check

```sh
just client spec counterspell
just client spec counterspell counters-a-spell-into-its-owners-graveyard --seat you --film
```

The first command lists every moment from both seats, with prompts, element
trees and interface check results. The second shows one seat of one moment
and prints paths to its film. Use the card name or slug. An unrecognized name
prints suggestions.

For a closer look, use the recording name and seat printed by `spec`:

1. Run `just client info <recording> --seat <seat>` for views, script steps and
   prompt times. Run `just client timeline <recording> --seat <seat>` to see
   when each step begins and ends and which cues play. A wrong script points
   to the session builder. A correct script played incorrectly points to the
   client.
2. Run `just client tree <recording> --at prompt:1 --seat <seat>` for the
   prompt's options and locators. Append `--do 'press button "Cancel"'` when
   that button is present, or use another locator from the tree. See the
   [client skill](../client/SKILL.md) for drag, hover and time-step actions.
3. Run `just client check <recording> --seat <seat>` for interface faults and
   use its printed `show:` commands. Add `--layout phone` when checking the
   phone layout. Use `just client png <recording> --at <moment> --tree` for a
   still or `just client film <recording> --from <start> --to <end>` to see
   motion around a step. Pass the same seat and layout to each command.

Use an existing spec moment for regression coverage when it reaches the broken
state. Add a scenario in the card's file when the state is missing.

## How a spec is generated

`engine/crates/beryl_cli/src/specs/auto.rs` builds each card's moments from two
sources, with short general labels:

- **The definition.** A permanent with activated abilities (not mana abilities)
  gets probes: `Stops: main 2` and `Stops: their end` (instant-speed abilities),
  `Lands tapped` (what stays offered unpaid), `Nothing to do` (an ability whose
  `Needs` is unmet must not stop) beside the same board with the need met
  (`Stops: card exiled`, through `Permanent::exiled_with`), each ability
  activated (labelled by its cost: `{2}{T}`, `+2`, `−1`), and one bounced in
  response (`{2}{T}, bounced`).
- **The card's scenarios** (`beryl_cards::scenarios()`), each under its own name
  as a heading (less the card's name). `Start` opens on its first board, and
  play runs the whole scenario (held to the turn a trailing `AdvanceTo` names).
  It is cut at each cast or activation of the card (`{T}`, played out once
  shown), the same activation with the card bounced in response
  (`{T}, bounced`), each question the card asks for each cause (`Ask: mayMove`),
  with a yes or no flipped (`Answer No`). For a card with activated abilities,
  its end or a flipped answer where the human holds priority goes on twice more:
  they pass (`Then pass`: where does it stop them next?) and the game reaches
  their next turn (`Next turn`: what does it offer untapped?). Scenario boards
  are dressed to look like a game (life off 20, graveyards, the opponent's lands
  and a response in hand, unplayable spells under each library, and a twin of
  the card in the human's hand where it starts there, so two cards just alike
  must be kept apart) only where the scenario still passes with the dressing. A
  scenario's name identifies its group of moments, so name it for what it shows.

Each candidate is played headless from both seats to its first live decision (or
the game's end), and what holds there is recorded in the fixture conditions'
words (`fixture.rs`'s `Condition::parse`), which describe the recorded position. Candidates
that repeat an earlier question or end are dropped; candidates whose scripts
cannot be played are reported as `skip` lines.

## Choosing what a card needs

Use this list to judge whether a generated spec is complete, and what general
capability to add when it is not:

1. **Priority and stops.** When the card makes the human stop, and when it must
   not: an affordable instant-speed ability stops them; one that would do
   nothing (`Needs`) must not, though it stays activatable.
2. **Each ability's path.** Clicking it (a permanent never drags); two or more
   non-mana activated abilities open the side-by-side chooser, unaffordable ones
   dimmed.
3. **Costs and payments.** Mana, X, alternate and additional costs, life,
   sacrifice, tapping creatures, loyalty.
4. **Prompts.** Each question the card asks: may, choose, target, order,
   discard.
5. **What it makes and changes.** Tokens, emblems, counters, damage, control
   changes, attachments, tapping.
6. **Zones.** Cards it exiles (and where they show), cards it makes playable
   from exile or the graveyard, what it puts into a graveyard.
7. **The stack.** Responses to it and from it: countered, bounced in response,
   its targets gone, its source gone.
8. **The other side.** Each moment's Them frame: the opponent watches the card
   played, and stops on it while they hold a response.
9. **Edges a player hits.** Empty hand, no legal target, nothing to choose, one
   choice (answered silently), more than one.

## How scripts run

- Both seats' actions run in the order listed, across seats: a response waits
  until what it answers has been played. An `AdvanceTo` holds the actions after
  it until the game reaches its turn and step.
- Short of its next action, a scripted seat passes priority, pays the first way,
  answers one-answer questions, declares no attackers or blockers, discards its
  first cards to hand size, and takes a scripted "may" the session folds into
  the question it leads to (a search's choice: yes is the choice, no is Done); a
  scripted target the engine chose silently (the only one) is skipped. Any other
  decision it cannot answer fails, naming it.
- Once both seats' actions are taken (and the game reaches the turn of a
  trailing `AdvanceTo`), the viewer's seat plays live. The other seat then
  passes and takes the first candidate of anything else.
- The human is under the smart stops, as in the app, and the session halts them
  at the start of their own first main phase (not when a moment starts there).

## When a moment is missing or wrong

Fix it in this order, preferring the general fix:

1. **A scenario in the card's file** that reaches the state. It also tests the
   rules and runs against XMage and Forge (`just oracle`), and the generator
   picks it up.
2. **A general capability in the generator**, which covers every card with the
   same ability, cost or question: a new probe in `from_definition`, a better
   cut or dedupe in `from_scenario`/`generate`, a condition `snapshot` records,
   or a scripted-seat rule in `boards.rs`'s `Scripted`. Say which other cards'
   moments it changed.
3. **A general fix in the UI**: a prompt, layout or wording belongs to a shape
   or a zone, never to one card, so fix it where every card with that shape gets
   it, and audit a few other cards that open it. A wrong motion, arrow or combat
   line can come from the script builder in
   `engine/crates/beryl_session/src/script/` or playback in
   `client/crates/game_play/`. See the [bugfix worker handoff](../bugfix/SKILL.md#worker-fixing-one-report).

There are no hand-written moments: a case only one card has is a scenario in its
file. A fixed engine bug gets its entry in `docs/bugs.md`.

## Judging against Arena

Beyond motion, judge each moment from both seats against Arena and
[UI Conventions](../../../docs/ui-conventions.md): find the same kind of moment
in the recordings with the `arena-video` skill and compare what is asked, in
what words, with which buttons, what is highlighted, and what moves where and
for how long. Beryl differs only where UI Conventions lists a numbered
exception; any other difference is a finding, cited with the recording id and
time. Where the recordings show nothing, judge by the design philosophy in
`.agents/skills/ui-review/SKILL.md`; never file a ticket asking the user to look
at something. Report what each moment shows from each seat, what you fixed,
which other cards' moments changed, and any moment you could not get and why.
