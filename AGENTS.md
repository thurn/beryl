# Working on Project Beryl

Project Beryl is a research project to build a Magic: The Gathering AI that
plays as well as a strong human player, with a native Rust 3D client for macOS,
Windows, iOS and Android. Games are two-player, best of three with sideboards,
between fixed decklists. There is no drafting.

The goal is 50,000+ card definitions. The rules engine and the AI are both
expected to handle every card that is implemented. The WU Control mirror
(`decks::WU_CONTROL`, from `docs/sample-decks/`) is the cheapest test bed, not a
limit on what either must play well. Never delete a card implementation.

The repository is public. Never commit logs, screenshots, transcripts or other
output from a run.

This file holds the standing rules for every change. `engine/`, `client/`,
`data/`, `tools/`, `ignite/` and `efficiency/` each have an `AGENTS.md` with the
reference material for that directory: read it before you change anything there.
Procedures are skills in `.agents/skills/`.

## Writing for readers

This applies to code comments, commit messages, tickets and reports, and to this
file.

- Write for a reader who knows Magic and Rust but has not read the code you just
  changed. If a sentence only makes sense after reading the diff, rewrite it.
- Start with what someone saw (the symptom), then why it happened, then what you
  changed.
- Refer to things by their identifier in backticks, or describe them in plain
  words. Don't invent metaphors ("dressed", "lifted", "the door"). When you use
  a project term, say briefly what it is the first time.
- Write complete sentences with articles and ordinary verbs, one idea per
  sentence. The rule in UI about halving labels is for on-screen text only,
  never for prose.
- Check: would a colleague reading it have to ask "meaning what?" If so, put the
  answer in the text.

For example, instead of:

> The phone dressed the cards the opponent looks at as the pile's top, at a
> depth over it, so the pile showed no card beneath them.

write:

> On the phone layout, cards the opponent was looking at from the top of their
> library were drawn as the library's top card, stacked above the pile, so the
> library looked empty underneath them.

## Code structure

Every design has to hold at 50,000 cards. A mechanic that exists for one card
means a general abstraction is missing, so add the abstraction.

- No card names in generic code. A card's behavior is data in the card
  vocabulary (`beryl_card`) that one interpreter runs. When the vocabulary can't
  say a clause, add a vocabulary entry that other cards can use.
- A card definition implements every clause of its Oracle text, including one
  that no deck makes happen yet. A clause the vocabulary can't say yet is
  escalated in `data/card-escalations.json`, never left out with a comment.
- State that a card creates goes in a store, not in a new field on `Player`,
  `Object` or `State` (K4 in `docs/engine-architecture.md`).
- A rule or card that a game does not use costs that game nothing. The count
  gate (`engine/crates/beryl_bench/count-gate.toml`) measures this and says when
  its limit may rise.
- All rules engine code has comments that cite and quote the part of the
  Comprehensive Rules it implements (`engine/AGENTS.md` gives the format). A
  card file is not rules engine code: it cites a rule only where the card relies
  on something no vocabulary entry says, such as a ruling about an unusual
  interaction.
- The kernel, rules and card crates have no runtime dependencies. When you add a
  dependency elsewhere, its `Cargo.toml` comment says what it replaces.
- Forge is used only as a test oracle and, in `just census`, as a count of which
  mechanics its scripts use. No card data, vocabulary or design may come from
  its scripts or code.
- Don't write procedural macros. Use `macro_rules!` only where a `const fn`
  can't remove the repetition.

The `source` checks hold the mechanical rules, such as the 800-line limit on a
source file, and each explains itself when it fails. `engine/AGENTS.md` lists
the ones for engine code.

## Testing your work

Use the cheapest test that covers your change, and run only the checks the
change needs. Agents never open a window.

1. Logic: `ignite test <names>` runs the Rust tests you name in your tree. Test
   engine and AI behavior here, not through the app. To write a card, follow the
   `card-writing` skill.
2. UI: `just client tree`, `diff` and `check` inspect what the player sees, and
   `png` and `film` draw it. Use `just client spec <card>` for a card and
   `just client test <prefix>` for a recording or story.
