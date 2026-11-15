//! Read fixed rules-text forms for optional manual table actions.

use std::collections::BTreeSet;

use beryl_card_store::Face;

use crate::action::{Adjustment, Destination, Flow as FlowKind, Position, Selection};
use crate::state::{ObjectId, Zone};
use crate::{Action, Refusal, Seat};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoyaltyCost {
    Plus(u16),
    Minus(u16),
    MinusX,
    Zero,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AbilityRow {
    Loyalty { cost: LoyaltyCost, text: String },
    Activated { cost: String, text: String },
}

/// Masks quoted text and reminder text while preserving byte positions.
/// The separator keeps words on either side from forming a match.
fn visible_text(text: &str) -> String {
    let mut visible = String::with_capacity(text.len());
    let mut close_quote = None;
    let mut parentheses = 0usize;
    for character in text.chars() {
        let closing_quote = close_quote == Some(character);
        if close_quote.is_none() {
            match character {
                '"' => close_quote = Some('"'),
                '“' => close_quote = Some('”'),
                '(' => parentheses += 1,
                _ => {}
            }
        }
        if character != '\n' && (close_quote.is_some() || parentheses != 0) {
            visible.extend(std::iter::repeat_n('|', character.len_utf8()));
        } else {
            visible.push(character);
        }
        if closing_quote {
            close_quote = None;
        } else if close_quote.is_none() && character == ')' {
            parentheses = parentheses.saturating_sub(1);
        }
    }
    visible
}

fn loyalty_cost(cost: &str) -> Option<LoyaltyCost> {
    if cost == "0" {
        return Some(LoyaltyCost::Zero);
    }
    if let Some(amount) = cost.strip_prefix('+') {
        return amount.parse().ok().map(LoyaltyCost::Plus);
    }
    match cost.strip_prefix(['-', '−'])? {
        "X" => Some(LoyaltyCost::MinusX),
        amount => amount.parse().ok().map(LoyaltyCost::Minus),
    }
}

fn activated_cost(prefix: &str) -> Option<&str> {
    // A charge threshold, ability word or printed name may precede the cost.
    let prefix = prefix
        .split_once(" | ")
        .map_or(prefix, |(threshold, cost)| {
            if threshold
                .strip_suffix('+')
                .is_some_and(|n| n.parse::<u16>().is_ok())
            {
                cost
            } else {
                prefix
            }
        });
    let cost = prefix.split_once(" — ").map_or(prefix, |(_, cost)| cost);
    let first_word = cost.split_whitespace().next()?;
    (cost.starts_with('{')
        || matches!(
            first_word,
            "Sacrifice"
                | "Tap"
                | "Untap"
                | "Remove"
                | "Discard"
                | "Pay"
                | "Exile"
                | "Return"
                | "Put"
                | "Reveal"
                | "Collect"
                | "Waterbend"
        ))
    .then_some(cost)
}

/// Reads the card's own abilities written as “[Cost]: [Effect.]” ([CR 602.1]).
/// [CR 602.1a] calls the text “before the colon” the activation cost.
///
/// [CR 602.1]: https://yawgatog.com/resources/magic-rules/#R6021
/// [CR 602.1a]: https://yawgatog.com/resources/magic-rules/#R6021a
#[must_use]
pub fn abilities(face: &Face) -> Vec<AbilityRow> {
    let text = face.oracle_text.as_deref().unwrap_or_default();
    let visible = visible_text(text);
    text.lines()
        .zip(visible.lines())
        .filter_map(|(paragraph, visible)| {
            let colon = visible.find(':')?;
            let cost = paragraph[..colon].trim();
            let text = paragraph[colon + 1..].trim();
            if text.is_empty() {
                return None;
            }
            if let Some(cost) = loyalty_cost(cost) {
                Some(AbilityRow::Loyalty {
                    cost,
                    text: text.into(),
                })
            } else {
                Some(AbilityRow::Activated {
                    cost: activated_cost(cost)?.into(),
                    text: text.into(),
                })
            }
        })
        .collect()
}

/// Puts a marker on the stack from a permanent on the battlefield.
/// Loyalty costs “put on or remove from that permanent a certain number of
/// loyalty counters” ([CR 606.4]). The caller supplies the player's X choice
/// for a `MinusX` row. An absent X is treated as zero.
///
/// [CR 606.4]: https://yawgatog.com/resources/magic-rules/#R6064
#[must_use]
pub fn ability_action(source: ObjectId, row: &AbilityRow, x: Option<u16>) -> Action {
    let selection = Selection {
        objects: vec![source],
        zone: Zone::Battlefield,
    };
    match row {
        AbilityRow::Activated { text, .. } => Action::Marker {
            selection,
            text: text.clone(),
        },
        AbilityRow::Loyalty { cost, text } => {
            let change = match cost {
                LoyaltyCost::Plus(amount) => i32::from(*amount),
                LoyaltyCost::Minus(amount) => -i32::from(*amount),
                LoyaltyCost::MinusX => -i32::from(x.unwrap_or_default()),
                LoyaltyCost::Zero => 0,
            };
            Action::Batch {
                actions: vec![
                    Action::Counter {
                        selection: selection.clone(),
                        name: "loyalty".into(),
                        adjustment: Adjustment::Add(change),
                    },
                    Action::Marker {
                        selection,
                        text: text.clone(),
                    },
                ],
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Flow {
    Scry(u16),
    Surveil(u16),
    LookTop(u16),
}

impl Flow {
    #[must_use]
    pub fn new(kind: FlowKind, count: u16) -> Self {
        match kind {
            FlowKind::Scry => Self::Scry(count),
            FlowKind::Surveil => Self::Surveil(count),
            FlowKind::LookAtTop => Self::LookTop(count),
        }
    }

    #[must_use]
    pub fn kind(self) -> FlowKind {
        match self {
            Self::Scry(_) => FlowKind::Scry,
            Self::Surveil(_) => FlowKind::Surveil,
            Self::LookTop(_) => FlowKind::LookAtTop,
        }
    }

    #[must_use]
    pub fn count(self) -> u16 {
        match self {
            Self::Scry(count) | Self::Surveil(count) | Self::LookTop(count) => count,
        }
    }
}

fn count(word: &str) -> Option<u16> {
    const NUMBERS: [&str; 10] = [
        "one", "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    ];
    word.parse().ok().or_else(|| {
        NUMBERS
            .iter()
            .zip(1..)
            .find_map(|(&name, number)| (word == name).then_some(number))
    })
}

fn flow_at(words: &[&str]) -> Option<Flow> {
    match words {
        ["scry", number, ..] => count(number).map(Flow::Scry),
        ["surveil", number, ..] => count(number).map(Flow::Surveil),
        [
            "look",
            "at",
            "the",
            "top",
            number,
            "cards",
            "of",
            "your",
            "library",
            ..,
        ] => count(number).map(Flow::LookTop),
        _ => None,
    }
}

/// Finds literal count-bearing library instructions in Oracle text. Reminder
/// text and abilities quoted from other objects are skipped.
#[must_use]
pub fn flows(face: &Face) -> Vec<Flow> {
    let text = visible_text(face.oracle_text.as_deref().unwrap_or_default()).to_ascii_lowercase();
    let mut result = Vec::new();
    for phrase in
        text.split(|character: char| !character.is_alphanumeric() && !character.is_whitespace())
    {
        let words: Vec<_> = phrase.split_whitespace().collect();
        result.extend((0..words.len()).filter_map(|index| flow_at(&words[index..])));
    }
    result
}

/// The player and the ordered destinations chosen in the host's sorting tray.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlowChoice {
    pub seat: Seat,
    /// The top of the library, first card first.
    pub on_top: Vec<ObjectId>,
    /// The cards going to the bottom of the library or into the graveyard,
    /// in order from top to bottom. Empty for `LookTop`.
    pub elsewhere: Vec<ObjectId>,
}

/// Moves exactly the looked-at cards in the order the player chose. The
/// host supplies the currently looked-at top cards, which may be fewer than
/// requested when the library is short.
/// Scry puts cards “on the bottom of your library in any order” ([CR 701.22a]).
/// Surveil puts cards “into your graveyard” or back on top ([CR 701.25a]).
///
/// [CR 701.22a]: https://yawgatog.com/resources/magic-rules/#R70122a
/// [CR 701.25a]: https://yawgatog.com/resources/magic-rules/#R70125a
pub fn flow_result(flow: Flow, top: &[ObjectId], choice: &FlowChoice) -> Result<Action, Refusal> {
    if top.len() > usize::from(flow.count()) {
        return Err(Refusal::INVALID);
    }
    validate_flow(flow.kind(), top, &choice.on_top, &choice.elsewhere)?;
    Ok(Action::FlowResult {
        flow: flow.kind(),
        seat: choice.seat,
        on_top: choice.on_top.clone(),
        elsewhere: choice.elsewhere.clone(),
    })
}

pub(crate) fn flow_moves(
    flow: FlowKind,
    seat: Seat,
    on_top: &[ObjectId],
    elsewhere: &[ObjectId],
    library: &[ObjectId],
) -> Result<Vec<Action>, Refusal> {
    let count = on_top.len() + elsewhere.len();
    let packet = library.get(..count).ok_or(Refusal::Stale)?;
    validate_flow(flow, packet, on_top, elsewhere)?;
    let library = Zone::Library(seat);
    let move_cards = |objects: &[ObjectId], to, position| Action::Move {
        selection: Selection {
            objects: objects.to_vec(),
            zone: library,
        },
        to: Destination::Zone(to),
        position,
        face: None,
        tapped: None,
    };
    let mut actions = Vec::new();
    if !elsewhere.is_empty() {
        let (to, position) = match flow {
            FlowKind::Scry => (library, Position::Bottom),
            FlowKind::Surveil => (Zone::Graveyard(seat), Position::Top),
            FlowKind::LookAtTop => unreachable!("look-at-top destinations were validated"),
        };
        actions.push(move_cards(elsewhere, to, position));
    }
    if !on_top.is_empty() {
        actions.push(move_cards(on_top, library, Position::Top));
    }
    Ok(actions)
}

fn validate_flow(
    flow: FlowKind,
    packet: &[ObjectId],
    on_top: &[ObjectId],
    elsewhere: &[ObjectId],
) -> Result<(), Refusal> {
    let mut remaining: BTreeSet<_> = packet.iter().copied().collect();
    if packet.is_empty()
        || packet.len() != remaining.len()
        || on_top
            .iter()
            .chain(elsewhere)
            .any(|id| !remaining.remove(id))
        || !remaining.is_empty()
        || (flow == FlowKind::LookAtTop && !elsewhere.is_empty())
    {
        return Err(Refusal::INVALID);
    }
    Ok(())
}

/// Formats the public summary for a validated library flow.
#[must_use]
pub fn flow_line(flow: Flow, choice: &FlowChoice) -> String {
    flow_summary(flow.kind(), choice.on_top.len(), choice.elsewhere.len())
}

pub(crate) fn flow_summary(flow: FlowKind, top: usize, elsewhere: usize) -> String {
    let count = top + elsewhere;
    match flow {
        FlowKind::Scry => format!("Scried {count}: {top} on top, {elsewhere} on the bottom"),
        FlowKind::Surveil => {
            format!("Surveilled {count}: {top} on top, {elsewhere} in the graveyard")
        }
        FlowKind::LookAtTop => format!("Looked at {count}: {top} on top"),
    }
}
