---
name: card-writing
description: Write one card's definition in Beryl from its brief, using only the existing card vocabulary (beryl_card), then check it with just card check and commit it, or escalate the card by recording the mechanic it needs in data/card-escalations.json. Use when asked to implement, write or add a card, or to work through cards one at a time.
---

# Writing one card

You write one card file, `rust/cards/<crate>/<set>/<card>.rs`, from the card's
brief and the existing vocabulary, check it, and commit it. If the vocabulary
cannot express the card, you escalate it instead. The vocabulary is the public
API of the `beryl_card` crate: the effects, triggers, costs, filters, keywords
and builders a card definition is made of.

## Rules

- **Only card files.** Write the card's file, plus the file of any card its
  scenarios need (see "Cards for scenarios"), plus `data/card-escalations.json`
  when you escalate and a `wording` entry in `data/card-text-allowlist.json`
  (see "Reading failures"). Never edit the vocabulary
  (`rust/crates/beryl_card`), the engine (`beryl_rules`, `beryl_kernel`), the
  renderer, tools, other existing cards, or any other allowlist or divergence
  file under `data/` (the exception is `just card diverge`, below).
- **Every clause, always.** Implement every sentence of the Oracle text, even
  one that no card in the decks makes happen today. "No Equipment is among the
  cards, so attaching never comes up" is how Thieving Skydiver shipped without
  its attach: the decks change, and the definition must already be right. If
  the vocabulary can't say a clause, escalate (below); never leave it out with
  a comment.
- **No new vocabulary.** Use only entries that exist. If the card needs one that
  doesn't, escalate (below).
- **No Rust body.** A definition is `static` and `const` data built from
  vocabulary constructors (`draw(you(), 1)`, `Ability::triggered(...)`). Write no
  `fn` in the definition and no `impl`. Small `const fn` helpers inside the
  `scenarios` module are fine.
- **Write it in the house style.** One `use crate::prelude::*;` at the top and
  one in `scenarios`. Name cards as `Card::Counterspell`, faces as
  `Printed::Counterspell`, tokens as `Token::Clue` and props as
  `Prop::Artifact`. A card of one face is
  `pub static CARD: CardDef = Printed::X.abilities(&[..]).card();`. Write each
  effect inline in `CARD`, and explain the abilities in `CARD`'s doc comment, one
  paragraph each; name an effect as a separate `static` only when it is used
  twice or is too long to read inline. `rust/cards/cards_2010/wwk/` has Jace,
  the Mind Sculptor as a model.
- **Never from Forge.** Don't read Forge's card scripts or code. The
  brief's XMage class is a reference for what the card does, never code to
  translate.
- **Never weaken a check.** Don't skip a step, delete a failing scenario, or add
  an entry to any allowlist to make a check pass, apart from a `wording` entry
  and an oracle fault as "Reading failures" describes. Never edit
  `data/oracle-divergences.json` by hand. A failing
  scenario, oracle or render diff means your definition is wrong until you have
  shown otherwise.

## Steps

1. **Worktree.** From the main checkout, run
   `enact worktree create <card-slug>` (`enact` is defined in the root
   `AGENTS.md`) and work only in the path it prints.
2. **Brief.** Run `just card brief "<name>"` and read all of it: the Oracle
   text, the rulings, the vocabulary entries, the nearest implemented cards and
   the XMage class. If it has an "Escalation" section, the card is blocked:
   stop, unless the mechanic it names now exists in `beryl_card`.
   - If it says "Complete from the store", the card needs no file. Stop.
3. **Start the file.** Run `just card new "<name>"`. It writes the file with its
   generated header (never edit the header) and an empty definition, and names
   the nearest implemented card of the same type to read as a model.
