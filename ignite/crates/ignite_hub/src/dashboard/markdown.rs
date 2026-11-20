//! The Markdown subset ticket bodies and comments use, rendered to HTML.
//!
//! The subset is headings, paragraphs, bullet and numbered lists (nested), block quotes, fenced
//! code, tables, code spans, links, bare URLs, bold and italic, and images of the ticket's own
//! attachments, `![alt](attachment:<name>)`, which an [`Images`] hook turns into HTML. No other
//! image is shown: `![alt](https://...)` stays a plain link, so a ticket cannot make the
//! dashboard load images from elsewhere. Source text is escaped; only the
//! renderer and its trusted autolink hook supply HTML. The renderer is a port of the ticket
//! service's, which follows a Python original, so the matching rules below are written out by
//! hand to behave as the regular expressions they replace did (the workspace has no regex
//! dependency, and the rules need lookbehind).
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use super::components::{esc, esc_text};

/// Turns already-escaped text into HTML with links, outside code and links.
pub type Autolink<'a> = &'a dyn Fn(&str) -> String;

/// Rewrites a block's source text before its inline pass.
pub type Prepare<'a> = &'a dyn Fn(&str) -> String;

/// Turns an attachment image into HTML, given its alt text and the attachment's name.
pub type Images<'a> = &'a dyn Fn(&str, &str) -> String;

/// The target prefix that names one of the ticket's attachments.
pub const ATTACHMENT_TARGET: &str = "attachment:";

/// A character that separates nothing: the stand-in for text kept out of later passes.
const MARK: char = '\u{E000}';

fn identity(text: &str) -> String {
    text.to_owned()
}

