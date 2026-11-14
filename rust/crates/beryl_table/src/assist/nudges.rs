use std::collections::BTreeMap;

use beryl_card_store::facts;

use crate::action::{Destination, Position, Selection};
use crate::assist::{Assist, enabled};
use crate::helpers::{card_record, counter, printed_face};
use crate::state::{Card, Face, Object, ObjectId, Zone};
use crate::{Action, Seat, Table};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ObjectOrSeat {
    Object(ObjectId),
    Seat(Seat),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Label {
    pub on: ObjectOrSeat,
    pub text: String,
    pub action: Action,
    pub show_to: Vec<Seat>,
}

impl Label {
    fn graveyard(card: &Object, text: &str, show_to: &[Seat]) -> Self {
        Self {
            on: ObjectOrSeat::Object(card.id),
            text: text.into(),
            action: Action::Move {
                selection: Selection {
                    objects: vec![card.id],
                    zone: Zone::Battlefield,
                },
                to: Destination::Zone(Zone::Graveyard(card.owner)),
                position: Position::Top,
                face: None,
                tapped: None,
            },
            show_to: show_to.to_vec(),
        }
    }
}

/// Suggests manual fixes using the public face and counters of battlefield
/// cards. A concession is offered only to the seat that would concede.
///
/// [CR 704.5f] covers creatures with “toughness 0 or less,” and [CR 704.5g]
/// covers damage “greater than or equal to its toughness.” [CR 704.5i]
/// covers a planeswalker with “loyalty 0.” [CR 704.5j] covers legendary
/// permanents “with the same name” controlled by “the same player.”
///
/// [CR 704.5f]: https://yawgatog.com/resources/magic-rules/#R7045f
/// [CR 704.5g]: https://yawgatog.com/resources/magic-rules/#R7045g
/// [CR 704.5i]: https://yawgatog.com/resources/magic-rules/#R7045i
/// [CR 704.5j]: https://yawgatog.com/resources/magic-rules/#R7045j
#[must_use]
pub fn labels(table: &Table) -> Vec<Label> {
    let seats: Vec<_> = table
        .seats()
        .filter(|&seat| enabled(table, seat, Assist::StateNudges))
        .collect();
    if seats.is_empty() {
        return Vec::new();
    }

    let mut labels = Vec::new();
    let mut legends: BTreeMap<_, Vec<_>> = BTreeMap::new();
    for id in &table.battlefield {
        let card = &table.objects[id];
        if card.face == Face::Down {
            continue;
        }

        let record;
        let (name, types, printed) = match &card.card {
            Card::Custom {
                name, types, stats, ..
            } => (name, types, *stats),
            Card::Named(_) => {
                record = card_record(&card.card);
                let Some(face) = record
                    .as_ref()
                    .and_then(|record| printed_face(record, card.face))
                else {
                    continue;
                };
                (&face.name, &face.type_line, facts::printed_stats(face))
            }
            Card::Hidden => continue,
        };
        let shown = facts::shown_stats(
            printed,
            card.stats,
            counter(card, "+1/+1"),
            counter(card, "-1/-1"),
        );
        if facts::is_creature_type_line(types)
            && shown.is_some_and(|(_, toughness)| toughness <= 0 || card.damage >= toughness)
        {
            labels.push(Label::graveyard(card, "Move to graveyard", &seats));
        }
        if facts::is_planeswalker_type_line(types) && counter(card, "loyalty") <= 0 {
            labels.push(Label::graveyard(card, "No loyalty", &seats));
        }
        if facts::is_legendary_type_line(types) && facts::is_permanent_type_line(types) {
            legends
                .entry((card.controller, name.to_lowercase()))
                .or_default()
                .push(card);
        }
    }
    for cards in legends.values().filter(|cards| cards.len() > 1) {
        for card in cards {
            labels.push(Label::graveyard(card, "Legend rule", &seats));
        }
    }
    if table.game_match.in_game {
        for seat in seats {
            if table.players[usize::from(seat.0)].life <= 0 {
                labels.push(Label {
                    on: ObjectOrSeat::Seat(seat),
                    text: "Concede".into(),
                    action: Action::Concede {},
                    show_to: vec![seat],
                });
            }
        }
    }
    labels
}
