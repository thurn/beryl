//! Mana value calculation from printed mana cost strings.

/// What the mana symbols of `cost`, written as `{1}{U}{U}`, add up to (`FaceList::mana_value`).
pub(super) fn cost_value(cost: &str) -> u32 {
    (cost.split('{').skip(1))
        .filter_map(|part| part.split_once('}'))
        .map(|(symbol, _)| match symbol {
            "X" | "Y" | "Z" => 0,
            _ => (symbol.split('/').next())
                .and_then(|generic| generic.parse().ok())
                .unwrap_or(1),
        })
        .sum()
}
