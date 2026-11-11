# Beryl's commands; `just` lists them. Most wrap `by` (tools/), and extra arguments pass
# straight through: `just smoke --seconds 30`.

set positional-arguments := true

# Recipes are POSIX shell; on Windows, Git for Windows' (PowerShell has no `sh` on its PATH).

set windows-shell := ["C:/Program Files/Git/bin/sh.exe", "-cu"]

# Python reads and writes text as UTF-8, as it does off Windows.

export PYTHONUTF8 := "1"
by := "uv run --quiet --project tools by"

# The CPU the `search` profile builds for: a Cargo profile cannot set it (architecture §18).
# Off Apple silicon, this machine's own.

search_cpu := if arch() == "aarch64" { "-C target-cpu=apple-m4" } else { "-C target-cpu=native" }

# Low priority for long compute: utility QoS, under which macOS gives interactive work and
# checks the CPU first. `nice` does not: beside nice'd work a check ran as slowly as beside
# work at full priority. Windows has neither: `by low` starts the work below normal priority.

by_anywhere := "uv run --quiet --project " + quote(justfile_directory() / "tools") + " by"
low := if os() == "windows" { by_anywhere + " low --" } else { "taskpolicy -c utility" }

# The long-think arena's priority, kept awake.

awake := if os() == "windows" { by_anywhere + " low --awake --" } else { "caffeinate -i nice -n 10" }

default:
    @just --list --unsorted

# The app with hot reload, in a window (agents use `smoke`)
[group('app')]
dev *args:
    {{ by }} dev "$@"

# Build the debug app and launch it headless until its UI loads (--seconds N, --no-build)
[group('app')]
smoke *args:
    {{ by }} smoke "$@"

# Install the phone apps' toolchains: Rust targets, CocoaPods, the iOS Simulator runtime, the Android SDK (idempotent)
[group('app')]
mobile-setup:
    {{ by }} mobile-setup

# Build the debug iOS app, boot the Beryl iPhone simulator and launch it there (--no-build)
[group('app')]
ios *args:
    {{ by }} ios "$@"

# Build the debug iOS app and launch it in a headless simulator until its UI loads (--seconds N, --no-build)
[group('app')]
ios-smoke *args:
    {{ by }} ios-smoke "$@"

# Build and sign the release iOS app and install it on the paired iPhone (--no-install)
[group('app')]
ios-device *args:
    {{ by }} ios-device "$@"

# Build the debug Android app, start the Beryl emulator and launch it there (--no-build)
[group('app')]
android *args:
    {{ by }} android "$@"

# Build the debug Android app and launch it in a headless emulator until its UI loads (--seconds N, --no-build)
[group('app')]
android-smoke *args:
    {{ by }} android-smoke "$@"

# Screenshot UI states headless into the cache and print the paths (shot names; ui/shots.json)
[group('app')]
ui-shots *args:
    {{ by }} ui-shots "$@"

# Play the game screen headless through a recorded fixture: drag, click, hover (title filters)
[group('app')]
ui-test *args:
    {{ by }} ui-test "$@"

# Live games in a headless page an agent drives over HTTP, until interrupted (--release, --port N)
[group('app')]
ui-live *args:
    {{ by }} ui-live "$@"

# The cards' spec pages, each moment a live game
[group('app')]
specs *args:
    {{ by }} specs "$@"

# Arena recordings indexed into ~/arena as a UI reference: list, index, find, film, cards or card
video *args:
    {{ by }} video "$@"

# Playable card review and the reproducible scenario viewer.
playground *args:
    {{ by }} playground "$@"

scenarios *args:
    {{ by }} scenarios "$@"

# The app's bug reports: list, import a ZIP, file as beads, or mark done
[group('app')]
bugs *args:
    {{ by }} bugs "$@"

# The release Beryl.app, under rust/target/tauri/release/bundle/macos (Windows: an installer under bundle/nsis)
[group('app')]
bundle:
    {{ by }} bundle

# Restore rust/target and ui/node_modules from the cache (every command does this too)
[group('app')]
warm *args:
    {{ by }} warm "$@"

# Time benchmarks under the search profile, refusing a busy, unplugged or throttled machine (cargo bench flags; --force)
[group('bench')]
bench *args:
    {{ by }} bench "$@"

# Build cloned cards at scale in a scratch workspace and time the compiler (--cards N --crates K)
[group('bench')]
scale *args:
    {{ by }} scale "$@"

# Gungraun instruction counts in the Lima VM (cargo bench flags; default --bench '*_counts')
[group('bench')]
bench-count *args:
    {{ by }} bench-count "$@"

# Fail when the rollout benchmark's instruction count rises past its bar (count-gate.toml)
[group('bench')]
count-gate:
    {{ by }} count-gate

# Look up card data on Scryfall into data/cards.json
[group('data')]
cards *args:
    {{ by }} cards "$@"

# Transcode the UI's music and sounds from the staged CC0 packs into ui/public/audio (--check)
[group('data')]
audio *args:
    {{ by }} audio "$@"

# Install Maven and JDK 21, fetch XMage and Forge at their pins and build them, in the cache
[group('oracles')]
oracle-setup:
    {{ by }} oracle-setup

# Check every card scenario against the oracles, with a JSON summary (card names for just those)
[group('oracles')]
oracle *cards:
    {{ by }} oracle "$@"

