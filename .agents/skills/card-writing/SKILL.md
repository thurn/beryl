---
name: card-writing
description: Write or finish one Beryl card from its drafted definition and Oracle text. Verify it with generated or focused hand-written scenarios, or record the mechanic it needs. Use when asked to add or implement a card or work through cards one at a time.
---

# Writing one card

## Drafted-card loop

1. Use the build tree of your ticket. If you have none, run
   `ignite start "Card: <name>"` and work only in the tree it prints.
2. Run `just card brief "<name>"`. Read the Oracle text, draft and rulings.
   If it says "Complete from the store", the card needs no file. If it shows
   an escalation, proceed only if the mechanic it names is now available.
3. Run `just card new "<name>"` for an unwritten card. It writes
   `rust/cards/<crate>/<set>/<card>.rs` with the generated header and draft,
   then prints its render status. Keep the header as generated. Check the
   draft sentence by sentence against the Oracle text and rulings, including
   keywords, costs, targets, conditions and order. Use "Composing a
   definition" for gaps. After editing, run `just card render "<name>"`
   again and read the diff.
4. Remove any empty `scenarios` stub from `card new`. Run
   `just card check "<name>" --no-oracles`, then `just card check "<name>"`.
   Read the `generated` and `oracles` lines.
   Follow "Checking coverage" and "Recording rulings" below.

**Stop rules for the drafted tier.** Hand the card back to `tier:sonnet` if
a surviving mutant needs a scenario, a clause needs vocabulary that Beryl
lacks, a compiler error comes from outside the draft, or the render still
differs after three attempts to fix it. Give the next writer the card name,
the stop rule and the first failing output, including the mutant or Oracle
clause where applicable. Use the same handoff if there are no generated
scenarios or the card needs a hand-written scenario for oracle coverage or
a ruling. Keep any useful draft edits in the handoff.

## Which cards to write

Use the classification from `just set plan <code>`.

| Tier | Work |
| --- | --- |
| `store` | Beryl builds the card from its store record. |
| `drafted` | `tier:light` checks the complete draft. |
| `composed` | `tier:sonnet` fills gaps using the existing vocabulary. |
| `vocabulary` | One `tier:opus` ticket per group that needs the same new vocabulary entry. |
| `mechanism` | A `tier:opus` design ticket for the missing engine support. |

## Composing a definition

The vocabulary is the public API of `beryl_card`. If the draft has gaps, read
the brief's unresolved clauses, nearby cards and the doc comments of the
vocabulary entries it names. Compose every clause, even if no current deck
can make it happen. Escalate a clause that needs a new variant or interpreter
arm. Use XMage's class to understand behavior. Never use Forge's scripts or
code as a source for a definition.

- Use `use crate::prelude::*;` at the top and in `scenarios`. Use `Card::X`
  for cards, `Printed::X` for faces, `Token::X` for tokens and `Prop::X` for
  scenario stand-ins. See `rust/cards/cards_1993/lea/counterspell.rs` for a
  definition with generated coverage.
- Build the definition from static and const data using vocabulary
  constructors. Keep straightforward
  expressions such as `draw(you(), 3)` inline. A small `const fn` in
  `scenarios` can share setup. A documented prelude function in
  `rust/crates/beryl_card/src/phrases.rs` can name a reusable composition.
  Pass details that vary by card as arguments.
- A single face usually starts with `Printed::X.abilities(&[...]).card()`.
  `Printed::X` carries printed characteristics. Add keywords explicitly with
  `.keywords(...)`. Put instructions in one ability in order with
  `sequence(&[...])`.
- Cite the Comprehensive Rules only for a card-specific ruling. Quote the
  pinned text and use a reference such as `[CR 118.12a]`, with
  `[CR 118.12a]: https://yawgatog.com/resources/magic-rules/#R11812a` at the
  end of the doc comment. Run `just citations` when adding a citation.

Keep edits to card files, shared compositions in the prelude, shared scenario
props where needed, and the ledger, escalation and render entries described
here. The divergence command can record a verified oracle fault. Engine,
vocabulary variant, renderer and tooling changes belong in their own work.
Keep each check in place and fix the definition or scenario it exposes.

## Checking coverage

The `generated` line reports which mutants the generated scenarios alone
kill. A mutant changes one part of the definition. A scenario kills it when
the change alters what the scenario observes. If generated scenarios kill all
mutants, at least one passes in XMage or Forge, and neither oracle reports an
unexpected failure, omit the `scenarios` module unless a ruling needs one.

For each surviving mutant, write a scenario whose outcome changes under that
mutation. If all generated scenarios are unsupported by the oracles, write a
scenario an oracle can play. Use the smallest board and sequence of decisions
that shows the difference, and assert the result using `ME` and `THEM`. See
`rust/crates/beryl_scenario_types/src/player.rs` for their methods.