/// Renders blocks with no link hooks.
pub fn to_html(markdown: &str) -> String {
    render(markdown, &identity, &identity)
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

fn is_space(c: char) -> bool {
    c.is_whitespace() || c == '\u{FEFF}'
}

/// Whether `c` is a decimal digit in any script, as `\p{Decimal_Number}` matches.
fn is_decimal(c: char) -> bool {
    c.is_ascii_digit() || is_nd(c)
}

/// The non-ASCII decimal digit blocks: each is ten consecutive code points.
fn is_nd(c: char) -> bool {
    const STARTS: &[u32] = &[
        0x0660, 0x06F0, 0x07C0, 0x0966, 0x09E6, 0x0A66, 0x0AE6, 0x0B66, 0x0BE6, 0x0C66, 0x0CE6,
        0x0D66, 0x0DE6, 0x0E50, 0x0ED0, 0x0F20, 0x1040, 0x1090, 0x17E0, 0x1810, 0x1946, 0x19D0,
        0x1A80, 0x1A90, 0x1B50, 0x1BB0, 0x1C40, 0x1C50, 0xA620, 0xA8D0, 0xA900, 0xA9D0, 0xA9F0,
        0xAA50, 0xABF0, 0xFF10, 0x104A0, 0x11066, 0x1D7CE, 0x1D7D8, 0x1D7E2, 0x1D7EC, 0x1D7F6,
    ];
    let code = c as u32;
    STARTS
        .iter()
        .any(|&start| (start..start + 10).contains(&code))
}

// ---------------------------------------------------------------------------------------------
// Inline text

/// A matcher at one position returns the end of its match and what replaces it.
type Found = Option<(usize, String)>;

/// Replaces each match of `at` in `text`, scanning left to right without overlap.
fn replace_all(text: &str, at: &dyn Fn(&[char], usize) -> Found) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if let Some((end, html)) = at(&chars, index) {
            out.push_str(&html);
            index = end;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

fn string(chars: &[char]) -> String {
    chars.iter().collect()
}

fn starts_with(chars: &[char], at: usize, needle: &str) -> bool {
    needle
        .chars()
        .enumerate()
        .all(|(offset, c)| chars.get(at + offset) == Some(&c))
}

/// A code span: a run of backticks, the shortest nonempty text on one line, the same run.
fn code_span(chars: &[char], at: usize) -> Found {
    if chars[at] != '`' {
        return None;
    }
    let run = chars[at..].iter().take_while(|&&c| c == '`').count();
    // The run is greedy but gives characters back when no closing run exists.
    for ticks in (1..=run).rev() {
        let start = at + ticks;
        let mut end = start + 1;
        while end <= chars.len() && !chars[start..end].contains(&'\n') {
            if chars
                .get(end..end + ticks)
                .is_some_and(|c| c.iter().all(|&c| c == '`'))
            {
                let code = string(&chars[start..end]);
                return Some((end + ticks, format!("<code>{}</code>", esc(&code))));
            }
            end += 1;
        }
    }
    None
}

/// Whether an explicit link target is one a page may follow: an absolute URL, a path, a
/// fragment, or a word-led relative path.
fn safe_target(target: &str) -> bool {
    if target.starts_with("http://") || target.starts_with("https://") {
        return true;
    }
    if target.starts_with(['/', '.', '#']) {
        return true;
    }
    let word = target
        .chars()
        .take_while(|&c| c.is_alphanumeric() || c == '_' || c == '-')
        .count();
    word > 0 && target.chars().nth(word).is_none_or(|c| c == '/')
}

/// `[label](target)`: a label without `]`, a target without `)` or space.
fn link(chars: &[char], at: usize) -> Found {
    let (end, label, target) = link_parts(chars, at)?;
    let html = if safe_target(&target) {
        format!("<a href=\"{}\">{}</a>", esc(&target), esc(&label))
    } else {
        esc(&string(&chars[at..=end]))
    };
    Some((end + 1, html))
}

/// `![alt](attachment:<name>)`, rendered by `images`. Any other image target is no match, so
/// its `[alt](target)` is then read as a link.
fn image(chars: &[char], at: usize, images: Images) -> Found {
    if chars[at] != '!' || chars.get(at + 1) != Some(&'[') {
        return None;
    }
    let (end, alt, target) = link_parts(chars, at + 1)?;
    let name = target.strip_prefix(ATTACHMENT_TARGET)?;
    (!name.is_empty()).then(|| (end + 1, images(&alt, name)))
}

/// The index of the closing `)`, the label and the target of the link starting at `at`.
fn link_parts(chars: &[char], at: usize) -> Option<(usize, String, String)> {
    if chars[at] != '[' {
        return None;
    }
    let close = (at + 1..chars.len()).find(|&i| chars[i] == ']')?;
    if close == at + 1 || chars.get(close + 1) != Some(&'(') {
        return None;
    }
    let start = close + 2;
    let end = start
        + chars[start.min(chars.len())..]
            .iter()
            .take_while(|&&c| c != ')' && !is_space(c))
            .count();
    if end == start || chars.get(end) != Some(&')') {
        return None;
    }
    Some((
        end,
        string(&chars[at + 1..close]),
        string(&chars[start..end]),
    ))
}

/// A bare `http(s)://` URL that does not end in punctuation and is not part of a word or an
/// attribute value.
fn bare_url(chars: &[char], at: usize) -> Found {
    if at > 0 {
        let before = chars[at - 1];
        if before == '"' || before == '\'' || before == '=' || is_word(before) {
            return None;
        }
    }
    let scheme = if starts_with(chars, at, "https://") {
        8
    } else if starts_with(chars, at, "http://") {
        7
    } else {
        return None;
    };
    let run = chars[at + scheme..]
        .iter()
        .take_while(|&&c| !is_space(c) && c != '<')
        .count();
    let trailing = |c: char| ".,;:!?)]'\"".contains(c);
    // At least one character, then a final one that is not trailing punctuation.
    let last = (1..run)
        .rev()
        .find(|&i| !trailing(chars[at + scheme + i]))?;
    let end = at + scheme + last + 1;
    let escaped = esc(&string(&chars[at..end]));
    Some((end, format!("<a href=\"{escaped}\">{escaped}</a>")))
}

/// Emphasis delimited by `marker`, doubled for bold. `outside` says which neighbours of the
/// delimiters rule a match out.
fn delimited(
    chars: &[char],
    at: usize,
    marker: char,
    double: bool,
    outside: &dyn Fn(char) -> bool,
) -> Found {
    let width = if double { 2 } else { 1 };
    if (0..width).any(|i| chars.get(at + i) != Some(&marker)) {
        return None;
    }
    if !double {
        if at > 0 && outside(chars[at - 1]) {
            return None;
        }
        if chars.get(at + 1).is_none_or(|&c| is_space(c)) {
            return None;
        }
    }
    let start = at + width;
    let mut end = start + 1;
    while end <= chars.len() && !chars[start..end].contains(&'\n') {
        let closes = (0..width).all(|i| chars.get(end + i) == Some(&marker));
        let spaced = !double && is_space(chars[end - 1]);
        let after = chars.get(end + width).copied();
        if closes && !spaced && (double || after.is_none_or(|c| !outside(c))) {
            let inner = string(&chars[start..end]);
            let tag = if double { "strong" } else { "em" };
            return Some((end + width, format!("<{tag}>{inner}</{tag}>")));
        }
        end += 1;
    }
    None
}

fn bold(chars: &[char], at: usize) -> Found {
    delimited(chars, at, '*', true, &|_| false)
        .or_else(|| delimited(chars, at, '_', true, &|_| false))
}

fn italic(chars: &[char], at: usize) -> Found {
    delimited(chars, at, '*', false, &|c| is_word(c) || c == '*')
        .or_else(|| delimited(chars, at, '_', false, &is_word))
}

/// Renders inline Markdown, leaving the autolink hook out of code and links.
pub fn inline(source: &str, autolink: Autolink) -> String {
    inline_with(source, autolink, None)
}

/// [`inline`], with attachment images rendered by `images`; without it they stay text.
fn inline_with(source: &str, autolink: Autolink, images: Option<Images>) -> String {
    // A delimiter absent from the source keeps literal text out of the lookup.
    let mut delimiter = MARK.to_string();
    while source.contains(&delimiter) {
        delimiter.push(MARK);
    }
    let kept = std::cell::RefCell::new(Vec::<String>::new());
    let keep = |html: String| {
        let mut kept = kept.borrow_mut();
        kept.push(html);
        format!("{delimiter}{}{delimiter}", kept.len() - 1)
    };

    let mut text = replace_all(source, &|chars, at| {
        code_span(chars, at).map(|(end, html)| (end, keep(html)))
    });
    if let Some(images) = images {
        text = replace_all(&text, &|chars, at| {
            image(chars, at, images).map(|(end, html)| (end, keep(html)))
        });
    }
    let text = replace_all(&text, &|chars, at| {
        link(chars, at).map(|(end, html)| (end, keep(html)))
    });
    let text = replace_all(&text, &|chars, at| {
        bare_url(chars, at).map(|(end, html)| (end, keep(html)))
    });
    let text = esc_text(&text);
    let text = replace_all(&text, &bold);
    let text = replace_all(&text, &italic);
    let linked = autolink(&text);
    restore(&linked, &delimiter, &kept.borrow())
}

/// Replaces each `delimiter index delimiter` with the kept text, in one pass.
fn restore(text: &str, delimiter: &str, kept: &[String]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let width = delimiter.chars().count();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        if starts_with(&chars, index, delimiter) {
            let digits = chars[index + width..]
                .iter()
                .take_while(|c| c.is_ascii_digit())
                .count();
            let after = index + width + digits;
            if digits > 0 && starts_with(&chars, after, delimiter) {
                let number = string(&chars[index + width..after]);
                if let Some(html) = number.parse::<usize>().ok().and_then(|n| kept.get(n)) {
                    out.push_str(html);
                } else {
                    out.push_str(&string(&chars[index..after + width]));
                }
                index = after + width;
                continue;
            }
        }
        out.push(chars[index]);
        index += 1;
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Blocks

/// `(indent, rest)` of a line that is a fence: ```` ``` ```` or `~~~` after optional spaces.
fn fence(line: &str) -> Option<(&'static str, &str)> {
    let rest = line.trim_start_matches(is_space);
    for marker in ["```", "~~~"] {
        if let Some(info) = rest.strip_prefix(marker) {
            return Some((marker, info));
        }
    }
    None
}

/// The text of an ATX heading and its level.
fn heading(line: &str) -> Option<(usize, &str)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if hashes == 0 || hashes > 6 {
        return None;
    }
    let after = &line[hashes..];
    let rest = after.trim_start_matches(is_space);
    if rest.len() == after.len() {
        return None;
    }
    let text = rest
        .trim_end_matches(is_space)
        .trim_end_matches('#')
        .trim_end_matches(is_space);
    Some((hashes, text))
}

/// Which kind of list a line starts, its indent in characters and its text.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Bullet,
    Numbered,
}

fn bullet(line: &str) -> Option<(usize, &str)> {
    let rest = line.trim_start_matches(is_space);
    let indent = line.chars().count() - rest.chars().count();
    let mut chars = rest.chars();
    if !matches!(chars.next(), Some('-' | '*' | '+')) {
        return None;
    }
    let after = chars.as_str();
    let text = after.trim_start_matches(is_space);
    (text.len() != after.len()).then_some((indent, text))
}

fn numbered(line: &str) -> Option<(usize, &str)> {
    let rest = line.trim_start_matches(is_space);
    let indent = line.chars().count() - rest.chars().count();
    let digits = rest
        .chars()
        .take_while(|&c| is_decimal(c))
        .map(char::len_utf8)
        .sum::<usize>();
    let mut chars = rest[digits..].chars();
    if digits == 0 || !matches!(chars.next(), Some('.' | ')')) {
        return None;
    }
    let after = chars.as_str();
    let text = after.trim_start_matches(is_space);
    (text.len() != after.len()).then_some((indent, text))
}

fn list_item(line: &str) -> Option<(Kind, usize, &str)> {
    bullet(line)
        .map(|(indent, text)| (Kind::Bullet, indent, text))
        .or_else(|| numbered(line).map(|(indent, text)| (Kind::Numbered, indent, text)))
}

/// The text after a `>` marker and one optional space.
fn quote(line: &str) -> Option<&str> {
    let rest = line.trim_start_matches(is_space).strip_prefix('>')?;
    let mut chars = rest.chars();
    match chars.next() {
        Some(c) if is_space(c) => Some(chars.as_str()),
        _ => Some(rest),
    }
}

/// One cell of a table rule: optional colon, two or more dashes, optional colon.
fn rule_cell(chars: &[char], mut at: usize) -> Option<usize> {
    if chars.get(at) == Some(&':') {
        at += 1;
    }
    let dashes = chars[at.min(chars.len())..]
        .iter()
        .take_while(|&&c| c == '-')
        .count();
    if dashes < 2 {
        return None;
    }
    at += dashes;
    if chars.get(at) == Some(&':') {
        at += 1;
    }
    Some(at)
}

fn skip_space(chars: &[char], mut at: usize) -> usize {
    while chars.get(at).is_some_and(|&c| is_space(c)) {
        at += 1;
    }
    at
}

/// Whether a line is the dashed rule under a table's header.
fn table_rule(line: &str) -> bool {
    let chars: Vec<char> = line.chars().collect();
    let mut at = skip_space(&chars, 0);
    if chars.get(at) == Some(&'|') {
        at += 1;
    }
    at = skip_space(&chars, at);
    let Some(end) = rule_cell(&chars, at) else {
        return false;
    };
    at = skip_space(&chars, end);
    while chars.get(at) == Some(&'|') {
        let next = skip_space(&chars, at + 1);
        let Some(end) = rule_cell(&chars, next) else {
            at += 1;
            break;
        };
        at = skip_space(&chars, end);
    }
    skip_space(&chars, at) == chars.len()
}

fn cells(line: &str) -> Vec<&str> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = line.strip_suffix('|').unwrap_or(line);
    line.split('|').map(str::trim).collect()
}

