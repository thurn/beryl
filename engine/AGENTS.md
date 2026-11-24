# Working in `engine/`

This adds to the root `AGENTS.md`, whose rules all apply here.

The native client is a separate workspace in `client/` with its own build and
checks; see `client/AGENTS.md`.

In a build tree, the `cargo` on `PATH` runs through Ignite, which gives the
command cores from the daemon and records it, so a build may wait a moment for
cores before it starts. `ignite run -- <command>` does the same for any other
heavy command.

## Crates and card files

- `crates/beryl_shell` resolves desktop data paths and named profiles.
  `crates/beryl_session` runs the engine session and supplies each seat's view
  and animation script. See `docs/engine-architecture.md` §16.
- `crates/beryl_kernel/src/lib.rs` describes the state and mutator contract.
  `crates/beryl_rules/src/lib.rs` maps rules to their modules. Use
  `just citations coverage` to see which rules lack an ownership declaration.
- `crates/beryl_protocol` owns the shared views, scripts and IPC definitions,
  including the manual table commands in `src/ipc/table.rs`.
  `crates/beryl_card_records` supplies shared printing and art records.
- `crates/beryl_table` owns manual state, actions, permissions, assists and
  saves. `crates/beryl_table_session` projects each seat's view and script and
  runs solo, hosted and joined tables. Run their tests with
  `just test-rust -p beryl_table -p beryl_table_session`.
- `crates/beryl_link` supplies the handshake and in-memory and loopback TCP
  links. `crates/beryl_link_iroh` supplies the optional internet transport. The
  `iroh` check family runs its tests over loopback with the relay disabled.
- Each card is one file, `engine/cards/<crate>/<set>/<card>.rs`, placed by the
  set it was first printed in. `just card path <name>` prints the path. Card
  crates depend only on the shared vocabulary crate.
- Nobody maintains lists of card files by hand. Build scripts
  (`beryl_card_build`) generate each set crate's module list and tables,
  `beryl_cards`' registry from them, and `beryl_card_names`' `names` from the
  card store. A creature whose rules text is only keywords the engine has needs
  no file: its set crate's build writes it from the card store
  (`just card path <name>` says so), and `just card crates` writes the set
  crates.
- Nobody types a mana cost or type line either. Each card face starts from a
  `Printed::` constant generated from the card store, and each card file's
  header comment is generated too (`just card header`). Card files name cards as
  `Card::Counterspell` and import everything with `use crate::prelude::*;`; code
  outside the card files uses `beryl_card_names::names` (also re-exported as
  `beryl_cards::names`).
- The test decks are `decks::WU_CONTROL` and `decks::VINTAGE_WU_CONTROL`.

## Rules the checks hold

Each of these checks explains itself when it fails.

- `file-length`: no source file is over 800 lines, with exceptions in
  `data/file-length-allowlist.json`.
- `rule-ownership`: kernel and rules modules declare the rules they implement in
  an `Implements:` header and mark cross-references with `see`.
- `card-names`: no card names in generic code, with exceptions in its allowlist.
- `two-player`: rules code and card code never assume there is exactly one other
  player. They ask the game state for a player's opponents, the defending
  players or the next player in turn order ("Players" in `beryl_kernel`'s
  contract).
