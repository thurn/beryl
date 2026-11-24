"""The check-inputs rule against small Rust files."""

from __future__ import annotations

from pathlib import Path

from beryl_tools import check_inputs


def run(
    tmp_path: Path, files: dict[str, str], extra: list[str] | None = None
) -> list[str]:
    tracked = [*files, *(extra or [])]
    for path, text in files.items():
        (tmp_path / path).parent.mkdir(parents=True, exist_ok=True)
        (tmp_path / path).write_text(text)
    return [
        line.split(": read it")[0] for line in check_inputs.check(tmp_path, tracked)
    ]


def test_a_test_that_opens_the_checkout_through_cargo_manifest_dir_is_reported(
    tmp_path: Path,
) -> None:
    found = run(
        tmp_path,
        {
            "engine/crates/a/Cargo.toml": "",
            "engine/crates/a/tests/reads.rs": "\n".join(
                [
                    "fn root() -> PathBuf {",
                    '    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..")',
                    "}",
                    "fn text() { std::fs::read_to_string(root().join(x)); }",
                    "",
                ]
            ),
        },
    )
    assert found == [
        "engine/crates/a/tests/reads.rs:2: opens the checkout through CARGO_MANIFEST_DIR"
    ]


def test_a_repository_literal_in_a_raw_read_is_reported_but_not_in_other_statements(
    tmp_path: Path,
) -> None:
    found = run(
        tmp_path,
        {
            "engine/crates/a/Cargo.toml": "",
            "engine/crates/a/tests/reads.rs": "\n".join(
                [
                    'let a = fs::read_to_string(root().join("data/x.json"));',
                    'let name = "data/x.json";',
                    "let b = 1;",
                    "let c = 2;",
                    "let d = 3;",
                    'let e = fs::read_to_string(dir.join("scratch.json"));',
                    "",
                ]
            ),
        },
        ["data/x.json"],
    )
    assert found == ["engine/crates/a/tests/reads.rs:1: reads data/x.json"]


def test_a_read_through_the_helper_is_not_reported(tmp_path: Path) -> None:
    found = run(
        tmp_path,
        {
            "engine/crates/a/Cargo.toml": "",
            "engine/crates/a/tests/reads.rs": "\n".join(
                [
                    'let a = beryl_test_files::read_to_string("data/x.json");',
                    'let b = ignite_core::test_files::read("data/x.json");',
                    "",
                ]
            ),
        },
        ["data/x.json"],
    )
    assert found == []


def test_code_outside_tests_and_build_scripts_is_not_read(tmp_path: Path) -> None:
    found = run(
        tmp_path,
        {
            "engine/crates/a/Cargo.toml": "",
            "engine/crates/a/src/lib.rs": "\n".join(
                [
                    'fn dev() { fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))); }',
                    "#[cfg(test)]",
                    "mod tests {",
                    '    fn t() { fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR"))); }',
                    "}",
                    "",
                ]
            ),
            "engine/crates/a/build.rs": 'fn main() { fs::read_to_string(env!("CARGO_MANIFEST_DIR")); }\n',
        },
    )
    # Only the line in the test module is reported.
    assert found == [
        "engine/crates/a/src/lib.rs:4: opens the checkout through CARGO_MANIFEST_DIR"
    ]


def test_an_env_remove_of_the_variable_is_not_a_read(tmp_path: Path) -> None:
    found = run(
        tmp_path,
        {
            "engine/crates/a/Cargo.toml": "",
            "engine/crates/a/tests/run.rs": "\n".join(
                [
                    'cmd.env_remove("CARGO_MANIFEST_DIR");',
                    "fs::read_to_string(dir.join(name));",
                    "",
                ]
            ),
        },
    )
    assert found == []


def test_ignite_literals_are_not_matched_but_its_manifest_dir_reads_are(
    tmp_path: Path,
) -> None:
    found = run(
        tmp_path,
        {
            "ignite/crates/a/Cargo.toml": "",
            "ignite/crates/a/tests/reads.rs": "\n".join(
                [
                    '    fs::write(dir.join("ignite.toml"), "");',
                    '    let t = fs::read_to_string(dir.join("ignite.toml"));',
                    'let r = Path::new(env!("CARGO_MANIFEST_DIR"));',
                    "",
                ]
            ),
        },
        ["ignite.toml"],
    )
    assert found == [
        "ignite/crates/a/tests/reads.rs:3: opens the checkout through CARGO_MANIFEST_DIR"
    ]
