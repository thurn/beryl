//! The order of the milestones on an epic's page. Milestones are steps of a plan, so they are
//! listed in the order the plan runs them: a milestone comes after the milestones of the same
//! epic that it depends on. Where dependencies leave the order open, the number in each title
//! decides ("M0" before "M1" before "M7"), and titles with no number come last. Status and
//! priority play no part, so a finished milestone keeps its place at the top.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use super::data::{Card, by_ticket_id};

/// Whether a child is listed as a milestone rather than a subtask.
pub fn is_milestone(card: &Card) -> bool {
    card.ticket_type == "epic" || card.ticket_type == "milestone"
}

/// Orders `milestones` so that each comes after the ones it depends on. `depends` holds
/// `(dependent, dependency)` pairs between them; pairs naming anything else are ignored. A
/// cycle cannot be ordered by its dependencies, so its members follow the rest in title order.
pub fn order(milestones: &mut [Card], depends: &[(String, String)]) {
    let ids: HashSet<&str> = milestones.iter().map(|card| card.id.as_str()).collect();
    let mut waiting: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (dependent, dependency) in depends {
        if dependent != dependency
            && ids.contains(dependent.as_str())
            && ids.contains(dependency.as_str())
        {
            waiting
                .entry(dependent.as_str())
                .or_default()
                .insert(dependency.as_str());
        }
    }
    let mut left: Vec<&Card> = milestones.iter().collect();
    left.sort_by(|a, b| by_title_number(a, b));
    let mut placed: HashSet<&str> = HashSet::new();
    let mut sorted: Vec<String> = Vec::with_capacity(left.len());
    while !left.is_empty() {
        let ready = left.iter().position(|card| {
            waiting
                .get(card.id.as_str())
                .is_none_or(|needs| needs.iter().all(|need| placed.contains(need)))
        });
        // With nothing ready the rest form a cycle; take them in title order.
        let card = left.remove(ready.unwrap_or(0));
        placed.insert(card.id.as_str());
        sorted.push(card.id.clone());
    }
    let rank: HashMap<String, usize> = sorted
        .into_iter()
        .enumerate()
        .map(|(index, id)| (id, index))
        .collect();
    milestones.sort_by_key(|card| rank.get(&card.id).copied());
}

/// Orders by the first number in the title, titles with none last, then by id with its
/// numbered parts compared as numbers, so that `.9` comes before `.10`.
fn by_title_number(left: &Card, right: &Card) -> Ordering {
    let key = |card: &Card| title_number(&card.title).unwrap_or(u64::MAX);
    key(left)
        .cmp(&key(right))
        .then_with(|| by_ticket_id(left, right))
}

fn title_number(title: &str) -> Option<u64> {
    let start = title.find(|c: char| c.is_ascii_digit())?;
    let digits: String = title[start..]
        .chars()
        .take_while(char::is_ascii_digit)
        .collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use ignite_core::protocol::tickets::Status;

    use super::*;

    fn card(id: &str, title: &str) -> Card {
        Card {
            id: id.to_owned(),
            parent: Some("TQ44S".to_owned()),
            title: title.to_owned(),
            ticket_type: "epic".to_owned(),
            status: Status::Open,
            priority: 2,
            assignee: None,
            updated_at: String::new(),
            owner: None,
        }
    }

    fn pair(dependent: &str, dependency: &str) -> (String, String) {
        (dependent.to_owned(), dependency.to_owned())
    }

    fn titles(cards: &[Card]) -> Vec<&str> {
        cards.iter().map(|card| card.title.as_str()).collect()
    }

    #[test]
    fn dependencies_come_first_and_title_numbers_break_ties() {
        let mut cards = vec![
            card("TQ44S.91", "M7: tools"),
            card("TQ44S.12", "Final review gate"),
            card("TQ44S.2", "M1 review gate"),
            card("TQ44S.3", "M2: the table plays"),
            card("TQ44S.1", "M1: foundations"),
            card("TQ44S.14", "M0: a prototype"),
        ];
        let depends = [
            pair("TQ44S.1", "TQ44S.14"),
            pair("TQ44S.2", "TQ44S.1"),
            pair("TQ44S.3", "TQ44S.2"),
            pair("TQ44S.12", "TQ44S.3"),
            pair("TQ44S.3", "TOUTSIDE"),
        ];
        order(&mut cards, &depends);
        assert_eq!(
            titles(&cards),
            [
                "M0: a prototype",
                "M1: foundations",
                "M1 review gate",
                "M2: the table plays",
                "M7: tools",
                "Final review gate",
            ]
        );
    }

    #[test]
    fn a_dependency_outranks_the_title_number() {
        let mut cards = vec![card("TQ44S.1", "M1"), card("TQ44S.2", "M2")];
        order(&mut cards, &[pair("TQ44S.1", "TQ44S.2")]);
        assert_eq!(titles(&cards), ["M2", "M1"]);
    }

    #[test]
    fn a_cycle_falls_back_to_title_numbers() {
        let mut cards = vec![
            card("TQ44S.10", "Gate"),
            card("TQ44S.9", "Gate"),
            card("TQ44S.3", "M3"),
        ];
        order(
            &mut cards,
            &[pair("TQ44S.3", "TQ44S.10"), pair("TQ44S.10", "TQ44S.3")],
        );
        assert_eq!(
            cards
                .iter()
                .map(|card| card.id.as_str())
                .collect::<Vec<_>>(),
            ["TQ44S.9", "TQ44S.3", "TQ44S.10"]
        );
    }
}
