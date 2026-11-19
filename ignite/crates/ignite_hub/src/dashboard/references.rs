//! Ticket names in prose: finding them in text and turning them into links.
//!
//! A ticket is shown by its ID, a `T` and then capitals (`TQ44S.7`), or an `E` for a root
//! created as an epic (`EQ44S.7`). Text written before IDs
//! were stored in capitals names one as `Tq44s.7`. Text may also name one by its bare dotted
//! ID, by the project prefix (`proj-q44s.7`), or by the path of its page. These functions find
//! those names the way the old dashboard's regular expressions did, written by hand because the
//! workspace has no regex dependency.
// Pages are built from many small fragments; building them with `format!` reads better than
// the allocation-free forms these lints suggest.
#![allow(clippy::format_collect, clippy::format_push_string)]

use super::components::esc;

/// The name a ticket is shown by: its ID, which is stored as it is shown.
pub fn display_id(id: &str) -> String {
    id.to_owned()
}

/// The address of a ticket's page, under the dashboard's `root` (`/`): `/TQ44S.7`.
pub fn ticket_url(root: &str, id: &str) -> String {
    format!("{root}{}", percent_encode(&display_id(id)))
}

/// The dashboard's home. The hub serves it at its own root; only the API is under the project
/// name (`/<project>/api`).
pub fn dashboard_root() -> String {
    "/".to_owned()
}

pub fn percent_encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.!~*'()".contains(&byte) {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_'
}

/// Whether there is a word boundary before `chars[at]`.
fn boundary(chars: &[char], at: usize) -> bool {
    let before = at > 0 && word(chars[at - 1]);
    let after = chars.get(at).is_some_and(|&c| word(c));
    before != after
}

fn name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-'
}

/// Extends from `at` over `(\.[0-9]+)*`, longest first, to a position that ends a word.
fn dotted(chars: &[char], at: usize) -> Option<usize> {
    if chars.get(at) == Some(&'.') {
        let digits = chars[at + 1..]
            .iter()
            .take_while(|c| c.is_ascii_digit())
            .count();
        for count in (1..=digits).rev() {
            if let Some(end) = dotted(chars, at + 1 + count) {
                return Some(end);
            }
        }
    }
    boundary(chars, at).then_some(at)
}

/// The end of a ticket name `T?[A-Za-z0-9-]+(\.[0-9]+)*` that starts at `start` and ends at a word
/// boundary, preferring the longest.
fn name_at(chars: &[char], start: usize) -> Option<usize> {
    for skip in [usize::from(chars.get(start) == Some(&'T')), 0] {
        let base = start + skip;
        let run = chars[base.min(chars.len())..]
            .iter()
            .take_while(|&&c| name_char(c))
            .count();
        for length in (1..=run).rev() {
            if let Some(end) = dotted(chars, base + length) {
                return Some(end);
            }
        }
    }
    None
}

fn collect(chars: &[char]) -> String {
    chars.iter().collect()
}

/// Whether `name` has the shape of the root ID of a meta-ticket or of a ticket Ignite filed on
/// its own: `M` or `A` and four capitals or digits (`MAB12`, `AR2D2`), which ordinary words
/// written in capitals rarely are.
fn lettered_root(name: &str) -> bool {
    name.len() == 5
        && name.starts_with(['M', 'A'])
        && name
            .chars()
            .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
}

/// Calls `replace` on each ticket-like word of `text` and keeps the result. Bare names are
/// replaced only when they begin with `T` or `E`, begin with `<project>-`, are dotted IDs or
/// have the shape of a meta-ticket's or an automatic ticket's root, because a lone root such as `fix` is an ordinary word.
pub fn mentions(project: &str, text: &str, replace: &mut dyn FnMut(&str) -> String) -> String {
    let chars: Vec<char> = text.chars().collect();
    let prefix = format!("{project}-");
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let end = if boundary(&chars, index) {
            name_at(&chars, index)
        } else {
            None
        };
        if let Some(end) = end {
            let name = collect(&chars[index..end]);
            let dotted_id = name.split_once('.').is_some_and(|(head, tail)| {
                !head.is_empty()
                    && head.chars().all(name_char)
                    && tail.starts_with(|c: char| c.is_ascii_digit())
            });
            if name.starts_with(['T', 'E'])
                || name.starts_with(&prefix)
                || dotted_id
                || lettered_root(&name)
            {
                out.push_str(&replace(&name));
            } else {
                out.push_str(&name);
            }
            index = end;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// Calls `replace(id, path)` on each path of this project's ticket pages in `text`
/// in their earlier form (`/<project>/tickets/<id>`), when the path does not continue a longer word or path.
pub fn ticket_paths(
    project: &str,
    text: &str,
    replace: &mut dyn FnMut(&str, &str) -> String,
) -> String {
    let chars: Vec<char> = text.chars().collect();
    let head: Vec<char> = format!("/{project}/tickets/").chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut index = 0;
    while index < chars.len() {
        let free = index == 0
            || !(word(chars[index - 1])
                || chars[index - 1] == '.'
                || chars[index - 1] == '/'
                || chars[index - 1] == '-');
        if free
            && chars[index..].starts_with(&head)
            && let Some(end) = name_at(&chars, index + head.len())
        {
            let id = collect(&chars[index + head.len()..end]);
            out.push_str(&replace(&id, &collect(&chars[index..end])));
            index = end;
        } else {
            out.push(chars[index]);
            index += 1;
        }
    }
    out
}

/// Rewrites this project's ticket page paths in `text` to the canonical address of the ticket
/// `resolve` finds, leaving other paths alone.
pub fn rewrite_ticket_paths(
    project: &str,
    text: &str,
    resolve: &dyn Fn(&str) -> Option<String>,
) -> String {
    let root = dashboard_root();
    ticket_paths(project, text, &mut |name, path| {
        resolve(name).map_or_else(|| path.to_owned(), |id| ticket_url(&root, &id))
    })
}

/// Escapes a ticket name for the places pages show it.
pub fn escaped_display(id: &str) -> String {
    esc(&display_id(id))
}

#[cfg(test)]
mod tests {
    use super::mentions;

    #[test]
    fn epic_ids_with_an_e_are_found_like_ids_with_a_t() {
        let mut seen = Vec::new();
        mentions("proj", "See EQ44S.7 and TR2D2, not fix.", &mut |name| {
            seen.push(name.to_owned());
            format!("<{name}>")
        });
        assert_eq!(seen, ["EQ44S.7", "TR2D2"]);
    }

    #[test]
    fn automatic_and_meta_root_ids_are_found_but_other_capital_words_are_not() {
        let mut seen = Vec::new();
        mentions("proj", "AR2D2 and MAB12 filed, ALL ACTIVE.", &mut |name| {
            seen.push(name.to_owned());
            name.to_owned()
        });
        assert_eq!(seen, ["AR2D2", "MAB12"]);
    }
}
