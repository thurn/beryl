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

- **Only the card file**, plus `data/card-escalations.json` when you escalate.
  Never edit the vocabulary (`rust/crates/beryl_card`), the engine
  (`beryl_rules`, `beryl_kernel`), the renderer, tools, other cards, or any
  allowlist or divergence file under `data/`.
- **No new vocabulary.** Use only entries that exist. If the card needs one that
  doesn't, escalate (below).
- **No Rust body.** A definition is `static` and `const` data built from
  vocabulary constructors (`draw(YOU, 1)`, `Ability::triggered(...)`). Write no
  `fn` in the definition and no `impl`. Small `const fn` helpers inside the
  `scenarios` module are fine.
- **Never from Forge.** Don't read Forge's card scripts or code (plan K16). The
  brief's XMage class is a reference for what the card does, never code to
  translate.
- **Never weaken a check.** Don't skip a step, delete a failing scenario, or add
  an entry to `data/oracle-divergences.json` or any allowlist to make a check
  pass. A failing scenario, oracle or render diff means your definition is wrong
  until you have shown otherwise (see "Reading failures").

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
   - `printed::<CARD>` carries the mana cost, types, power and toughness, but
     not keywords. Add them with `.keywords(Keywords::FLYING)`, or
     `Keywords::FLASH.union(Keywords::FLYING)` for several.
   - Instructions in one ability go in order in `sequence(&[...])`.
   - Each rule the card relies on gets a comment citing the Comprehensive Rules,
     as a link to `https://yawgatog.com/resources/magic-rules/#R<rule without
     dots>` and a quote of the rule's text, like the comments in the model
     card. `just citations` checks the quotes against the pinned rules release.
5. **Write scenarios.** The file's `scenarios` module needs at least one
   hand-written `Scenario` for each ability, ending in `Action::assert`s on what
   the ability did. Copy the model card's scenarios. In scenarios:
   - Only implemented cards exist: cards with a file under `rust/cards`, or
     complete from the store, named by `names::<CARD>`, plus the stand-ins in
     `props::` (`props::CREATURE`, `props::ARTIFACT`, ...). Today only Island,
     Plains and Swamp are basic lands with files, so a card that needs {G} or
     {R} to cast cannot be cast in a scenario: escalate it with the mechanic
     "no implemented land makes {G}" (or {R}).
   - `Action::cast` puts the spell on the stack, and each `Action::Resolve`
     resolves the top object: a creature spell, then its enters trigger.
   - Give each player a library with a card or two more than the scenario
     draws, or a player loses for drawing from an empty library.
   - Generated scenarios (cast it, activate it) are added for you; they don't
     count as hand-written.
6. **Check fast.** Run `just card check "<name>" --no-oracles` (seconds once
   built) and fix the definition until build, tests, beryl and render pass.
7. **Check fully.** Run `just card check "<name>"`, which also plays the
   scenarios in XMage and Forge (about a minute; it waits for the shared oracle
   lock). Then run `just citations`.
8. **Commit and push.** Commit only the card file as `feat: <card name>` and
   run `enact push` in the worktree. If it exits with code 2, fix what it
   reports and run it again.

## Reading failures

- `build` fails: a type or name error. Read the vocabulary entry's signature in
  its file.
- `beryl` fails with `UnknownCard("<name>")`: a scenario uses a card that is not
  implemented. Use another card or a `props::` stand-in.
- `beryl` or `oracles` fails on an assertion: the definition or the scenario is
  wrong. Reread the Oracle text and rulings. If Beryl agrees with neither XMage
  nor Forge, Beryl is wrong.
- `render` fails: it prints the rendered text against the Oracle text, with
  `[-removed-]` and `{+added+}` words. Different instructions, amounts, targets,
  conditions or order mean the definition is wrong: fix it. When every other
  step passes and the only differences are the renderer's wording for the same
  instructions (for example `you gain 1 life [-and-] {+.+} draw a card`,
  "it" for the card's name, or one sentence split in two), the renderer lacks a
  template, which you may not change. Commit the card anyway and quote the diff
  in the commit body. `just card render --all` lists every such mismatch by
  cause.

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