4. **Write the definition.** Read the model card and the nearest cards from the
   brief, and copy their shape. For each ability in the Oracle text, find the
   vocabulary entries that say it (the brief lists the likely ones with their
   files; read the entry's doc comment there). Things that trip writers up:
   - `Printed::<Card>` carries the mana cost, types, power and toughness, but
     not keywords. Add them with `.keywords(Keywords::FLYING)`, or
     `Keywords::FLASH.union(Keywords::FLYING)` for several.
   - Instructions in one ability go in order in `sequence(&[...])`.
   - Each rule the card relies on gets a comment citing the Comprehensive Rules,
     as a reference, `[CR 118.12a]`, with the link
     `[CR 118.12a]: https://yawgatog.com/resources/magic-rules/#R11812a` at the
     end of the doc comment, and a quote of the rule's text, like the comments in
     the model card. `just citations shorten <file>` turns inline links into
     references, and `just citations` checks the quotes against the pinned rules
     release.
5. **Write scenarios.** The file's `scenarios` module needs hand-written
   `Scenario`s that, between them, carry out every part of every ability: each
   branch of an `if_then` or `if_else`, the inside of each `may` (answered yes)
   and each `unless`. Each ends in assertions on what it did, written with the
   `ME` and `THEM` shorthands (`ME.casts(card)`, `ME.yes()`,
   `THEM.has(Zone::Hand, card, 1)`; `beryl_scenario_types`' `player.rs` lists
   them all). The test `every_mutant_of_a_card_is_killed_by_its_scenarios`
   changes one part of the definition at a time (a step dropped, a player or an
   amount changed, a "may" removed) and names each change that no scenario,
   hand-written or generated, tells from the card. Fix one by adding a scenario
   whose outcome it changes, or a witness to the generator
   (`beryl_cards/src/derive/witness.rs`) when the change is to a vocabulary
   entry every card using it shares. A part is only checked against XMage and
   Forge if a scenario shows what it does. Copy the model card's scenarios. In scenarios:
   - Only implemented cards exist: cards with a file under `rust/cards`, or
     complete from the store, named by `Card::<Card>`, plus the stand-ins in
     `Prop::` (`Prop::Creature`, `Prop::Artifact`, `Prop::Equipment`, ...).
     If a scenario needs a card nobody has implemented, implement one (see
     "Cards for scenarios") rather than leave the part untested. Today only
     Island, Plains and Swamp are basic lands with files, so a card that needs
     {G} or {R} to cast cannot be cast in a scenario: escalate it with the
     mechanic "no implemented land makes {G}" (or {R}).
   - `ME.casts(card)` puts the spell on the stack, and each `Resolve` resolves
     the top object: a creature spell, then its enters trigger.
   - A player whose library the scenario doesn't set starts with one Island.
     Give a player who draws more than once a longer library, or they lose for
     drawing from an empty one. `.library(P0, &[])` is an empty library.
   - `.battlefield(P0, &[..])` takes plain card names; tapped permanents,
     tokens and permanents with counters go in `.permanents(P0, &[..])`.
   - Generated scenarios (cast it, activate it) are added for you; they don't
     count as hand-written.
6. **Check fast.** Run `just card check "<name>" --no-oracles` (seconds once
   built) and fix the definition until build, tests, beryl and render pass.
7. **Check fully.** Run `just card check "<name>"`, which also plays the
   scenarios in XMage and Forge (about a minute; it waits for the shared oracle
   lock). Then run `just citations`.
8. **Commit and push.** Commit the card file, with any card file written for
   its scenarios, as `feat: <card name>`, and run `enact push` in the worktree.
   If it exits with code 2, fix what it reports and run it again. If a test
   outside your card files fails, and passes once your files are moved aside,
   the card exposed a bug in that test or the engine, not in your definition.
   Leave the card uncommitted, file a bead naming the test, the card and the
   failure, and stop.

## Cards for scenarios

It is fine, and expected, to implement a card only so that a scenario can use
it. We can't wait for every card to be written before a definition has good
tests. When a part of your card needs another card to happen (an Equipment to
steal, a creature with a particular ability, a spell to counter):

- Use a prop from `Prop::` if one fits.
- Otherwise pick the simplest real card in the card store that sets the
  situation up (`just card brief` works on any card), and write it as you write
  any card, in its own file with its own scenarios, checked with
  `just card check`. A real card, not an invented one, so that XMage and Forge
  can play the same scenarios. Leonin Scimitar was written this way for Thieving
  Skydiver: it is in no deck. Its doc comment says that it was implemented for
  scenarios.
- If other cards will need the same role, add it to `props.rs` in
  `beryl_card_shared`, both as a constant and in `Prop`, with a doc comment.
- Name another card in your scenario with `Scenario::interaction`, unless it is
  a prop.

## Reading failures

- `build` fails: a type or name error. Read the vocabulary entry's signature in
  its file.
- `beryl` fails with `UnknownCard("<name>")`: a scenario uses a card that is not
  implemented. Use another card or a `Prop::` stand-in.
- `beryl` or `oracles` fails on an assertion: the definition or the scenario is
  wrong. Reread the Oracle text and rulings. If Beryl agrees with neither XMage
  nor Forge, Beryl is wrong.
- `oracles` fails in one oracle only, while the other passes the same scenario,
  and the Comprehensive Rules say the failing oracle is wrong (XMage requires a
  search to find a card that CR 701.23b lets the player not find): record it
  with `just card diverge "<name>" "<scenario>" <xmage|forge> --cr <rule>
  --reason "<what that oracle does>"`, then run `just card check` again. The
  command plays the scenarios itself and refuses unless that oracle fails, the
  other passes and the rule is in the pinned release; it fills in the rule's
  quotation. Never edit `data/oracle-divergences.json` by hand: the
  `divergences` check fails a hand-written entry, and the oracle run fails an
  entry whose oracle now passes.
- `render` fails: it prints the rendered text against the Oracle text, with
  `[-removed-]` and `{+added+}` words. Oracle words the render lacks (a `[-...-]`
  with no matching `{+...+}`) mean a clause is missing: implement it, or
  escalate it as partial (below). Different instructions, amounts, targets,
  conditions or order also mean the definition is wrong: fix it. Only when the
  definition does exactly what the card says and the difference is the
  renderer's wording for the same instructions (for example
  `you gain 1 life [-and-] {+.+} draw a card`, "it" for the card's name, or one
  sentence split in two) may you pin it: add an entry to
  `data/card-text-allowlist.json` with `"reason": "wording"`, a `note` saying
  why the two texts mean the same thing, and the exact `diff` lines that
  `just card render "<name>"` prints. The `card-text` check in CI fails on any
  difference that isn't pinned exactly. A missing clause is never `wording`.

