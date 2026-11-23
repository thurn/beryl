//! Keep the Rust tokens that can affect the JSON schema, including serializer bodies.

use proc_macro2::{Delimiter, Group, TokenStream, TokenTree};

pub(super) fn fingerprint(hash: u64, source: &str) -> Result<u64, proc_macro2::LexError> {
    let schema = normalize(source.parse()?).to_string();
    Ok(super::wire_fingerprint::definition(hash, &schema))
}

fn normalize(tokens: TokenStream) -> TokenStream {
    let mut output = TokenStream::new();
    let tokens: Vec<_> = tokens.into_iter().collect();
    let mut remaining = tokens.as_slice();
    while let Some((first, rest)) = remaining.split_first() {
        if is(first, "#") {
            let attribute_index = usize::from(rest.first().is_some_and(|token| is(token, "!")));
            if let Some(TokenTree::Group(group)) = rest.get(attribute_index)
                && group.delimiter() == Delimiter::Bracket
            {
                if let Some(attribute) = normalize_attribute(group) {
                    output.extend(remaining[..=attribute_index].iter().cloned());
                    output.extend([TokenTree::Group(attribute)]);
                }
                remaining = &rest[attribute_index + 1..];
                continue;
            }
        }
        if is(first, "use")
            && let Some((import, tail)) = rest.split_at_checked(5)
            && import
                .iter()
                .zip(["ts_rs", ":", ":", "TS", ";"])
                .all(|(token, text)| is(token, text))
        {
            remaining = tail;
            continue;
        }

        let token = match first {
            TokenTree::Group(group) => {
                TokenTree::Group(Group::new(group.delimiter(), normalize(group.stream())))
            }
            token => token.clone(),
        };
        output.extend([token]);
        remaining = rest;
    }
    output
}

fn normalize_attribute(group: &Group) -> Option<Group> {
    let tokens: Vec<_> = group.stream().into_iter().collect();
    let (name, rest) = tokens.split_first()?;
    if is(name, "doc") || is(name, "ts") {
        return None;
    }
    if let [TokenTree::Group(arguments)] = rest {
        let clauses: Vec<_> = arguments.stream().into_iter().collect();
        let mut clauses = clauses
            .split(|token| is(token, ","))
            .filter(|clause| !clause.is_empty())
            .map(|clause| clause.iter().cloned().collect::<TokenStream>().to_string());
        // The manual table uses cfg_attr for its optional legacy exports.
        if is(name, "cfg_attr")
            && clauses.next().as_deref() == Some("feature = \"ts\"")
            && clauses.all(|clause| clause == "derive (ts_rs :: TS)" || clause.starts_with("ts ("))
        {
            return None;
        }
        if is(name, "derive") {
            let paths = clauses
                .filter(|path| path != "TS" && path != "ts_rs :: TS")
                .collect::<Vec<_>>();
            if paths.is_empty() {
                return None;
            }
            let attribute = format!("derive({})", paths.join(","));
            return Some(Group::new(
                Delimiter::Bracket,
                attribute
                    .parse()
                    .expect("the retained derive paths are Rust tokens"),
            ));
        }
    }
    Some(group.clone())
}

fn is(token: &TokenTree, text: &str) -> bool {
    match token {
        TokenTree::Ident(ident) => ident == text,
        TokenTree::Punct(punct) => {
            let mut buffer = [0; 4];
            punct.as_char().encode_utf8(&mut buffer) == text
        }
        _ => false,
    }
}