# Run one XMage test class and one Forge sim game (xmage or forge for just one)
[group('oracles')]
oracle-smoke *args:
    {{ by }} oracle-smoke "$@"

# The Rust tests (cargo test flags, e.g. -p beryl_app)
[group('test')]
test-rust *args:
    cd rust && cargo test "$@"

# Random-game fuzzing at length: N games, optimized but with debug assertions on
[group('test')]
fuzz games="10000":
    cd rust && BERYL_FUZZ_GAMES="$1" CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true CARGO_PROFILE_RELEASE_OVERFLOW_CHECKS=true cargo test --release -p beryl_search --test games -- --nocapture

# Adds saves to the frozen corpus, weekly or so: seeded games covering every card and decision, or one replay (--replay FILE, --games N, --seed N)
[group('test')]
[no-cd]
freeze-saves *args:
    cargo run --quiet --release --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- freeze-saves {{ quote(justfile_directory() / "rust" / "crates" / "beryl_session" / "tests" / "saves") }} "$@"

# A seeded WU Control mirror game between two agents at low priority, as a transcript (--seed N, --out FILE)
[group('agents')]
[no-cd]
play *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- play "$@"

# An app game's transcript from a dev build in seconds; `--agent recorded` (or NAME) adds its searches from a search build at low priority (REPLAY.json, --turn N, --out FILE)
[group('agents')]
[no-cd]
review *args:
    #!/usr/bin/env bash
    set -euo pipefail
    manifest={{ quote(justfile_directory() / "rust" / "Cargo.toml") }}
    case " $* " in
      *" --agent greedy "*) search=false ;;
      *" --agent "*) search=true ;;
      *) search=false ;;
    esac
    if $search; then
      RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path "$manifest" -p beryl_cli -- review "$@"
    else
      cargo run --quiet --manifest-path "$manifest" -p beryl_cli -- review "$@"
    fi

# A round robin between agents, games in parallel at low priority (--games N, --seed N, --json FILE, --check)
[group('agents')]
[no-cd]
tournament *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- tournament "$@"

# A paired match against the champion until the SPRT decides, at low priority (--vs NAME, --work 3M, --bar stronger|plausible, --cap N, --seed N, --out DIR)
[group('agents')]
[no-cd]
ladder *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- ladder "$@"

# The ladder's start positions, cut from champion games at low priority: build [--games N] [--seed N] [--work 3M] [--out DIR], or sample [--count N] DIR
[group('agents')]
[no-cd]
positions *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- positions "$@"

# A challenger's differing moves over the champion's last match, re-searched, at low priority (<challenger> <match-dir> --vs NAME, --recheck N, --work 3M, --games N)
[group('agents')]
[no-cd]
screen *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- screen "$@"

# The blunder suite against an agent, the champion by default, at low priority ([agent] --work 3M,30M, --seeds 5, --only NAME,..., --json FILE)
[group('agents')]
[no-cd]
blunders *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- blunders "$@"

# Discover candidate blunders for later LLM review; six hours by default (--hours 6, --out DIR, --seed N, --inspect CASE.json)
[group('agents')]
[no-cd]
blunder-hunt *args:
    RUSTFLAGS="{{ search_cpu }}" {{ low }} {{ awake }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- blunder-hunt "$@"

# The long-think arena: starts or resumes the latest run, low priority, kept awake (--status, --new, --run DIR, --upgrade, --add FAMILY, --games N, --power full|light)
[group('agents')]
[no-cd]
longthink *args:
    RUSTFLAGS="{{ search_cpu }}" {{ awake }} cargo run --quiet --profile search --manifest-path {{ quote(justfile_directory() / "rust" / "Cargo.toml") }} -p beryl_cli -- longthink "$@"

# The tools' tests
[group('test')]
test-py *args:
    cd tools && uv run --quiet pytest "$@"

# The fast check `enact push` runs (lanes for what changed, budget 60 s)
[group('checks')]
check:
    {{ by }} check

# Slow CI, or some of its lanes: repo, python, rust, ui, app, shots, ui-test, oracle, counts (budget 300 s)
[group('checks')]
ci *lanes:
    {{ by }} ci "$@"

# Each check's runs, failures, verdicts and seconds, costliest first, with lane totals (--days 30, --json, --quarantined, --pending)
[group('checks')]
ci-report *args:
    {{ by }} ci-report "$@"

# Correct the latest verdict on a check's failure: just verdict '<check>' stale|catch|flaky
[group('checks')]
verdict check verdict:
    {{ by }} verdict "$1" "$2"

# Format everything: black, rustfmt, prettier, mdformat (80 columns), the justfile
[group('checks')]
fmt:
    uv run --quiet --project tools black --quiet tools
    cd rust && cargo fmt --all
    {{ by }} warm
    cd ui && node_modules/.bin/prettier --write --log-level warn .
    just --fmt --unstable
    git ls-files -co --exclude-standard '*.md' | grep -v -e '^.beads/' -e '^.agents/' | xargs uv run --quiet --project tools mdformat

# Edit one file in $EDITOR in a fresh worktree, then commit and `enact push` it
[group('enact')]
[no-cd]
edit file:
    @uv run --quiet --project {{ quote(justfile_directory() / "tools") }} by edit "$1"