- `board-door`: rules code and card code never access the board directly through
  `.board()` or `board::Board`. They change the game state through mutator
  functions ("Mutators" in `beryl_kernel`'s contract).
- `discarded-push`: kernel, rules and card code never ignore a failed push onto
  a fixed-capacity list. If you can argue the list can't overflow, use
  `Queue::push_bounded`, which checks that claim in debug builds. Otherwise
  propagate the `Full` error, which makes the game switch to the slower `Large`
  state representation, which has room to grow.

## Testing rules and cards

- `just test-rust` runs the workspace's tests.
- Every package lists `beryl_workspace_hack` among its dev-dependencies, so its
  tests can build alone with the whole workspace's dependency features. The
  script, scenario and CLI builds select the workspace's libraries in the test
  profile so they share those artifacts too. After changing a dependency or its
  features, run `just workspace-hack`. The tests in
  `tools/tests/test_workspace_hack.py` and `tools/tests/test_cli_build.py`
  compare the host and target features.
- A test that names a real card lives in the crate's `tests/` directory and uses
  only its public API; a unit test under `src/` uses only
  `beryl_card_shared::props`. Each crate's integration tests are one binary,
  `tests/all/main.rs` with one module per file, because every test binary links
  the crate and its dependencies again after an edit.
- Only leaf crates (the benchmark, the command line and the client, listed in
  `data/card-leaves-allowlist.json`) depend on `beryl_cards` or a set crate as a
  normal dependency, and no `src/` file of any other crate names them (the
  `card-leaves` check). Cargo rebuilds every dependent of a changed crate, so
  this keeps a card edit from rebuilding the kernel, rules, search and session
  crates. A test of another crate that needs real cards goes in its `tests/`
  directory with `beryl_cards` as a dev-dependency.
- `just oracle <card names>` plays a card's generated, rules-library and
  hand-written scenarios against XMage and Forge. It needs `just oracle-setup`
  once per machine.
- Rules engine code cites the Comprehensive Rules by linking to
  https://yawgatog.com/resources/magic-rules/ and quoting the relevant text from
  the rules release pinned in `data/rules-version.json`. In a doc comment, cite
  as a reference, `[CR 118.12a]`, and define its link at the end of the comment.
  `just citations` checks every citation and quote in the code against the
  pinned release. `just citations migrate <date>` moves the code to a newer
  rules release: it renumbers the citations it can match and lists the ones that
  need a human decision. `just citations shorten <paths>` rewrites the inline
  links in doc comments as references, `[CR 118.12a]`, with the links defined at
  the end of each comment.
- `just fuzz` plays long random games, and `just exhaust` tries every sequence
  of decisions near the end positions of the scenarios.
- Follow the [card-writing skill](../.agents/skills/card-writing/SKILL.md) to
  add a card. `just card brief <name>` shows its Oracle text, a Rust draft,
  rulings and references for unresolved clauses. `just card new <name>` writes
  the generated header and draft and reports how it renders.
- `just card check <name>` builds the card, checks its scenarios and compares
  the rendered text with Oracle text. Use `--no-oracles` for a fast pass. The
  `generated` line names any mutants that generated scenarios miss. When
  generation kills every mutant and passes the oracle bar, the card needs a
  hand-written scenario only for a card-specific ruling. The `card render`
  subcommand prints the text comparison for one card or the match rate over
  every card file with `--all`.
- `just card rulings <name> --add` drafts ledger entries for a card audit.
  Complete each in `data/rulings-ledger.json` with a scenario, existing coverage
  or a reason the ruling is untestable. The `rulings` step in the names lane
  checks every entry for a listed card.
- Two checks hold a definition to its card. The `card-text` step of the rust
  lane renders every card file and fails on any difference from its Oracle text
  that `data/card-text-allowlist.json` doesn't pin exactly. The mutation check
  `every_mutant_of_a_card_is_killed_by_its_scenarios` (in `beryl_cards`) builds
  each mutant of a card, its definition with one node changed to a neighbouring
  value, and fails on any mutant that none of the card's scenarios (its file's,
  the rules library's and its generated ones) tells from the card, unless
  `EQUIVALENT` lists it with why the two always do the same. Generated scenarios
  also start from witnesses (`derive/witness.rs`), position fragments under
  which a field of a vocabulary entry changes the outcome. The oracles only
  compare what a scenario reaches, so a part no scenario shows is untested. When
  a scenario needs a card nobody has implemented, implement one, even one in no
  deck (the `card-writing` skill, "Cards for scenarios").
- `just set list <code>` lists the cards first printed in a set and how each is
  implemented or escalated. `just set plan <code>` groups unwritten cards by
  what they need and writes `$BERYL_CACHE/set-plan-<code>.json`. The `drafted`
  tier records complete drafts; each new definition still goes through render
  and card checks. The planner reuses the local renderer and census report when
  available. Add `--tickets` to print draft ticket commands for vocabulary
  groups and batches of drafted or composed cards.
- `just census` counts which mechanics Forge's card scripts use that
  `beryl_card` has no entry for, and how many cards each one blocks, from the
  mapping in `data/census-coverage.json`. It writes its report to
  `$BERYL_CACHE/census.json`.
- `beryl_cli script-check` runs every card's spec moments from both seats and
  every UI fixture's recording, checking each animation script against its
  assertions (the `script` family in `ignite/checks/script.toml`).

## App checks

Use the [client workspace guide](../client/AGENTS.md) for native client tests
and packaging or launching the app. `ignite test native-app:` checks desktop
startup, while `native-ios:` and `native-android:` build the phone packages. Use
`just table-pair --headless` to reproduce a localhost session without opening
windows.

## Benchmarks

Benchmarks follow `docs/benchmarking.md`: `just bench-count` counts instructions
and `just bench` measures time under the machine's normal load.
`just count-gate` checks the rollout benchmark's instruction count against the
limit in `engine/crates/beryl_bench/count-gate.toml`. That file's comments say
when the limit may go up.

## Tournaments

Tournaments are expensive. One game with an MCTS agent at
`--iterations 300 --trees 4` takes about 180 CPU-seconds, so a 100-game match
between two agents uses the whole machine for about 20 minutes, and a full round
robin for over 2 hours.

- `just tournament`, `just play` and `just review --agent` run at utility
  priority (`taskpolicy -c utility`) so that other sessions' checks get the CPU
  first (`nice` doesn't do this on macOS). They slow down while the machine is
  busy.
- Reviewers (the sessions that evaluate AI changes) rerun only the matches that
  decide whether a change is accepted, never the ones that are only reported.
