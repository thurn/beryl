# Working in `rust/`

This adds to the root `AGENTS.md`, whose rules all apply here.

## Crates and card files

- `crates/beryl_app` is the Tauri shell: the window, IPC commands,
  `tauri.conf.json`, capabilities and icons. `docs/engine-architecture.md` §16
  describes the other crates.
- Each card is one file, `rust/cards/<crate>/<set>/<card>.rs`, placed by the set
  it was first printed in. `just card path <name>` prints the path. Card crates
  depend only on the shared vocabulary crate.
- Nobody maintains lists of card files by hand. Build scripts
  (`beryl_card_build`) generate each set crate's module list and tables, and
  `beryl_cards`' registry and `names` from them. A creature whose rules text is
  only keywords the engine has needs no file: its set crate's build writes it
  from the card store (`just card path <name>` says so), and `just card crates`
  writes the set crates.
- Nobody types a mana cost or type line either. Each card face starts from a
  `Printed::` constant generated from the card store, and each card file's
  header comment is generated too (`just card header`). Card files name cards as
  `Card::Counterspell` and import everything with `use crate::prelude::*;`; code
  outside the card files uses `beryl_cards::names`.
- `ui/src/generated/protocol.ts` is generated from `beryl_session` by ts-rs. A
  test fails when it is out of date, and its header explains how to regenerate
  it.
- The test decks are `decks::WU_CONTROL` and, before it,
  `decks::VINTAGE_WU_CONTROL`.

## Testing rules and cards

- `just test-rust` runs the workspace's tests.
- Each card's test scenarios live in its own file. `just oracle <card names>`
  runs those scenarios against XMage and Forge, two existing Magic engines we
  use as "oracles" to check our rules. It needs `just oracle-setup` once per
  machine.
- `just citations` checks every Comprehensive Rules citation and quote in the
  code against the rules release pinned in `data/rules-version.json`.
  `just citations migrate <date>` moves the code to a newer rules release: it
  renumbers the citations it can match and lists the ones that need a human
  decision. `just citations shorten <paths>` rewrites the inline links in doc
  comments as references, `[CR 118.12a]`, with the links defined at the end of
  each comment.
- `just fuzz` plays long random games, and `just exhaust` tries every sequence
  of decisions near the end positions of the scenarios.
- To start a new card, run `just card brief <name>`. It shows the card's Oracle
  text, rulings, matching vocabulary entries, the most similar cards already
  implemented and the XMage class. `just card new <name>` writes the card's file
  with its header and an empty definition, and `just card check <name>` builds
  its set crate, plays its scenarios in Beryl, XMage and Forge (`--no-oracles`
  for a fast loop), and compares the rules text its definition renders to with
  its Oracle text. `just card render <name>` prints that comparison, and
  `just card render --all` the match rate over every card file. A card the
  vocabulary cannot yet express is recorded in `data/card-escalations.json`, and
  its brief says what it is waiting for. The `card-writing` skill is the whole
  procedure.
- Two checks hold a definition to its card. The `card-text` step of the rust
  lane renders every card file and fails on any difference from its Oracle text
  that `data/card-text-allowlist.json` doesn't pin exactly. The test
  `every_instruction_of_a_card_is_carried_out_by_its_own_scenarios` (in
  `beryl_cards`) fails on any part of a card's effect data, such as one branch
  of an `If`, that none of the card's own scenarios carries out. The oracles
  only compare what a scenario reaches, so a part no scenario reaches is
  untested. When a scenario needs a card nobody has implemented, implement one,
  even one in no deck (the `card-writing` skill, "Cards for scenarios").
- `just census` counts which mechanics Forge's card scripts use that
  `beryl_card` has no entry for, and how many cards each one blocks, from the
  mapping in `data/census-coverage.json`. It writes its report to
  `$BERYL_CACHE/census.json`.
- `beryl_cli script-check` runs every card's spec moments from both seats and
  every UI fixture's recording, checking each animation script against its
  assertions (the `script` lane in `docs/checks.md`).

## The app and the phone apps

- `just smoke` builds the debug app and launches it without a window or Dock
  icon, and waits until the UI reports over IPC that it has loaded (about a
  second once built). It fails on a build error or if the UI never loads.
- `just mobile-setup` installs the phone toolchains, once per machine.
- `just ios-smoke` builds the iOS app and launches it in a headless iOS
  Simulator until the UI loads. `just ios` launches it in a simulator, which you
  can drive with the iOS Simulator tool.
- Agents never install onto a real phone: `just ios-device` (which signs the app
  and installs it on the paired iPhone) is only for the user, and with
  `--no-install` it only builds.
- `just android-smoke` and `just android` do the same in the Android emulator
  (headless for agents; drive it with `adb shell input` or the WebView's
  DevTools over `adb forward`).
- A human runs `just dev` for the app with hot reload, or `just bundle` for a
  release `Beryl.app`.

## Benchmarks

Benchmarks follow `docs/benchmarking.md`: `just bench-count` counts instructions
and `just bench` measures time under the machine's normal load.
`just count-gate` checks the rollout benchmark's instruction count against the
limit in `rust/crates/beryl_bench/count-gate.toml`. That file's comments say
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
- Wardens (the sessions that evaluate AI changes) rerun only the matches that
  decide whether a change is accepted, never the ones that are only reported.
- The "long-think arena" is retired and its command deleted
  (`docs/ai-strategy.md` §8): it used the machine for days, and the AI now
  improves through the ladder's head-to-head matches instead. Its last run is
  recorded in `docs/longthink-run-003.md`.
