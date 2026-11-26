//! A Rust source file's code without its comments, and an object ID that stands for it, so a
//! family whose listing cannot depend on comments keeps its listing across a change that only
//! edits comments (see `listing_ignores_comments` in "Declaring a family", `docs/ignite.md`).
//!
//! A landing that changed one doc comment in one card file built eight products, 1,332 s of
//! builds, only to list families whose checks the comment could not change: every family whose
//! listing hash covered the card file missed its kept listing.

use std::fmt::Write as _;

use sha2::{Digest, Sha256};

use crate::ids::Oid;

/// `source` without its comments, line, block and doc comments alike, and without the
/// whitespace between tokens, except one space where two words or numbers would otherwise run
/// together (`pub fn`). String, raw string, byte string and character literals are kept as they
/// are, so a `//` inside a string is code.
///
/// Doc comments go too: a family that sets `listing_ignores_comments` says its `list` reads
/// neither, which holds for test names, compiled behavior and the code of a card file.
#[must_use]
pub fn code_only(source: &str) -> String {
    let chars: Vec<char> = source.chars().collect();
    let mut out = String::with_capacity(source.len());
    let mut gap = false;
    let mut i = 0;
    // A space is written only between two characters that would otherwise join into one
    // word, so `{}` and `{ }`, or `a+b` and `a + b`, give the same text.
    let emit_gap = |out: &mut String, gap: &mut bool, next: char| {
        if *gap && out.chars().next_back().is_some_and(is_ident) && is_ident(next) {
            out.push(' ');
        }
        *gap = false;
    };
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c.is_whitespace() {
            gap = true;
            i += 1;
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            gap = true;
        } else if c == '/' && next == Some('*') {
            i = block_comment_end(&chars, i);
            gap = true;
        } else if c == '"' {
            emit_gap(&mut out, &mut gap, c);
            let end = string_end(&chars, i);
            out.extend(&chars[i..end]);
            i = end;
        } else if c == '\'' {
            emit_gap(&mut out, &mut gap, c);
            let end = quote_end(&chars, i);
            out.extend(&chars[i..end]);
            i = end;
        } else if is_ident(c) {
            emit_gap(&mut out, &mut gap, c);
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            out.push_str(&word);
            if matches!(word.as_str(), "r" | "br" | "cr")
                && let Some(end) = raw_string_end(&chars, i)
            {
                out.extend(&chars[i..end]);
                i = end;
            }
        } else {
            emit_gap(&mut out, &mut gap, c);
            out.push(c);
            i += 1;
        }
    }
    out
}

/// An object ID for a Rust file's [`code_only`] text: the SHA-256 of that text in hex. Two
/// files that differ only in comments and whitespace get the same one.
#[must_use]
pub fn code_id(source: &str) -> Oid {
    let digest = Sha256::digest(code_only(source).as_bytes());
    let hex = digest.iter().fold(String::new(), |mut hex, byte| {
        let _ = write!(hex, "{byte:02x}");
        hex
    });
    Oid::new(hex).unwrap_or_else(|_| unreachable!("SHA-256 gives 64 hex digits"))
}

