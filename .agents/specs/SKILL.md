---
name: specs
description: Write or update a card's spec — the live, scrollable page at /specs/<card> showing every UI moment of that card (stops, prompts, ability choosers, tokens, statuses, exile links, stack interactions, payments) as playable game states — and QA the card's UI through it. Use when asked to spec a card, add a card's UI states, QA a card end to end, or check that a card's UI follows the design.
---

# Card specs

A spec is a scrollable page, `/specs/<card-slug>`, that shows every moment of
play a card's UI must get right. Each moment is a real game, live and fully
playable in the page: the engine behind it is the app's. Scrolling a spec is QA
testing the card.

Your job is the subjective part: deciding which moments the card needs, building
boards that look like real games, and judging what you see. The automatable
part (each moment reaching its state, and what must hold there) is checked by a
test.

## The pieces

- `rust/crates/beryl_cli/src/specs/<card>.rs`: one file per card, a `Spec` of
  `Moment`s, registered in `SPECS` in `specs/mod.rs`. Read
  `specs/currency_converter.rs` first; it is the model.
- A `Moment` is:
  - `board`: a `Scenario` (the human is seat 0). Its `.actions(...)` are the
    **setup**, played before the page shows anything.
  - `.play(...)`: actions played out, animated, **when the viewer presses
    play**. The replay button starts the moment over. Use it for what must be seen happening: a
    counter, a bounce, a token appearing.
  - `.expect(...)`: conditions on the human's **first live decision** once all
    actions are taken, in the fixture vocabulary (`fixture.rs`'s
    `Condition::parse`): `turn 7`, `your turn`, `their turn`,
    `step postcombatMain`, `decision yesNo`, `ask mayMove`, `2 candidates`,
    `resolving <card>`, `offers <card> ability <n>`, `tapped <card>`,
    `controls <n> <card>`, `<n> exiled with <card>`, `castable from exile`,
    `not <condition>`, and the rest listed there. Add a condition when you need
    one.
  - `title` (a few words: what it shows) and `.look(...)` (a few words: what to
    look at or try). Halve every sentence you write.
- `just specs --check [slug]` plays every moment headless and checks it;
  `cargo test -p beryl_cli specs` runs the same in CI.
- `just specs --shots [slug]` screenshots every moment, settled, into
  `$BERYL_CACHE/spec-shots/<checkout>/<slug>/` and prints the paths.
- `just specs [slug]` serves the pages for a human and prints the URL
  (`http://localhost:1430/specs/<slug>`). It opens no window.

## Choosing moments

Go through the card's oracle text and its file's scenarios, and list a moment
for each of these that applies. Most cards need 6 to 15.

1. **Priority and stops.** When does the card make the human stop, and when
   must it not? A cheap instant-speed ability that is affordable stops them; one
   that would do nothing (`Needs`) must not, though it stays activatable. Show
   both sides: the turn passing unasked, and the stop it does make.
2. **Each ability's path.** Clicking it, dragging it out. A permanent with two
   or more non-mana activated abilities always opens the side-by-side chooser,
   unaffordable ones dimmed. Show it affordable and unaffordable.
3. **Costs and payments.** Mana, X, alternate and additional costs (kicker,
   evoke, overload, impending), life, sacrifice, tapping creatures.
4. **Prompts.** Each question the card asks: may, choose, target, order,
   discard. Show the prompt itself as the start of a moment, so the viewer
   answers it.
5. **What it makes and changes.** Tokens, emblems, counters, damage, control
   changes, attachments, tapping: the board after it, and the moment it happens.
6. **Zones.** Cards it exiles (and where they show: behind a permanent, down
   and to the left), cards it makes playable from exile or the graveyard, what
   it puts into a graveyard.
7. **The stack.** Responses to it and from it: countered, bounced in response,
   its targets gone, its source gone. The play button is for these.
8. **The other side.** The same card in the opponent's hands, where its view
   differs (a prompt the human only watches, a target on their permanent).
9. **Edges a player hits.** Empty hand, no legal target, nothing to choose,
   one choice (answered silently), more than one.

Skip a moment only when it cannot differ from another already there.

## Building boards

Boards should look like a real mid-game, so that a moment reads as play, not as
a test fixture:

- Both sides with lands (some tapped), nonland permanents, cards in hand, a
  graveyard, life totals off 20. Use cube cards the WU decks play.
- Share the opponent's side across moments with a helper (`table(...)` in the
  Converter's file).
- Give the opponent creatures only when combat is the point: the scripted
  opponent attacks with the first candidate it is offered.
- Untapped human creatures add a combat stop; tap them unless that is wanted.

## How scripts run

- Both seats' actions run in the order listed, across seats: a response waits
  until what it answers has been played.
- Short of its next action, a scripted seat passes priority, pays the first
  way, and answers one-answer questions; a decision it cannot answer fails the
  check, naming it.
- Once both seats' actions are taken, the human plays live. The opponent then
  passes and takes the first candidate of anything else.
- The human is under the smart stops, as in the app, and the session halts
  them at their own first main phase. Decisions the engine answers silently
  need no action.

## Judging

1. `just specs --check <slug>` until every moment passes. A check that fails on
   behavior the design wants is a bug to fix, not an expectation to loosen.
2. `just specs --shots <slug>` and look at every screenshot against the design
   philosophy in `.agents/ui-review/SKILL.md`: common case one gesture, the
   client does the bookkeeping, ask once in context, Arena as the reference,
   show don't tell, few words.
3. For what a still cannot show (a click, a drag, a chooser, a browser), drive
   the moment's page with Playwright WebKit from `ui/scripts`, as
   `ui/scripts/specs.mjs` does: open
   `<vite>/?live=<vite>/spec-api/m/<slug>/<moment>/<any-id>#/game`, click
   `.card-el[data-zone=battlefield][data-card='<name>']`, screenshot. Delete
   any probe script after.
4. Fix what is wrong, in the engine or the UI, inline. Fix it for every card
   that shares the moment, not just this one, and say which others it touched.
   A fixed engine bug gets its entry in `docs/bugs.md`.
5. Report what each moment shows, what you fixed, and any moment you could not
   build and why.