fn paragraph_ends(line: &str) -> bool {
    line.trim().is_empty()
        || fence(line).is_some()
        || heading(line).is_some()
        || list_item(line).is_some()
        || quote(line).is_some()
}

/// Renders blocks, including fenced code, tables, lists and quotes. `prepare` rewrites each
/// block's text before its inline pass, and `autolink` links the inline text.
pub fn render(source: &str, autolink: Autolink, prepare: Prepare) -> String {
    render_blocks(source, autolink, prepare, None)
}

/// [`render`], with the ticket's attachment images rendered by `images`.
pub fn render_with_images(
    source: &str,
    autolink: Autolink,
    prepare: Prepare,
    images: Images,
) -> String {
    render_blocks(source, autolink, prepare, Some(images))
}

fn render_blocks(
    source: &str,
    autolink: Autolink,
    prepare: Prepare,
    images: Option<Images>,
) -> String {
    let normalized = source.replace("\r\n", "\n");
    let lines: Vec<&str> = normalized.split('\n').collect();
    let inlines = |text: &str| inline_with(&prepare(text), autolink, images);
    let mut out: Vec<String> = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let line = lines[index];
        if line.trim().is_empty() {
            index += 1;
            continue;
        }

        if let Some((marker, info)) = fence(line) {
            let language = info.trim().split(' ').next().unwrap_or("");
            let mut body = Vec::new();
            index += 1;
            while index < lines.len() && !lines[index].trim().starts_with(marker) {
                body.push(lines[index]);
                index += 1;
            }
            index += 1;
            let class = if language.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", esc(language))
            };
            out.push(format!(
                "<pre><code{class}>{}</code></pre>",
                esc(&body.join("\n"))
            ));
            continue;
        }

        if let Some((hashes, text)) = heading(line) {
            // A description's headings sit under the page's own section titles.
            let level = (hashes + 2).min(6);
            out.push(format!("<h{level}>{}</h{level}>", inlines(text)));
            index += 1;
            continue;
        }

        if quote(line).is_some() {
            let mut quoted = Vec::new();
            while let Some(text) = lines.get(index).and_then(|line| quote(line)) {
                quoted.push(text);
                index += 1;
            }
            out.push(format!(
                "<blockquote>{}</blockquote>",
                render_blocks(&quoted.join("\n"), autolink, prepare, images)
            ));
            continue;
        }

        if line.contains('|')
            && lines
                .get(index + 1)
                .is_some_and(|next| next.contains('|') && table_rule(next))
        {
            let headers: String = cells(line)
                .iter()
                .map(|cell| format!("<th>{}</th>", inlines(cell)))
                .collect();
            let mut rows = String::new();
            index += 2;
            while index < lines.len() && lines[index].contains('|') {
                let data: String = cells(lines[index])
                    .iter()
                    .map(|cell| format!("<td>{}</td>", inlines(cell)))
                    .collect();
                rows.push_str(&format!("<tr>{data}</tr>"));
                index += 1;
            }
            out.push(format!(
                "<div class=\"c-table\"><table><thead><tr>{headers}</tr></thead><tbody>{rows}</tbody></table></div>"
            ));
            continue;
        }

        if list_item(line).is_some() {
            let (html, after) = list_block(&lines, index, &inlines);
            out.push(html);
            index = after;
            continue;
        }

        let mut paragraph = vec![line.trim()];
        index += 1;
        while index < lines.len() && !paragraph_ends(lines[index]) {
            paragraph.push(lines[index].trim());
            index += 1;
        }
        out.push(format!("<p>{}</p>", inlines(&paragraph.join(" "))));
    }
    out.join("\n")
}

