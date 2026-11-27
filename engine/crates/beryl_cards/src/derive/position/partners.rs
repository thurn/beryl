//! Choose starting permanents whose printed characteristics exercise a definition's filters.

use beryl_card::words::Words;
use beryl_card::{CardDef, Face, FaceIx, Filter, Node, Players, Seat, Subtype, Types, Zone};
use beryl_scenario::{P0, P1, Permanent};

use crate::{CARDS, props};

use super::is_permanent;

pub(super) fn add(def: &CardDef, battlefield: &mut [Vec<Permanent>; 2], early: bool) -> bool {
    let mut filters = Vec::new();
    let mut children = 0;
    def.filters(&mut |filter| {
        if children > 0 {
            children -= 1;
            return;
        }
        // Both walks visit every part after its parent. Keep the whole filter
        // so a card outside a bound can still meet the surrounding constraints.
        filter.walk(&mut |node| children += usize::from(matches!(node, Node::Filter(_))));
        children -= 1;
        if *filter != Filter::Any && !filters.contains(filter) {
            filters.push(*filter);
        }
    });

    let mut changed = false;
    for filter in filters {
        for seat in sides(filter) {
            let used = |name: &str| {
                battlefield
                    .iter()
                    .any(|board| board.iter().any(|p| p.card == name))
            };
            let added = pair(
                def,
                &battlefield[seat.index()],
                filter,
                seat,
                seat == P0 || !early,
                &used,
            );
            changed |= added.iter().any(Option::is_some);
            battlefield[seat.index()].extend(added.into_iter().flatten());
        }
    }
    changed
}

/// Reuse existing permanents and prefer a contrast of the same type. A creature
/// above a mana value bound is more informative than a land outside the filter.
fn pair(
    def: &CardDef,
    board: &[Permanent],
    filter: Filter,
    seat: Seat,
    can_tap: bool,
    used: &impl Fn(&str) -> bool,
) -> [Option<Permanent>; 2] {
    let known = || {
        board.iter().copied().filter_map(|permanent| {
            let card = CARDS.get(permanent.card)?;
            (card.name != def.name).then_some((permanent, card.face(permanent.face)))
        })
    };
    let applies = |(permanent, face): (Permanent, &Face)| {
        matches(filter, seat, &|part| {
            printed(part, permanent, face, Zone::Battlefield)
        })
    };
    let positive = |&card: &(Permanent, &Face)| applies(card) == Some(true);
    let negative = |&card: &(Permanent, &Face)| applies(card) == Some(false);
    let existing = known().find(positive);
    let matching = existing.or_else(|| pick(def, used, can_tap, positive));
    let Some((permanent, face)) = matching else {
        // A pure controller predicate wants an example on the other player's board.
        let contrast = (matches(filter, seat, &|_| None) == Some(false)
            && !known().any(|card| negative(&card)))
        .then(|| pick(def, used, can_tap, negative))
        .flatten()
        .map(|card| card.0);
        return [None, contrast];
    };

    let same_type = |card: &(Permanent, &Face)| card.1.types == face.types && negative(card);
    let contrasting = if known().any(|card| same_type(&card)) {
        None
    } else {
        let used = |name: &str| name == permanent.card || used(name);
        pick(def, &used, can_tap, same_type).or_else(|| {
            if known().any(|card| negative(&card)) {
                return None;
            }
            pick(def, &used, can_tap, negative)
        })
    };
    [
        existing.is_none().then_some(permanent),
        contrasting.map(|card| card.0),
    ]
}

/// An unqualified filter can see both players. A filter for one side gets its pair there,
/// and a controller-only filter also needs a matching object on the other side.
fn sides(filter: Filter) -> impl Iterator<Item = Seat> {
    let ownership = Seat::BOTH.map(|seat| matches(filter, seat, &|_| None));
    let control_only = ownership.iter().all(Option::is_some);
    Seat::BOTH
        .into_iter()
        .filter(move |seat| control_only || ownership[seat.index()] != Some(false))
}

