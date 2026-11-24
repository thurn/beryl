use super::{Deck, DeckError};

/// Reads Cockatrice's deck name and main and side zones. Printing attributes,
/// other zones, and metadata such as sideboard plans do not affect the deck list.
pub fn parse_cod(xml: &str) -> Result<Deck, DeckError> {
    let mut input = xml.trim_start_matches('\u{feff}');
    let mut deck = Deck::default();
    let mut open = Vec::new();
    let mut root_seen = false;
    let mut zone = None;
    while !input.is_empty() {
        let event = next(&mut input).ok_or(DeckError::InvalidCod)?;
        match event {
            Event::Text(text, escaped) => {
                if open.len() == 2 && open.last() == Some(&"deckname") {
                    if escaped {
                        deck.name
                            .push_str(&unescape(text).ok_or(DeckError::InvalidCod)?);
                    } else {
                        deck.name.push_str(text);
                    }
                } else if open.is_empty() && !text.trim().is_empty() {
                    return Err(DeckError::InvalidCod);
                }
            }
            Event::Start(tag, attributes, empty) => {
                let attributes = Attributes::parse(attributes).ok_or(DeckError::InvalidCod)?;
                match (open.as_slice(), tag) {
                    ([], "cockatrice_deck") if !root_seen => root_seen = true,
                    ([], _) => return Err(DeckError::InvalidCod),
                    ([_], "deckname") => deck.name.clear(),
                    ([_], "zone") => {
                        zone = match attributes.name {
                            Some("main") if !empty => Some(&mut deck.main),
                            Some("side") if !empty => Some(&mut deck.side),
                            _ => None,
                        };
                    }
                    ([_, "zone"], "card") => {
                        if let Some(cards) = &mut zone {
                            cards.push(attributes.card().ok_or(DeckError::InvalidCod)?);
                        }
                    }
                    _ => {}
                }
                if !empty {
                    open.push(tag);
                }
            }
            Event::End(tag) => {
                if open.pop() != Some(tag) {
                    return Err(DeckError::InvalidCod);
                }
                if open.len() == 1 && tag == "zone" {
                    zone = None;
                }
            }
            Event::Ignore => {}
        }
    }
    if !root_seen || !open.is_empty() {
        return Err(DeckError::InvalidCod);
    }
    Ok(deck)
}

enum Event<'a> {
    Text(&'a str, bool),
    Start(&'a str, &'a str, bool),
    End(&'a str),
    Ignore,
}

// The exporter writes plain tags, quoted attributes and text. Declarations,
// comments and CDATA have their own terminators; a quoted '>' is not a tag end.
fn next<'a>(input: &mut &'a str) -> Option<Event<'a>> {
    for (prefix, end) in [("<!--", "-->"), ("<?", "?>"), ("<![CDATA[", "]]>")] {
        if let Some(body) = input.strip_prefix(prefix) {
            let (text, rest) = body.split_once(end)?;
            *input = rest;
            return Some(if prefix == "<![CDATA[" {
                Event::Text(text, false)
            } else {
                Event::Ignore
            });
        }
    }
    let Some(body) = input.strip_prefix('<') else {
        let end = input.find('<').unwrap_or(input.len());
        let (text, rest) = input.split_at(end);
        *input = rest;
        return Some(Event::Text(text, true));
    };
    let mut quote = None;
    let end = body.char_indices().find_map(|(index, character)| {
        if let Some(active) = quote {
            if character == active {
                quote = None;
            }
        } else {
            match character {
                '\'' | '"' => quote = Some(character),
                '>' => return Some(Some(index)),
                '<' => return Some(None),
                _ => {}
            }
        }
        None
    })??;
    let tag = body[..end].trim_end();
    *input = &body[end + 1..];
    if let Some(name) = tag.strip_prefix('/') {
        return is_name(name).then_some(Event::End(name));
    }
    let (tag, empty) = tag
        .strip_suffix('/')
        .map_or((tag, false), |tag| (tag.trim_end(), true));
    let name_end = tag.find(char::is_whitespace).unwrap_or(tag.len());
    let (name, attributes) = tag.split_at(name_end);
    is_name(name).then_some(Event::Start(name, attributes, empty))
}

fn is_name(name: &str) -> bool {
    name.as_bytes()
        .first()
        .is_some_and(|first| first.is_ascii_alphabetic() || *first == b'_')
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
}

#[derive(Default)]
struct Attributes<'a> {
    name: Option<&'a str>,
    number: Option<&'a str>,
}

impl<'a> Attributes<'a> {
    fn parse(mut input: &'a str) -> Option<Self> {
        let mut attributes = Self::default();
        while !input.is_empty() {
            let trimmed = input.trim_start();
            if trimmed.len() == input.len() {
                return None;
            }
            let (key, value) = trimmed.split_once('=')?;
            let key = key.trim_end();
            if !is_name(key) {
                return None;
            }
            let value = value.trim_start();
            let quote = *value.as_bytes().first()?;
            if !matches!(quote, b'\'' | b'"') {
                return None;
            }
            let (value, rest) = value[1..].split_once(char::from(quote))?;
            if value.contains('<') {
                return None;
            }
            let slot = match key {
                "name" => Some(&mut attributes.name),
                "number" => Some(&mut attributes.number),
                _ => None,
            };
            if let Some(slot) = slot
                && slot.replace(value).is_some()
            {
                return None;
            }
            input = rest;
        }
        Some(attributes)
    }

    fn card(&self) -> Option<(String, u16)> {
        let name = unescape(self.name?)?;
        let count = self.number?.parse().ok()?;
        (!name.trim().is_empty() && count != 0).then_some((name, count))
    }
}

fn unescape(mut input: &str) -> Option<String> {
    let mut text = String::with_capacity(input.len());
    while let Some((before, rest)) = input.split_once('&') {
        text.push_str(before);
        let (entity, rest) = rest.split_once(';')?;
        text.push(match entity {
            "amp" => '&',
            "apos" => '\'',
            "gt" => '>',
            "lt" => '<',
            "quot" => '"',
            _ => {
                let number = match entity.strip_prefix("#x") {
                    Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                    None => entity.strip_prefix('#')?.parse().ok()?,
                };
                char::from_u32(number)?
            }
        });
        input = rest;
    }
    text.push_str(input);
    Some(text)
}
