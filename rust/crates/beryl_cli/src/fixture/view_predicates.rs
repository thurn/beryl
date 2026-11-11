//! The questions the stopping conditions ask of a [`PlayerView`].

use super::condition::Mark;
use beryl_session::protocol::{
    Candidate, CandidateView, CardType, ObjectView, Side, StackItem, TargetRef, ZoneName,
};
use beryl_session::{PlayerView, Status};

/// Whether the human may activate more than one ability of a permanent named `card`.
pub(super) fn abilities_of(view: &PlayerView, card: &str) -> bool {
    view.battlefield
        .iter()
        .filter(|&&slot| {
            view.objects
                .iter()
                .any(|o| o.slot == slot && o.name.as_deref() == Some(card))
        })
        .any(|&slot| {
            candidates(view)
                .filter(|c| matches!(c.option, Candidate::Activate { slot: s, .. } if s == slot))
                .count()
                > 1
        })
}

/// Whether the open decision offers two objects of one name.
pub(super) fn namesake_candidates(view: &PlayerView) -> bool {
    let names: Vec<_> = candidates(view)
        .filter_map(|c| match c.option {
            Candidate::Object { slot } => view
                .objects
                .iter()
                .find(|o| o.slot == slot)
                .and_then(|o| o.name.as_deref()),
            _ => None,
        })
        .collect();
    names
        .iter()
        .enumerate()
        .any(|(i, n)| names[i + 1..].contains(n))
}

/// The open decision's candidates, if any.
/// Whether the human may play a card named `card` off the battlefield more than one way.
pub(super) fn ways_to_play(view: &PlayerView, card: &str) -> bool {
    let off_battlefield = |slot: u16| {
        view.objects.iter().any(|o| {
            o.slot == slot && o.zone != ZoneName::Battlefield && o.name.as_deref() == Some(card)
        })
    };
    candidates(view)
        .filter(|c| match c.option {
            Candidate::Cast { slot, .. }
            | Candidate::PlayLand { slot }
            | Candidate::Activate { slot, .. } => off_battlefield(slot),
            _ => false,
        })
        .count()
        > 1
}

pub(super) fn candidates(view: &PlayerView) -> impl Iterator<Item = &CandidateView> {
    match &view.status {
        Status::Deciding { decision } => decision.candidates.as_slice(),
        _ => &[],
    }
    .iter()
}

/// Whether the human may play or cast a card from their hand of which they hold another just
/// like it.
pub(super) fn playable_pair(view: &PlayerView) -> bool {
    let Status::Deciding { decision } = &view.status else {
        return false;
    };
    let in_hand = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot && o.zone == ZoneName::Hand)
    };
    decision.candidates.iter().any(|c| {
        let (Candidate::Cast { slot, .. } | Candidate::PlayLand { slot }) = c.option else {
            return false;
        };
        in_hand(slot).is_some_and(|card| {
            view.objects.iter().any(|o| {
                o.slot != slot
                    && o.zone == ZoneName::Hand
                    && o.owner == Side::You
                    && o.name.is_some()
                    && o.name == card.name
            })
        })
    })
}

/// Whether a card in exile links, by `host`, to a permanent on the battlefield.
pub(super) fn exiled_by_permanent(
    view: &PlayerView,
    host: impl Fn(&ObjectView) -> Option<u16>,
) -> bool {
    view.objects.iter().any(|o| {
        o.zone == ZoneName::Exile
            && host(o).is_some_and(|host| {
                view.objects
                    .iter()
                    .any(|h| h.slot == host && h.zone == ZoneName::Battlefield)
            })
    })
}

/// Whether each player has an emblem (CR 114.1).
pub(super) fn emblems_a_side(view: &PlayerView) -> bool {
    !view.players.you.emblems.is_empty() && !view.players.opponent.emblems.is_empty()
}

/// Whether the human has unspent mana (CR 106.4).
pub(super) fn mana_floating(view: &PlayerView) -> bool {
    let pool = view.players.you.mana;
    [
        pool.white,
        pool.blue,
        pool.black,
        pool.red,
        pool.green,
        pool.colorless,
    ]
    .iter()
    .any(|&n| n > 0)
}

/// Whether a spell or ability on the stack was announced with X above 0.
pub(super) fn x_on_stack(view: &PlayerView) -> bool {
    view.stack.iter().any(|item| {
        let (StackItem::Spell { x, .. } | StackItem::Ability { x, .. }) = item;
        *x > 0
    })
}

/// Whether a permanent one player controls is attached to a permanent the other controls.
pub(super) fn attached_across(view: &PlayerView) -> bool {
    view.objects.iter().any(|o| {
        o.zone == ZoneName::Battlefield
            && o.attached_to.is_some_and(|host| {
                view.objects.iter().any(|h| {
                    h.slot == host
                        && h.zone == ZoneName::Battlefield
                        && h.controller != o.controller
                })
            })
    })
}

/// Whether a permanent bears `mark` and no other while another of the same name and types,
/// and a token if it is one, bears none.
pub(super) fn marked_beside_twin(view: &PlayerView, mark: Mark) -> bool {
    let field = || {
        view.objects
            .iter()
            .filter(|o| o.zone == ZoneName::Battlefield && o.name.is_some())
    };
    field().any(|o| {
        Mark::only(Some(mark), o)
            && field().any(|t| {
                t.slot != o.slot
                    && t.name == o.name
                    && t.token == o.token
                    && t.types == o.types
                    && Mark::only(None, t)
            })
    })
}

