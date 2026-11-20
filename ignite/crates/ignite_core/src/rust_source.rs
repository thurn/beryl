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
}