3. Desktop and phone: `just smoke`, `just ios-smoke` and `just android-smoke`
   start the app without a window (`client/AGENTS.md`).

- Prefer fast tests of what a player or caller can observe (a decision, a
  protocol message, a saved file) over unit tests of private code, which mostly
  detect change rather than bugs.
- Tests fail deterministically, so delete flaky ones. A timeout only catches a
  hang: a slow test never fails for being slow. Avoid slow integration tests,
  and prefer a fast test that keeps most of their value.
- "No card with this behavior is implemented yet" is never a reason to skip a
  test. Implement a card that has it, even one in no deck, and pick a real card
  so that XMage and Forge can play the same scenario. File a ticket under your
  epic only for a really major item.
- Benchmarks follow `docs/benchmarking.md`. Tournaments (`just tournament`,
  `just play` and `just review --agent`) use the whole machine for minutes to
  hours, so read "Tournaments" in `engine/AGENTS.md` before running one.

## Checks

Ignite's checks are declared in `ignite/checks/*.toml`, with the reason for each
setting in a comment beside it. `ignite guide flaky-checks` and the `cifix`
skill hold the rules for a ticket about a check.

- A UI bug fix adds the broken state as a fixture or spec moment, so the generic
  UI checks (overlap, off-screen, reachability, duplicate objects, motion) cover
  it. If the bug is a new kind of problem, extend one of those checks. Use a
  focused native integration test only when none of the generic checks can
  express the property, and explain why in a comment (K20 in
  `docs/engine-architecture.md`).
- The counts in an allowlist under `data/` may only go down.

## UI

MTG Arena is the reference for Beryl's game flow, the order and timing of
events, the table layout and the wording (K22 in `docs/engine-architecture.md`).
Before you design or change a prompt, animation, arrow or label, look up how
Arena does it with the `arena-video` skill. A screenshot or filmstrip answers
questions that a description can't.

- Animations follow Arena in what they show and when: what changed, who did it,
  where it went, in what order, and what stays still. No review is required to
  land an animation: `just client check` checks motion in a replay, and the
  `motion-review` skill runs only when the user asks for it.
- Beryl keeps its own visual style: colors, fonts, icon artwork, card frames,
  and full-size cards that tap.
- Beryl differs from Arena only where [UI Conventions](docs/ui-conventions.md)
  lists a numbered exception. Check that list before copying Arena, and before
  "fixing" something that differs from it. That page is generated from
  `data/ui-conventions.json` by `just conventions`: edit the JSON, never the
  page.
- The user's bug report takes priority over Arena and over any earlier report.
  Build what it asks for, and change or remove the matching exception in the
  same change.
- Explicitly confirm with the user before adding a numbered exception to UI
  Conventions. Ask in chat, and add it only after a clear yes. Never infer one
  from an old report or a design choice nobody asked for.
- Anything the recordings don't show is built from animations and prompt types
  Beryl already has, never a new look, and listed in UI Conventions under "Not
  seen in Arena".
- On-screen text uses Arena's wording exactly. Where Arena shows no label, cut
  your first draft of the label to half as many words.
- Never give players priority when they would only pass, such as when their own
  spell is alone on the stack.

## Ignite

Ignite builds, checks and lands every change, and holds the tickets
(`docs/ignite.md`). Run `ignite prime` before your first change and follow what
each command prints. `ignite guide <topic>` holds the rules for one situation.

- Every change, whoever makes it, starts with `ignite start <ticket>` (or
  `ignite start "<summary>"` for a task with no ticket) and lands with
  `ignite land`. Work only in the build tree it prints. Never edit or commit in
  the main checkout, and never push to `master` or `staging` yourself.
- No plan, ticket or skill includes a step that waits for the user's approval.
  Where his judgement might be wanted, go ahead with a sensible default and
  record the choice on your ticket. Never file a ticket asking him to look at
  something.
- Ask the user in chat, and go ahead only after a clear yes, in three cases: the
  count gate (K1 in `docs/engine-architecture.md`), adding a UI exception (see
  UI), and filing a ticket whose work includes more than 30 minutes of anything
  other than writing code, such as simulations, tournaments, collecting logs,
  manual QA or monitoring. That covers a ticket filed as part of an epic.
  Running a check or test suite while writing code does not count.
