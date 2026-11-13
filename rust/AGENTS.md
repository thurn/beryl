# Working in `rust/`

This adds to the root `AGENTS.md`, whose rules all apply here.

## Crates and card files

- `crates/beryl_app` is the Tauri shell: the window, IPC commands,
  `tauri.conf.json`, capabilities and icons. `docs/engine-architecture.md` §16
  describes the other crates, and `docs/project-50k.md` is the plan for
  restructuring them to scale.
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
  `printed::` constant generated from the card store, and each card file's
  header comment is generated too (`just card header`).
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
  decision.
- `just fuzz` plays long random games, and `just exhaust` tries every sequence
  of decisions near the end positions of the scenarios.
- To start a new card, run `just card brief <name>`. It shows the card's Oracle
  text, rulings, matching vocabulary entries, the most similar cards already
  implemented and the XMage class.
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