fn is_ident(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// The index after the block comment that starts at `start`. Block comments nest in Rust.
fn block_comment_end(chars: &[char], start: usize) -> usize {
    let mut depth = 0usize;
    let mut i = start;
    while i < chars.len() {
        match (chars[i], chars.get(i + 1).copied()) {
            ('/', Some('*')) => {
                depth += 1;
                i += 2;
            }
            ('*', Some('/')) => {
                depth -= 1;
                i += 2;
                if depth == 0 {
                    return i;
                }
            }
            _ => i += 1,
        }
    }
    chars.len()
}

/// The index after the string literal whose opening `"` is at `start`.
fn string_end(chars: &[char], start: usize) -> usize {
    let mut i = start + 1;
    while i < chars.len() {
        match chars[i] {
            '\\' => i += 2,
            '"' => return i + 1,
            _ => i += 1,
        }
    }
    chars.len()
}

/// The index after the character literal, lifetime or label whose `'` is at `start`. A
/// character literal is `'x'` or an escape such as `'\n'`; anything else is a lifetime or a
/// label, of which only the `'` is taken here.
fn quote_end(chars: &[char], start: usize) -> usize {
    match (chars.get(start + 1), chars.get(start + 2)) {
        (Some('\\'), _) => {
            let mut i = start + 2;
            while i < chars.len() && chars[i] != '\'' {
                i += if chars[i] == '\\' { 2 } else { 1 };
            }
            (i + 1).min(chars.len())
        }
        (Some(_), Some('\'')) => start + 3,
        _ => start + 1,
    }
}

/// The index after a raw string literal whose `#`s or opening `"` start at `start`, just after
/// its `r`, `br` or `cr`; `None` when no raw string starts there.
fn raw_string_end(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while chars.get(i) == Some(&'#') {
        i += 1;
    }
    let hashes = i - start;
    if chars.get(i) != Some(&'"') {
        return None;
    }
    i += 1;
    while i < chars.len() {
        if chars[i] == '"' && (1..=hashes).all(|n| chars.get(i + n) == Some(&'#')) {
            return Some(i + 1 + hashes);
        }
        i += 1;
    }
    Some(chars.len())
}

/// Returns `source` with its comments, insignificant whitespace and `#[cfg(test)]` items
/// removed, or `None` if an attribute or block delimiter is unbalanced.
///
/// Two versions of a Rust file that produce the same `Some(...)` text changed only comments,
/// formatting, or `#[cfg(test)]` code. `#[cfg(all(..., test, ...))]` is also stripped because
/// it only compiles under `cfg(test)`; `#[cfg(any(test, ...))]` and `#[cfg(not(test))]` are
/// kept because they compile in non-test builds.
#[must_use]
pub fn production_code(source: &str) -> Option<String> {
    let tokens = tokenize(source);
    let mut out = String::with_capacity(source.len());
    let mut i = 0;
    while i < tokens.len() {
        if tokens[i] == Token::Punct('#')
            && tokens.get(i + 1) == Some(&Token::Punct('['))
            && let Some(attr_end) = matching_delimiter(&tokens, i + 1, '[', ']')
            && is_cfg_test(&tokens[i + 2..attr_end])
        {
            i = skip_item(&tokens, attr_end + 1)?;
            continue;
        }
        emit_token(&mut out, &tokens[i]);
        i += 1;
    }
    Some(out)
}

/// Whether changing a Rust file from `before` to `after` touched only comments, whitespace, or
/// `#[cfg(test)]` items. Returns `false` if either version fails to parse cleanly.
#[must_use]
pub fn is_test_only_change(before: &str, after: &str) -> bool {
    match (production_code(before), production_code(after)) {
        (Some(old), Some(new)) => old == new,
        _ => false,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Ident(String),
    Literal(String),
    Punct(char),
}

fn tokenize(source: &str) -> Vec<Token> {
    let chars: Vec<char> = source.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();
        if c.is_whitespace() {
            i += 1;
        } else if c == '/' && next == Some('/') {
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
        } else if c == '/' && next == Some('*') {
            i = block_comment_end(&chars, i);
        } else if c == '"' {
            let end = string_end(&chars, i);
            tokens.push(Token::Literal(chars[i..end].iter().collect()));
            i = end;
        } else if c == '\'' {
            let end = quote_end(&chars, i);
            if end == i + 1 {
                tokens.push(Token::Punct('\''));
            } else {
                tokens.push(Token::Literal(chars[i..end].iter().collect()));
            }
            i = end;
        } else if is_ident(c) {
            let start = i;
            while i < chars.len() && is_ident(chars[i]) {
                i += 1;
            }
            let word: String = chars[start..i].iter().collect();
            if matches!(word.as_str(), "r" | "br" | "cr")
                && let Some(end) = raw_string_end(&chars, i)
            {
                let mut lit = word;
                lit.extend(&chars[i..end]);
                tokens.push(Token::Literal(lit));
                i = end;
            } else if matches!(word.as_str(), "b" | "c") && chars.get(i) == Some(&'"') {
                let end = string_end(&chars, i);
                let mut lit = word;
                lit.extend(&chars[i..end]);
                tokens.push(Token::Literal(lit));
                i = end;
            } else if word == "b" && chars.get(i) == Some(&'\'') {
                let end = quote_end(&chars, i);
                let mut lit = word;
                lit.extend(&chars[i..end]);
                tokens.push(Token::Literal(lit));
                i = end;
            } else {
                tokens.push(Token::Ident(word));
            }
        } else {
            tokens.push(Token::Punct(c));
            i += 1;
        }
    }
    tokens
}

fn emit_token(out: &mut String, token: &Token) {
    match token {
        Token::Ident(word) => {
            if out.chars().next_back().is_some_and(is_ident) {
                out.push(' ');
            }
            out.push_str(word);
        }
        Token::Literal(lit) => {
            if out.chars().next_back().is_some_and(is_ident)
                && lit.chars().next().is_some_and(is_ident)
            {
                out.push(' ');
            }
            out.push_str(lit);
        }
        Token::Punct(c) => out.push(*c),
    }
}

fn matching_delimiter(tokens: &[Token], open_idx: usize, open: char, close: char) -> Option<usize> {
    let mut depth = 0usize;
    for (idx, token) in tokens.iter().enumerate().skip(open_idx) {
        if *token == Token::Punct(open) {
            depth += 1;
        } else if *token == Token::Punct(close) {
            depth -= 1;
            if depth == 0 {
                return Some(idx);
            }
        }
    }
    None
}

/// Whether the tokens inside `#[...]` are `cfg(test)` or `cfg(all(..., test, ...))`.
fn is_cfg_test(inner: &[Token]) -> bool {
    if inner.len() < 4
        || inner[0] != Token::Ident("cfg".to_owned())
        || inner[1] != Token::Punct('(')
        || matching_delimiter(inner, 1, '(', ')') != Some(inner.len() - 1)
    {
        return false;
    }
    let args = &inner[2..inner.len() - 1];
    if args == [Token::Ident("test".to_owned())] {
        return true;
    }
    if args.len() >= 4
        && args[0] == Token::Ident("all".to_owned())
        && args[1] == Token::Punct('(')
        && matching_delimiter(args, 1, '(', ')') == Some(args.len() - 1)
    {
        return top_level_comma_parts(&args[2..args.len() - 1])
            .any(|part| part == [Token::Ident("test".to_owned())]);
    }
    false
}

fn top_level_comma_parts(tokens: &[Token]) -> impl Iterator<Item = &[Token]> {
    let mut parts = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;
    for (idx, token) in tokens.iter().enumerate() {
        match token {
            Token::Punct('(' | '[' | '{') => depth += 1,
            Token::Punct(')' | ']' | '}') => depth = depth.saturating_sub(1),
            Token::Punct(',') if depth == 0 => {
                parts.push(&tokens[start..idx]);
                start = idx + 1;
            }
            _ => {}
        }
    }
    if start < tokens.len() {
        parts.push(&tokens[start..]);
    }
    parts.into_iter()
}

/// Skips the item or statement starting at `i`, including any further outer attributes on it.
fn skip_item(tokens: &[Token], mut i: usize) -> Option<usize> {
    while tokens.get(i) == Some(&Token::Punct('#')) && tokens.get(i + 1) == Some(&Token::Punct('['))
    {
        i = matching_delimiter(tokens, i + 1, '[', ']')? + 1;
    }
    let mut paren = 0usize;
    let mut bracket = 0usize;
    let mut brace = 0usize;
    let mut ends_with_semi = false;
    while i < tokens.len() {
        let at_top = paren == 0 && bracket == 0 && brace == 0;
        match &tokens[i] {
            Token::Ident(word) if at_top => {
                match word.as_str() {
                    "use" | "const" | "static" | "let" | "type" => ends_with_semi = true,
                    // `const fn` has a `{ ... }` body rather than ending at `;`.
                    "fn" => ends_with_semi = false,
                    _ => {}
                }
                i += 1;
            }
            Token::Punct('(') => {
                paren += 1;
                i += 1;
            }
            Token::Punct(')') => {
                paren = paren.checked_sub(1)?;
                i += 1;
            }
            Token::Punct('[') => {
                bracket += 1;
                i += 1;
            }
            Token::Punct(']') => {
                bracket = bracket.checked_sub(1)?;
                i += 1;
            }
            Token::Punct(';') if at_top => return Some(i + 1),
            Token::Punct('{') if at_top && !ends_with_semi => {
                return Some(matching_delimiter(tokens, i, '{', '}')? + 1);
            }
            Token::Punct('{') => {
                brace += 1;
                i += 1;
            }
            Token::Punct('}') => {
                brace = brace.checked_sub(1)?;
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn comments_and_layout_do_not_count() {
        let before = "//! A card.\nuse a::b;\n\n/// It exiles.\npub static X: u8 = 1; // one\n";
        let after = "use a::b; /* a note\n /* nested */ */ pub static X: u8 =\n    1;\n";
        assert_eq!(code_only(before), "use a::b;pub static X:u8=1;");
        assert_eq!(code_id("fn a() {}"), code_id("fn a ( ) { }"));
        assert_eq!(code_id(before), code_id(after));
    }

    #[test]
    fn code_counts() {
        assert_ne!(code_id("fn a() {}"), code_id("fn b() {}"));
        assert_ne!(code_id("let x = 1;"), code_id("let x = 2;"));
    }

    #[test]
    fn literals_are_code() {
        assert_eq!(code_only(r#"let s = "a // b";"#), r#"let s="a // b";"#);
        assert_eq!(
            code_only("let s = r#\"x \" /* y */\"#; // z"),
            "let s=r#\"x \" /* y */\"#;"
        );
        assert_eq!(code_only(r"let c = '\''; // q"), r"let c='\'';");
        assert_eq!(code_only("let c = '\"'; // q"), "let c='\"';");
        assert_eq!(
            code_only("fn f<'a>(x: &'a mut str) {} // q"),
            "fn f<'a>(x:&'a mut str){}"
        );
        assert_ne!(code_id(r#"f("a  b")"#), code_id(r#"f("a b")"#));
    }

    #[test]
    fn cfg_test_items_are_excluded_from_production_code() {
        let before = r"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn works() {
        assert_eq!(add(1, 2), 3);
    }
}
";
        let after = r"
pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;

    #[test]
    fn works() {
        assert_eq!(add(2, 3), 5);
    }
}
";
        assert!(is_test_only_change(before, after));

        let prod_edit = before.replace("a + b", "a - b");
        assert!(!is_test_only_change(before, &prod_edit));
    }

    #[test]
    fn cfg_test_handles_use_const_all_and_keeps_any_or_not() {
        let before = r#"
#[cfg(test)]
use std::collections::{BTreeMap, HashMap};

#[cfg(test)]
const S: Foo = Foo { x: 1 };

#[cfg(all(test, unix))]
fn helper() -> [u8; 2] { [1, 2] }

#[cfg(test)]
const fn test_const() -> usize { 1 }

#[cfg(any(test, feature = "extra"))]
pub fn kept_any() -> usize { 1 }

#[cfg(not(test))]
pub fn kept_not() -> usize { 2 }
"#;
        let after_test_only = before
            .replace("HashMap", "HashSet")
            .replace("x: 1", "x: 2")
            .replace("[1, 2]", "[3, 4]")
            .replace("test_const() -> usize { 1 }", "test_const() -> usize { 9 }");
        assert!(is_test_only_change(before, &after_test_only));

        let after_any = before.replace("kept_any() -> usize { 1 }", "kept_any() -> usize { 9 }");
        assert!(!is_test_only_change(before, &after_any));

        let after_not = before.replace("kept_not() -> usize { 2 }", "kept_not() -> usize { 9 }");
        assert!(!is_test_only_change(before, &after_not));
    }

    #[test]
    fn unbalanced_cfg_test_falls_back_to_product() {
        let broken = "pub fn f() {}\n#[cfg(test)]\nmod tests {\n";
        assert_eq!(production_code(broken), None);
        assert!(!is_test_only_change(broken, broken));
    }
}