## Escalating

Escalate when the vocabulary cannot express an ability: no entry in
`beryl_card` says it. To tell a missing mechanic from your own mistake:

- Search the vocabulary itself, not only the brief's list, which shows the ten
  closest entries by word overlap. Search `rust/crates/beryl_card/src` for the
  ability's key words and for the rule it falls under (`grep -rn "CR 702.19"`),
  and look at how the nearest cards say similar text.
- If an entry exists but you can't make it compile or behave, that is a writing
  error: keep trying with the entry's doc comment and the cards that use it.
- If no entry says it, or a check fails because the engine does something the
  card's text doesn't say and no entry changes that, the mechanic is missing.

To escalate:

1. Delete the card file.
2. Add the card to `data/card-escalations.json`, keyed by its name as the brief
   prints it, keeping the keys sorted:

   ```json
   {
     "Card Name": {
       "mechanic": "the missing mechanic in one phrase, e.g. regeneration",
       "rule": "CR 701.19",
       "text": "the ability's Oracle text"
     }
   }
   ```

3. Commit only that entry as `chore: escalate <card name>`, with the missing
   mechanic in the body, and push it as in step 8. `just card brief` now shows
   the entry to the next writer.

When the card already has a file (never delete a card implementation), or only
one clause of a card you are writing is out of reach, escalate it as partial
instead: keep the file with everything the vocabulary can say, and

1. add the card's entry to `data/card-escalations.json` with `"partial": true`
   and the left-out clause as its `text`;
2. pin the render diff in `data/card-text-allowlist.json` with
   `"reason": "escalated"`, its `note` naming the missing mechanic;
3. say in the definition's comment which clause is missing and point to the
   escalation.

`just card brief` then shows the card as partly written, and the `card-text`
check holds the gap to exactly those words until someone finishes it.