fn matches(filter: Filter, seat: Seat, printed: &impl Fn(Filter) -> Option<bool>) -> Option<bool> {
    match filter {
        Filter::All(parts) => combine(parts, seat, printed, false),
        Filter::AnyOf(parts) => combine(parts, seat, printed, true),
        Filter::Not(part) => matches(*part, seat, printed).map(|value| !value),
        Filter::Whose(players) | Filter::Owned(players) => match *players {
            Players::You => Some(seat == P0),
            Players::EachOpponent => Some(seat == P1),
            Players::EachPlayer => Some(true),
            Players::ThatPlayer
            | Players::SubgameNonwinners
            | Players::Enchanted
            | Players::EachOther(_)
            | Players::ControllerOf(_)
            | Players::OwnerOf(_)
            | Players::Target(_) => None,
        },
        part => printed(part),
    }
}

fn combine(
    parts: &[Filter],
    seat: Seat,
    printed: &impl Fn(Filter) -> Option<bool>,
    decisive: bool,
) -> Option<bool> {
    let mut known = true;
    for &part in parts {
        match matches(part, seat, printed) {
            Some(value) if value == decisive => return Some(decisive),
            Some(_) => {}
            None => known = false,
        }
    }
    known.then_some(!decisive)
}

/// Starting objects have no text changes or X in a resolving ability. The board
/// fixtures do not start with a stolen card or an object already attached.
pub(in crate::derive) fn printed(
    filter: Filter,
    permanent: Permanent,
    face: &Face,
    zone: Zone,
) -> Option<bool> {
    Some(match filter {
        Filter::Any => true,
        Filter::Types(types) => face.types.intersects(types),
        Filter::Supertypes(types) => face.supertypes.intersects(types),
        Filter::SubtypeWord(word) => {
            face.subtypes
                .read(&Words::NONE)
                .contains(word.read(&Words::NONE))
                || (face.keywords.contains(beryl_card::Keywords::CHANGELING)
                    && word.read(&Words::NONE).card_type() == Types::CREATURE)
        }
        Filter::ColorWords(words) => face.colors.intersects(words.read(&Words::NONE)),
        Filter::Tapped => permanent.tapped,
        Filter::AttackedThisTurn | Filter::WithCounter(_) => false,
        Filter::Token => permanent.token,
        Filter::Named(name) => face.name == name,
        Filter::ManaValueAtMost(value) => face.cost.mana_value() <= value,
        Filter::PowerAtMost(power) => face.power.is_some_and(|p| p <= power),
        Filter::InZone(expected) => zone == expected,
        Filter::ManaValueAtMostX
        | Filter::ManaValueAtMostPower
        | Filter::Whose(_)
        | Filter::Owned(_)
        | Filter::Not(_)
        | Filter::All(_)
        | Filter::AnyOf(_) => return None,
    })
}