- **No Fable without an explicit ask**: never label a ticket `tier:fable`, and
  never start a session or subagent on a Fable model, unless the user has
  explicitly asked for it. When launching more than one subagent at a time,
  always use Sonnet.
- Whoever files an epic settles its design: the design and the contract between
  its tickets are written in the epic, and no ticket's work is to figure out the
  design. File the epic with allocation zero, because the user decides when the
  worker pool starts on it (`/ignite <plan>` and the `ignite` skill).
- Each ticket names the files, helpers and tests it needs by path, so that its
  session can read them all in its first message, and has a model tier:
  `tier:opus` for design work, `tier:sonnet` for mechanical work. An epic gets
  no tier label, because its children inherit it.
- The worker pool can pick up a standalone ticket within minutes, so land what
  it depends on first or give it the dependency. When you file a ticket for
  something you found while working with the user, offer to fix it in the
  current session.

## Layout

- `engine/`: the Cargo workspace of the rules engine and the AI, as library
  crates under `engine/crates/` that build and test without the client. Each
  card is one file, `engine/cards/<crate>/<set>/<card>.rs`. Adding a card means
  adding a file: build scripts generate every list of cards, and each card's
  mana cost, type line and header comment, from the card store.
- `client/`: the native client's Cargo workspace, with the desktop entry point
  and the Swift and Kotlin hosts. Its layout and fixture data are in
  `data/client/`.
- `data/`: data files, each described in `data/AGENTS.md`. The card store,
  `card-store.tsv`, is built by `just cards` and embedded in the app.
- `tools/`: `by`, the Python CLI behind the justfile. `just` lists the commands.
- `efficiency/`: the efficiency dashboard, a standalone Python project.
- `ignite/`: Ignite's own Cargo workspace. `ignite.toml` and `ignite/checks/`
  say what it knows about this repository.
- Outside the repository, `$BERYL_CACHE` holds the oracle builds, the client's
  recording packs and screenshots, and `$BERYL_ARENA` holds the indexes of the
  MTG Arena recordings that the `arena-video` skill reads. Name these by their
  variables: the agents run on macOS and Windows, so never write a path that
  exists on only one of them.

## Conventions

- Commits use short Conventional Commits subjects, e.g.
  `feat: mulligan decisions`, of at most 60 characters (the `commit` check
  enforces this). Add a body for every fix and for any change whose reason a
  reader couldn't guess from the subject. The body follows "Writing for
  readers": what was wrong as someone would have seen it, why, and what changed.
- `just fmt` formats everything. Scripts are Python in `tools/`, using only the
  standard library unless a dependency is clearly worth it.
- Send reads together. Every request reads the whole conversation again, so work
  out every file and search you need and send them in one message as parallel
  tool calls, or as one shell command.
- When you fix a rules engine bug, including one in your own code from the same
  session, add an entry to `docs/bugs.md`: a short description of the bug, and
  the guard that now stops its class (a lint, a type, a check or a differential
  test), added in the same change where you can, or why none can. Link the
  fixing commit's hash when the bug wasn't found and fixed in the same session.
- A change made to save time or tokens adds its entry to
  `data/efficiency-changes.json` in the same commit.
- Never put a ticket number or a reference to a plan in code, comments or
  documentation, including `docs/`, the `AGENTS.md` files and skills. A ticket
  closes and its number then tells a reader nothing, so say what the ticket
  would have said, and delete such references from code you touch. Ticket
  numbers belong in commit messages, landing notes and other tickets.
- Agents don't add Markdown files unless explicitly asked. README.md, the
  `AGENTS.md` files and `docs/` are the only Markdown, and plans, designs and
  notes go in tickets. Each `AGENTS.md` has a one-line `CLAUDE.md` beside it
  that imports it.
- Before you add a line to this file, check three things. It is a standing rule
  for every change, not a remark the user made about one case. No check, command
  output or skill already says it at the moment it matters. A sensible engineer
  would not do it anyway. Reference material for one area goes in that
  directory's `AGENTS.md`, and a procedure goes in a skill. When you add a line
  here, look for one to remove.
