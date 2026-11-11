from pathlib import Path

from beryl_tools import citations

PAGE = """
<h2 id=R7><a href="#R7">7.</a> Additional Rules</h2>
<h4 id=R7126><a href="#R7126">712.6.</a> Each face of a
<a href="#double-faced_cards">double-faced card</a> has its own set of characteristics.</h4>
<h5 id=R7126e><a href="#R7126e">712.6e.</a> It has &ldquo;only&rdquo; the face that&rsquo;s up,
unless a rule says &quot;otherwise&quot;.</h5>
"""
LINK = "[CR {n}](https://yawgatog.com/resources/magic-rules/#{a})"


def cite(number: str, anchor: str) -> str:
    return LINK.format(n=number, a=anchor)


def problems(tmp_path: Path, rel: str, text: str) -> list[str]:
    path = tmp_path / rel
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)
    return citations.check(tmp_path, citations.parse_rules(PAGE))


def test_the_page_parses_into_rules_by_anchor() -> None:
    rules = citations.parse_rules(PAGE)
    assert (
        rules["R7126"]
        == "Each face of a double-faced card has its own set of characteristics."
    )
    # Curly quotes are straightened and the rule's double quotes become single ones.
    assert (
        rules["R7126e"]
        == "It has 'only' the face that's up, unless a rule says 'otherwise'."
    )


def test_a_true_citation_is_clean(tmp_path: Path) -> None:
    text = (
        f'/// ({cite("712.6", "R7126")}): "Each face of a double-faced\n'
        '/// card has its own set of characteristics."\n'
        f'/// {cite("712.6e", "R7126e")}: "It has \'only\' the face ... unless a rule says\n'
        "/// 'otherwise'.\"\n"
        f"/// A link with no quotation, {cite('712.6', 'R7126')}, is only checked for its rule.\n"
    )
    assert problems(tmp_path, "rust/crates/a/src/lib.rs", text) == []


def test_a_dead_anchor_or_wrong_anchor_is_reported(tmp_path: Path) -> None:
    text = f"// {cite('712.8f', 'R7128f')}\n// {cite('712.6', 'R712')}\n"
    assert problems(tmp_path, "ui/src/a.ts", text) == [
        "ui/src/a.ts:1: CR 712.8f: no such rule on the page",
        "ui/src/a.ts:2: CR 712.6: the anchor is #R712, not #R7126",
    ]


def test_a_quotation_the_rule_lacks_is_reported_per_segment(tmp_path: Path) -> None:
    text = f'/// {cite("712.6", "R7126")}: "Each face of a double-faced card ... has two sets."\n'
    assert problems(tmp_path, "rust/crates/a/src/lib.rs", text) == [
        'rust/crates/a/src/lib.rs:1: CR 712.6: the rule does not say "has two sets."'
    ]


def test_text_outside_the_roots_and_a_quotation_not_after_the_link_are_ignored(
    tmp_path: Path,
) -> None:
    text = f'// {cite("712.6", "R7126")} says things "that the page never does".\n'
    assert problems(tmp_path, "rust/crates/a/src/lib.rs", text) == []
    assert problems(tmp_path, "tools/tests/a.py", f'# {cite("1.1", "R11")}\n') == []
