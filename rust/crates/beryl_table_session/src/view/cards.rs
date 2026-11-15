use beryl_card_store::{
    STORE,
    facts::{self, Row3},
};
use beryl_session::card_records::{record, token_printing_key};
use beryl_session::protocol::{
    CardType, Counter, CounterView, FaceName, Facing, Keyword, MarkerPreview, NamedCounter,
    ObjectId as Slot, ObjectView, PowerToughness, RowName, Side, StackItem, ZoneName,
};
use beryl_table::helpers::{counter, printed_face};
use beryl_table::state::{AbilityMarker, Card, Face, Object, ObjectId, Row};

use super::{Slots, Viewer, zone};

pub(crate) fn object(card: &Object, slot: Slot, slots: &Slots, viewer: Viewer) -> ObjectView {
    let (zone, holder) = zone(card.zone);
    let owner = viewer.side(card.owner);
    let controller = viewer.side(card.controller);
    let printed = match &card.card {
        Card::Named(name) => record(name, card.token),
        Card::Token { name, scryfall_id } => STORE.token_printing(name, scryfall_id),
        _ => None,
    };
    let name = card.card.name();
    let named_face = printed
        .as_ref()
        .and_then(|record| printed_face(record, face_for_preview(card.face)));
    let mut object = if let Some(name) = name {
        let display_name = named_face.map_or(name, |face| &face.name);
        ObjectView::card(slot, display_name, zone, controller, owner)
    } else {
        ObjectView::hidden(slot, zone, controller, owner)
    };

    object.zone_side = holder
        .filter(|&holder| holder != card.owner)
        .map(|holder| viewer.side(holder));
    object.face = face(card.face);
    if card.face == Face::Down {
        object.facing = Facing::Down;
    }
    object.token = card.token;
    match &card.card {
        Card::Custom { types, colors, .. } => {
            object.type_line = Some(types.clone());
            object.custom_colors = Some(colors.clone().into_boxed_slice());
        }
        Card::Token { name, scryfall_id } => {
            object.token_printing = Some(token_printing_key(name, scryfall_id).into_boxed_str());
        }
        Card::Named(_) | Card::Hidden => {}
    }
    object.tapped = card.tapped;
    object.damage = card.damage;
    object.note.clone_from(&card.note);
    object.attached_to = card.attached_to.and_then(|id| slots.get(id));
    object.doesnt_untap = card.doesnt_untap;
    for (name, count) in &card.counters {
        if let Some(kind) = Counter::named(name) {
            object.counters.push(CounterView {
                kind,
                count: *count,
            });
        } else {
            object.named_counters.push(NamedCounter {
                name: name.clone(),
                count: *count,
            });
        }
    }
    if zone == ZoneName::Battlefield {
        battlefield(&mut object, card, printed.as_ref());
    }
    object
}

fn battlefield(object: &mut ObjectView, card: &Object, record: Option<&beryl_card_store::Record>) {
    let mut printed_stats = None;
    let row = if card.face == Face::Down {
        Row3::Creatures
    } else if let Card::Custom { types, stats, .. } = &card.card {
        object.types = card_types(types);
        printed_stats = *stats;
        if types.trim().is_empty() {
            Row3::Creatures
        } else {
            row_of(types)
        }
    } else if let Some(record) = record
        && let Some(printed) = printed_face(record, card.face)
    {
        object.types = card_types(&printed.type_line);
        printed_stats = facts::printed_stats(printed);
        let keywords = facts::printed_keywords(record, printed);
        object.keywords = keywords.into_iter().filter_map(keyword).collect();
        if card.face == Face::Front {
            facts::row(record)
        } else {
            row_of(&printed.type_line)
        }
    } else {
        Row3::Others
    };
    object.pinned = Some(match card.row {
        Row::Creatures => RowName::Creatures,
        Row::Others => RowName::Others,
        Row::Lands => RowName::Lands,
        Row::Auto => match row {
            Row3::Creatures => RowName::Creatures,
            Row3::Others => RowName::Others,
            Row3::Lands => RowName::Lands,
        },
    });
    object.pt = facts::shown_stats(
        printed_stats,
        card.stats,
        counter(card, "+1/+1"),
        counter(card, "-1/-1"),
    )
    .map(|(power, toughness)| PowerToughness { power, toughness });
}

fn row_of(type_line: &str) -> Row3 {
    if facts::is_creature_type_line(type_line) {
        Row3::Creatures
    } else if facts::is_land_type_line(type_line) {
        Row3::Lands
    } else {
        Row3::Others
    }
}

fn card_types(type_line: &str) -> Vec<CardType> {
    let types = type_line.split('—').next().unwrap_or_default();
    types
        .split_whitespace()
        .filter_map(|word| {
            Some(match word {
                "Artifact" => CardType::Artifact,
                "Creature" => CardType::Creature,
                "Enchantment" => CardType::Enchantment,
                "Instant" => CardType::Instant,
                "Land" => CardType::Land,
                "Planeswalker" => CardType::Planeswalker,
                "Sorcery" => CardType::Sorcery,
                _ => return None,
            })
        })
        .collect()
}

fn keyword(name: &str) -> Option<Keyword> {
    Some(match name.to_ascii_lowercase().as_str() {
        "first strike" => Keyword::FirstStrike,
        "flying" => Keyword::Flying,
        "reach" => Keyword::Reach,
        "vigilance" => Keyword::Vigilance,
        _ => return None,
    })
}

fn face(face: Face) -> FaceName {
    match face {
        Face::Front => FaceName::Front,
        Face::Back => FaceName::Back,
        Face::Down => FaceName::Down,
    }
}

fn face_for_preview(face: Face) -> Face {
    match face {
        Face::Down => Face::Front,
        other => other,
    }
}

pub(crate) fn marker(
    id: ObjectId,
    marker: &AbilityMarker,
    slots: &Slots,
    viewer: Viewer,
) -> StackItem {
    let preview = marker.display_source.as_ref().and_then(|source| {
        let name = source.card.name()?.to_owned();
        Some(MarkerPreview {
            name,
            face: face(source.face),
            text: marker.text.clone(),
        })
    });
    StackItem::ability(
        id.0,
        slots.get(marker.source),
        preview,
        marker
            .controller
            .map_or(Side::You, |seat| viewer.side(seat)),
    )
}