/// The list starting at `index` and the index of the line after it.
fn list_block(
    lines: &[&str],
    mut index: usize,
    inlines: &dyn Fn(&str) -> String,
) -> (String, usize) {
    let (kind, indent, _) = list_item(lines[index]).unwrap_or((Kind::Bullet, 0, ""));
    let mut items = String::new();

    while index < lines.len() {
        let Some((_, item_indent, first)) = list_item(lines[index]) else {
            break;
        };
        if item_indent != indent {
            break;
        }
        let mut text = vec![first.to_owned()];
        let mut nested = String::new();
        index += 1;
        while index < lines.len() && !lines[index].trim().is_empty() {
            let line = lines[index];
            let deeper = list_item(line);
            if let Some((_, deeper_indent, _)) = deeper
                && deeper_indent > indent
            {
                (nested, index) = list_block(lines, index, inlines);
                continue;
            }
            if deeper.is_some() || fence(line).is_some() || heading(line).is_some() {
                break;
            }
            text.push(line.trim().to_owned());
            index += 1;
        }
        items.push_str(&format!("<li>{}{nested}</li>", inlines(&text.join(" "))));
        // A blank line between items of the same kind keeps the list going.
        if index + 1 < lines.len()
            && lines[index].trim().is_empty()
            && let Some((next_kind, next_indent, _)) = list_item(lines[index + 1])
            && next_kind == kind
            && next_indent == indent
        {
            index += 1;
        }
    }
    let tag = if kind == Kind::Numbered { "ol" } else { "ul" };
    (format!("<{tag}>{items}</{tag}>"), index)
}