/// Every pair of distinct tokens of one name that one player controls on the battlefield.
pub(super) fn token_pairs(view: &PlayerView) -> impl Iterator<Item = (&ObjectView, &ObjectView)> {
    let tokens = || {
        view.objects
            .iter()
            .filter(|o| o.zone == ZoneName::Battlefield && o.token && o.name.is_some())
    };
    tokens().flat_map(move |a| {
        tokens()
            .filter(move |b| a.slot < b.slot && a.name == b.name && a.controller == b.controller)
            .map(move |b| (a, b))
    })
}

/// Whether anything on the battlefield is attached to `o`.
pub(super) fn bears_attachment(view: &PlayerView, o: &ObjectView) -> bool {
    view.objects
        .iter()
        .any(|a| a.zone == ZoneName::Battlefield && a.attached_to == Some(o.slot))
}

/// Whether two tokens are alike in all the table shows and bear nothing: no counter, damage,
/// combat or attachment.
pub(super) fn plain_twins(view: &PlayerView, a: &ObjectView, b: &ObjectView) -> bool {
    let plain = |o: &ObjectView| {
        o.counters.is_empty()
            && o.damage == 0
            && o.combat.is_none()
            && o.attached_to.is_none()
            && !bears_attachment(view, o)
    };
    plain(a)
        && plain(b)
        && a.tapped == b.tapped
        && a.summoning_sick == b.summoning_sick
        && a.types == b.types
        && a.pt == b.pt
}

/// Whether a token differs from another of its name its controller controls by its types or
/// an attachment.
pub(super) fn changed_token(view: &PlayerView) -> bool {
    token_pairs(view).any(|(a, b)| {
        a.types != b.types
            || [a, b]
                .iter()
                .any(|o| o.attached_to.is_some() || bears_attachment(view, o))
    })
}

/// Whether a token stands beside a card of its name under one controller.
pub(super) fn token_beside_card(view: &PlayerView) -> bool {
    let field = || {
        view.objects
            .iter()
            .filter(|o| o.zone == ZoneName::Battlefield)
    };
    field().any(|t| {
        t.token
            && t.name.is_some()
            && field().any(|c| !c.token && c.name == t.name && c.controller == t.controller)
    })
}

/// Whether each player controls at least `n` nonland permanents.
pub(super) fn crowded(view: &PlayerView, n: usize) -> bool {
    [Side::You, Side::Opponent].iter().all(|side| {
        view.objects
            .iter()
            .filter(|o| {
                o.zone == ZoneName::Battlefield
                    && o.controller == *side
                    && !o.types.contains(&CardType::Land)
            })
            .count()
            >= n
    })
}

/// Whether a permanent is controlled by the player who doesn't own it.
pub(super) fn stolen(view: &PlayerView) -> bool {
    view.objects
        .iter()
        .any(|o| o.zone == ZoneName::Battlefield && o.controller != o.owner)
}

/// Whether the human may cast a card from `zone`.
pub(super) fn castable_from(view: &PlayerView, zone: ZoneName) -> bool {
    let Status::Deciding { decision } = &view.status else {
        return false;
    };
    decision.candidates.iter().any(|c| {
        matches!(c.option, Candidate::Cast { slot, .. } if view
            .objects
            .iter()
            .any(|o| o.slot == slot && o.zone == zone))
    })
}

/// Whether the human knows a card in the opponent's hand.
pub(super) fn knows_opponents_hand(view: &PlayerView) -> bool {
    view.objects
        .iter()
        .any(|o| o.zone == ZoneName::Hand && o.owner == Side::Opponent && o.name.is_some())
}

/// Whether the top of the stack is an ability of a permanent that targets another permanent
/// its controller controls.
pub(super) fn targets_beside_source(view: &PlayerView) -> bool {
    let Some(StackItem::Ability {
        source: Some(source),
        targets,
        ..
    }) = view.stack.last()
    else {
        return false;
    };
    let permanent = |slot: u16| {
        view.objects
            .iter()
            .find(|o| o.slot == slot && o.zone == ZoneName::Battlefield)
    };
    permanent(*source).is_some_and(|s| {
        targets.iter().any(|t| {
            matches!(t, TargetRef::Object { slot } if *slot != *source
                && permanent(*slot).is_some_and(|o| o.controller == s.controller))
        })
    })
}

/// The permanents named `card` the human controls.
pub(super) fn mine<'a>(
    view: &'a PlayerView,
    card: &'a str,
) -> impl Iterator<Item = &'a ObjectView> {
    controlled(view, card, Side::You)
}

pub(super) fn theirs<'a>(
    view: &'a PlayerView,
    card: &'a str,
) -> impl Iterator<Item = &'a ObjectView> {
    controlled(view, card, Side::Opponent)
}

pub(super) fn controlled<'a>(
    view: &'a PlayerView,
    card: &'a str,
    side: Side,
) -> impl Iterator<Item = &'a ObjectView> {
    view.objects.iter().filter(move |o| {
        o.zone == ZoneName::Battlefield && o.controller == side && o.name.as_deref() == Some(card)
    })
}
