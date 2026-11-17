//! Neighbouring values shared by definitions and effects.
use super::{Neighbours, leak};
use beryl_card::{Amount, Filter, Objects, Players, Zone};

/// The other players a card could have named: "you", "each opponent" and "each player" for
/// one another, and an object's owner for its controller and the reverse.
pub(super) fn players(player: Players) -> Neighbours<Players> {
    const SIMPLE: [Players; 3] = [Players::You, Players::EachOpponent, Players::EachPlayer];
    match player {
        Players::You | Players::EachOpponent | Players::EachPlayer => SIMPLE
            .into_iter()
            .filter(|&p| p != player)
            .map(|p| (format!("Players::{p:?}"), p))
            .collect(),
        Players::ControllerOf(o) => vec![("Players::OwnerOf".to_owned(), Players::OwnerOf(o))],
        Players::OwnerOf(o) => vec![("Players::ControllerOf".to_owned(), Players::ControllerOf(o))],
        Players::Target(_) | Players::ThatPlayer | Players::EachOther(_) => Vec::new(),
    }
}

/// The other player a card could have named for one who makes a choice: "you" for "an
/// opponent" and the reverse. "Each player" is no value of such a field, which asks one
/// player; the engine asks the first of them in turn order (`Run::one` in `beryl_rules`), and
/// a card in which each player chooses says so with `Effect::ForEachPlayer`.
pub(super) fn chooser(player: Players) -> Neighbours<Players> {
    match player {
        Players::You => vec![("Players::EachOpponent".to_owned(), Players::EachOpponent)],
        Players::EachOpponent => vec![("Players::You".to_owned(), Players::You)],
        _ => players(player),
    }
}

/// An amount one higher and one lower.
pub(super) fn amounts(amount: Amount) -> Neighbours<Amount> {
    match amount {
        Amount::Fixed(n) => {
            let mut out = vec![(format!("Amount::Fixed({})", n + 1), Amount::Fixed(n + 1))];
            if n > 0 {
                out.push((format!("Amount::Fixed({})", n - 1), Amount::Fixed(n - 1)));
            }
            out
        }
        Amount::Times(n, of) => {
            let mut out = vec![(format!("times {}", n + 1), Amount::Times(n + 1, of))];
            if n > 1 {
                out.push((format!("times {}", n - 1), Amount::Times(n - 1, of)));
            }
            out
        }
        Amount::Plus(n, of) => {
            let mut out = vec![(format!("plus {}", n + 1), Amount::Plus(n + 1, of))];
            if n > 0 {
                out.push((format!("plus {}", n - 1), Amount::Plus(n - 1, of)));
            }
            out
        }
        _ => Vec::new(),
    }
}

/// The objects a card could have named instead: the same zone with any filter, and the
/// cards of the other players named as [`players`] names them.
pub(super) fn objects(which: Objects) -> Neighbours<Objects> {
    let any = "filter Filter::Any".to_owned();
    match which {
        Objects::All { zone, filter } if filter != Filter::Any => {
            vec![(
                any,
                Objects::All {
                    zone,
                    filter: Filter::Any,
                },
            )]
        }
        Objects::InZoneOf {
            zone,
            players: who,
            filter,
        } => {
            let mut out: Neighbours<Objects> = players(*who)
                .into_iter()
                .map(|(name, p)| {
                    (
                        name,
                        Objects::InZoneOf {
                            zone,
                            players: leak(p),
                            filter,
                        },
                    )
                })
                .collect();
            if filter != Filter::Any {
                out.push((
                    any,
                    Objects::InZoneOf {
                        zone,
                        players: who,
                        filter: Filter::Any,
                    },
                ));
            }
            out
        }
        Objects::CardsIn {
            players: who,
            zones,
        } => players(*who)
            .into_iter()
            .map(|(name, p)| {
                (
                    name,
                    Objects::CardsIn {
                        players: leak(p),
                        zones,
                    },
                )
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// The other zones a card could have put a card into.
pub(super) fn zones(zone: Zone) -> Neighbours<Zone> {
    [Zone::Hand, Zone::Library, Zone::Graveyard, Zone::Exile]
        .into_iter()
        .filter(|&z| z != zone)
        .map(|z| (format!("Zone::{z:?}"), z))
        .collect()
}
