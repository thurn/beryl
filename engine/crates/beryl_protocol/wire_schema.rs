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
    if is(name, "doc") {
        return None;
    }
    // Canonicalize derive lists so a trailing comma does not change the handshake.
    if is(name, "derive")
        && let [TokenTree::Group(arguments)] = rest
    {
        let tokens: Vec<_> = arguments.stream().into_iter().collect();
        let paths = tokens
            .split(|token| is(token, ","))
            .filter(|clause| !clause.is_empty())
            .map(|clause| clause.iter().cloned().collect::<TokenStream>().to_string())
            .collect::<Vec<_>>();
        let attribute = format!("derive({})", paths.join(","));
        return Some(Group::new(
            Delimiter::Bracket,
            attribute.parse().expect("the derive paths are Rust tokens"),
        ));
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