- `ME.casts(card)` puts the spell on the stack. Each `Resolve` resolves one
  object, so an enters trigger needs a separate resolve from its spell.
- A player with no explicit library starts with one Island. Set a longer
  library if they draw more than once.
- Use `.battlefield(P0, &[...])` for cards and `.permanents(P0, &[...])` for
  tapped permanents, tokens or permanents with counters.

## Recording rulings

Write a scenario for a card-specific ruling that can be demonstrated and
that the vocabulary or rules library does not already cover. Reuse a
scenario if it already shows the ruling. List the card in
`data/rulings-ledger.json` when it has such a ruling or the task calls for an
audit. Otherwise leave an unlisted card out of the ledger.

Run `just card rulings "<name>" --add` to add draft entries for every
ruling. Complete each entry with exactly one of these fields.

- `scenario` is a list of exact scenario titles from the card file or the
  rules library in `beryl_cards/src/library/`.
- `covered` names a documented vocabulary entry or rules-library scenario
  that already shows the ruling.
- `untestable` explains why a scenario cannot show the ruling. If it needs
  an unimplemented card, name it.

## Cards for scenarios

Use a `Prop::` stand-in when its contract in
`rust/crates/beryl_card_shared/src/props.rs` promises what the scenario needs.
Describe the role in the scenario and use assertions for stats that matter.
If the scenario needs a behavior beyond the contract, use a named card and
`Scenario::interaction`.

Choose an implemented or store-complete card first. If none fits, implement
the simplest real card that sets up the case, even if it is outside the deck
pool, and run its full card check too. Leonin Scimitar was implemented to
test Thieving Skydiver. If several scenarios need the same role, add a
contract, an `ALL` entry and a `Prop::` name in `beryl_card_shared`. Prefer a
store-complete card and name the role rather than the card.

## Finish

Once the card is complete, run the fast and full card checks again after
changes to its definition or scenarios. Commit as `feat: <card name>` and run
`ignite land -m "<what the card needed>"`. If it exits with code 2, fix what it
reports and land again. If a check outside your files fails, verify that
your change exposes it, then file a ticket with the card, check and failure.
Keep the card out of staging until the check passes.

## Reading failures

- `build` fails. Read the first error and the vocabulary signature it points
  to. For a drafted-tier compiler error outside the draft, hand it off.
- `beryl` reports `UnknownCard`. Use an implemented card, a store-complete
  card, or a prop. If it reports no scenarios before reaching `generated`,
  the card needs a scenario. For an assertion failure, check both the
  scenario and the definition against Oracle text and rulings.
- `generated` lists surviving mutants. Use the node named in the output to
  choose what the scenario must observe. This line still describes generated
  coverage alone after you add hand-written scenarios. The full mutation test
  `every_mutant_of_a_card_is_killed_by_its_scenarios` checks all sources.
- `oracles` fails in one engine while the other passes the same scenario.
  If the pinned rules establish an oracle fault, run
  `just card diverge "<name>" "<scenario>" <xmage|forge> --cr <rule> --reason "<text>"`
  and check the card again. The command verifies both outcomes and includes
  the rule quotation. Use it for changes to `data/oracle-divergences.json`.
- `render` shows `[-missing-]` and `{+extra+}` words. Fix differences in
  instructions, amounts, targets, conditions and order. If the instructions
  mean exactly the same thing and only the phrasing differs, add a
  `"reason": "wording"` entry to `data/card-text-allowlist.json` with the
  exact `diff` lines from `just card render "<name>"` and a `note` explaining
  the equivalence. A missing clause requires a definition change or a partial
  escalation.
- `rulings` fails in the names lane. Run `just card rulings "<name>"` to
  compare the ruling identities and evidence. Fill any unfinished entries
  and fix stale hashes or scenario titles to match the current ruling and
  its coverage.

## Escalating

Before deciding that a mechanic is missing, search
`rust/crates/beryl_card/src` for its key words or Comprehensive Rules number
and read similar cards. The module map in
`rust/crates/beryl_rules/src/lib.rs` points to the rules implementation.
A compile or assertion error alone does not establish a gap. Escalate if no existing composition expresses the clause.

Add an entry to `data/card-escalations.json` in card-name order. Use the
card's exact name and the unsupported ability's Oracle text.

```json
{
  "Card Name": {
    "mechanic": "the missing mechanic in one phrase",
    "rule": "CR 701.19",
    "text": "the ability's Oracle text"
  }
}
```

If there is no implementation to keep, discard the new scratch file and
commit the entry as `chore: escalate <card name>`, with the mechanic in the
commit body. If the card already has a file or only one clause of the new
definition is out of reach, keep the working clauses. Set `"partial": true`
on the escalation, pin the exact render diff in
`data/card-text-allowlist.json` with `"reason": "escalated"` and a note
naming the mechanic, and point from the definition's comment to the
escalation. Land it so the next writer can see the escalation in the brief.
