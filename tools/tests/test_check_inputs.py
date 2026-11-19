"""The check-inputs rule against a small Cargo workspace and check manifest."""

from __future__ import annotations

from pathlib import Path

from beryl_tools import check_inputs

MANIFEST = """\
[product.tests]
build = "cargo build"
dir = "ws"

[family.unit]
kind = "regression"
adapter = "nextest"
needs = ["tests"]
list = "cargo nextest list --exclude skipped"
run = "cargo nextest run"
inputs = ["crate-closure", "data/declared.json", "assets/**"]
"""


def write(root: Path, path: str, text: str) -> None:
    (root / path).parent.mkdir(parents=True, exist_ok=True)
    (root / path).write_text(text)


def workspace(root: Path) -> list[str]:
    write(root, "ignite/checks/unit.toml", MANIFEST)
    write(
        root,
        "ws/Cargo.toml",
        '[workspace]\nmembers = ["a", "b", "skipped"]\nresolver = "2"\n',
    )
    for name, deps in (("a", 'b = { path = "../b" }'), ("b", ""), ("skipped", "")):
        write(
            root,
            f"ws/{name}/Cargo.toml",
            f'[package]\nname = "{name}"\nversion = "0.1.0"\nedition = "2021"\n'
            f"[dependencies]\n{deps}\n",
        )
        write(root, f"ws/{name}/src/lib.rs", "")
    write(root, "data/declared.json", "{}")
    write(root, "data/secret.json", "{}")
    write(root, "assets/x/y.png", "")
    write(root, "ws/b/src/table.txt", "")
    write(
        root,
        "ws/a/tests/reads.rs",
        "\n".join(
            [
                'const A: &str = include_str!("../../../data/declared.json");',
                'const B: &str = include_str!("../../../data/secret.json");',
                'let c = root().join("assets/x/y.png");',
                'let d = Path::new(env!("CARGO_MANIFEST_DIR")).join("../b/src/table.txt");',
                'let e = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");',
                "let f = 1;",
                "let g = 2;",
                "let h = 3;",
                'let name = "data/secret.json";',
                "",
            ]
        ),
    )
    write(root, "ws/skipped/src/x.rs", 'include_str!("../../../data/secret.json");\n')
    return [str(p.relative_to(root).as_posix()) for p in root.rglob("*") if p.is_file()]


def test_a_read_outside_the_family_inputs_and_the_crate_closure_is_reported(
    tmp_path: Path,
) -> None:
    tracked = workspace(tmp_path)
    found = check_inputs.uncovered(tmp_path, tracked)
    # The declared file, the declared folder and a dependency's file are covered; the line
    # with no read and the package the family leaves out are not looked at.
    assert found == ["ws/a/tests/reads.rs:2: data/secret.json (unit, a)"]


def test_the_allowlist_holds_a_file_to_its_count_which_only_goes_down(
    tmp_path: Path,
) -> None:
    found = ["x.rs:2: data/a (f, p)", "x.rs:9: data/b (f, p)"]
    assert check_inputs.problems(found, {"x.rs": 2}) == []
    assert len(check_inputs.problems(found, {"x.rs": 1})) == 2
    lower = check_inputs.problems(found[:1], {"x.rs": 2})
    assert lower and "lower its count" in lower[0]


def test_a_package_input_covers_that_package_alone(tmp_path: Path) -> None:
    tracked = workspace(tmp_path)
    write(
        tmp_path, "ws/b/tests/reads.rs", 'include_str!("../../../data/secret.json");\n'
    )
    tracked.append("ws/b/tests/reads.rs")
    manifest = MANIFEST + '\n[family.unit.package_inputs]\na = ["data/secret.json"]\n'
    write(tmp_path, "ignite/checks/unit.toml", manifest)
    # Package a's read is declared for it; package b reads the same file without declaring it.
    assert check_inputs.uncovered(tmp_path, tracked) == [
        "ws/b/tests/reads.rs:1: data/secret.json (unit, b)"
    ]


def test_the_ignite_workspace_is_not_scanned(tmp_path: Path) -> None:
    tracked = workspace(tmp_path)
    manifest = MANIFEST.replace('dir = "ws"', 'dir = "ignite"')
    write(tmp_path, "ignite/checks/unit.toml", manifest)
    # Ignite's tests write fixture files under the real repository's names, which are not reads.
    assert check_inputs.families(tmp_path) == []