fn pick(
    source: &CardDef,
    used: &impl Fn(&str) -> bool,
    can_tap: bool,
    matches: impl Fn(&(Permanent, &Face)) -> bool,
) -> Option<(Permanent, &'static Face)> {
    let eligible = |card: &'static CardDef| {
        let face = card.face(FaceIx::FRONT);
        (card.name != source.name
            && !used(card.name)
            && is_permanent(face)
            && !face.subtypes.read(&Words::NONE).contains(Subtype::Aura)
            && !face.types.intersects(Types::PLANESWALKER))
        .then(|| {
            let permanent = Permanent::new(card.name);
            [Some(permanent), can_tap.then_some(permanent.tapped())]
                .into_iter()
                .flatten()
                .find(|&permanent| matches(&(permanent, face)))
                .map(|permanent| (permanent, face))
        })
        .flatten()
    };
    // A prop can stand for any characteristic its contract fixes. A printed-only
    // characteristic, such as a supertype, falls back to the definition below.
    props::ALL
        .iter()
        .filter_map(|prop| CARDS.get(prop.card))
        .find_map(eligible)
        .or_else(|| {
            CARDS
                .cards()
                .iter()
                .chain(CARDS.from_store())
                .copied()
                .filter_map(eligible)
                .min_by_key(|(permanent, face)| {
                    (face.abilities.len(), face.cost.mana_value(), permanent.card)
                })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chooses_a_tapped_creature_within_the_bound_and_an_untapped_contrast() {
        let def = CARDS.get(crate::names::PORTABLE_HOLE).unwrap();
        let filter = Filter::All(&[
            Filter::Types(Types::CREATURE),
            Filter::ManaValueAtMost(2),
            Filter::Tapped,
            Filter::Whose(&Players::EachOpponent),
        ]);
        assert_eq!(sides(filter).collect::<Vec<_>>(), [P1]);
        let [matching, contrasting] = pair(def, &[], filter, P1, true, &|_| false);
        let matching = matching.unwrap();
        assert_eq!(matching.card, props::VANILLA.card);
        assert!(matching.tapped);
        assert_eq!(contrasting.unwrap().card, props::FLIER.card);
        assert!(!contrasting.unwrap().tapped);
    }

    #[test]
    fn printed_battlefield_filters_have_a_partner_in_the_pool() {
        use crate::derive::witness::{ATTACHED_AURA, requires_aura};
        use beryl_card::{Objects, TargetKind};

        let mut missing = Vec::new();
        for &def in CARDS.cards() {
            let mut check = |filter: Filter, can_cast_aura: bool| {
                let mut only_printed = !filter.needs_a_resolution();
                filter.walk(&mut |node| {
                    if matches!(
                        node,
                        Node::Filter(
                            Filter::Token | Filter::AttackedThisTurn | Filter::WithCounter(_)
                        )
                    ) {
                        only_printed = false;
                    }
                });
                if !only_printed {
                    return;
                }
                for seat in sides(filter) {
                    if matches(filter, seat, &|_| None) == Some(false) {
                        continue;
                    }
                    let partner = pick(def, &|_| false, true, |&(permanent, face)| {
                        matches(filter, seat, &|part| {
                            printed(part, permanent, face, Zone::Battlefield)
                        }) == Some(true)
                    });
                    // A witness casts its partner before targeting it, so an Aura never
                    // starts on the board unattached.
                    let aura_partner = can_cast_aura
                        && requires_aura(filter)
                        && seat == P0
                        && ATTACHED_AURA.hand.iter().any(|&name| {
                            let face = CARDS.get(name).expect("an Aura witness card").front();
                            matches(filter, seat, &|part| {
                                printed(part, Permanent::new(name), face, Zone::Battlefield)
                            }) == Some(true)
                        });
                    if partner.is_none() && !aura_partner {
                        missing.push(format!(
                            "{} needs a board prop for {filter:?} on seat {seat}",
                            def.name
                        ));
                    }
                }
            };
            def.walk(&mut |node| {
                if let Node::Objects(
                    Objects::All {
                        zone: Zone::Battlefield,
                        filter,
                    }
                    | Objects::InZoneOf {
                        zone: Zone::Battlefield,
                        filter,
                        ..
                    },
                ) = node
                {
                    check(*filter, false);
                }
            });
            for target in def.faces.iter().flat_map(|face| {
                face.abilities
                    .iter()
                    .chain(face.grants)
                    .flat_map(beryl_card::Ability::targets)
            }) {
                if matches!(
                    target.kind,
                    TargetKind::Permanent
                        | TargetKind::PermanentOrPlayer
                        | TargetKind::SpellOrPermanent
                ) {
                    check(target.filter, target.kind == TargetKind::Permanent);
                }
            }
        }
        assert!(missing.is_empty(), "{}", missing.join("\n"));
    }

    #[test]
    fn an_unqualified_creature_type_gets_a_partner_on_both_sides() {
        let def = CARDS.get(crate::names::LORD_OF_ATLANTIS).unwrap();
        let mut battlefield = [
            vec![Permanent::new(props::VANILLA.card)],
            vec![Permanent::new(props::FLIER.card)],
        ];
        assert!(add(def, &mut battlefield, false));
        let [mine, theirs] = battlefield;
        assert_eq!(mine[1].card, props::THIEF.card);
        assert_ne!(theirs[1].card, mine[1].card);
        let merfolk = Filter::Subtype(Subtype::Merfolk);
        for permanent in [mine[1], theirs[1]] {
            let face = CARDS.get(permanent.card).unwrap().front();
            assert_eq!(
                printed(merfolk, permanent, face, Zone::Battlefield),
                Some(true)
            );
        }
    }
}
