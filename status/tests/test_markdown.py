"""The Markdown subset: every character of the source is escaped, and only safe links keep
their targets."""

from __future__ import annotations

from cirrus.markdown import inline, render


def test_escapes_html() -> None:
    assert render("<script>x</script>") == "<p>&lt;script&gt;x&lt;/script&gt;</p>"


def test_inline_marks_and_code() -> None:
    assert inline("**bold** and *it* and `a<b`") == (
        "<strong>bold</strong> and <em>it</em> and <code>a&lt;b</code>"
    )


def test_unsafe_link_targets_are_text() -> None:
    assert "href" not in inline("[x](javascript:alert(1))")
    assert inline("[x](https://e.com)") == '<a href="https://e.com">x</a>'


def test_bare_urls_link() -> None:
    assert inline("see https://e.com/a.") == (
        'see <a href="https://e.com/a">https://e.com/a</a>.'
    )


def test_blocks() -> None:
    html = render("# Head\n\n- a\n- b\n  - c\n\n1. one\n\n```py\nx < 1\n```\n\n> q")
    assert "<h3>Head</h3>" in html
    assert "<ul><li>a</li><li>b<ul><li>c</li></ul></li></ul>" in html
    assert "<ol><li>one</li></ol>" in html
    assert '<pre><code class="language-py">x &lt; 1</code></pre>' in html
    assert "<blockquote><p>q</p></blockquote>" in html


def test_tables() -> None:
    html = render("| a | b |\n|---|---|\n| 1 | 2 |")
    assert "<th>a</th><th>b</th>" in html and "<td>1</td><td>2</td>" in html


def test_autolink_skips_code_spans() -> None:
    def link(text: str) -> str:
        return text.replace("by-x", "<a>by-x</a>")

    assert inline("by-x and `by-x`", link) == "<a>by-x</a> and <code>by-x</code>"
