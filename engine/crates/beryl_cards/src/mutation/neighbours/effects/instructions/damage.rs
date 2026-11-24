//! Neighbouring values of damage and prevention instructions.

use beryl_card::{Amount, Effect, Filter, Prevention, Recipients};

use super::{Kind, Neighbours, amounts, objects, players};

pub(super) fn dealing(to: Recipients, amount: Amount, add: &mut impl FnMut(Kind, String, Effect)) {
    if let Recipients::DividedTwo(spec) = to {
        add(
            Kind::DamageRecipients,
            "all damage to each target".to_owned(),
            Effect::DealDamage {
                to: Recipients::target(spec),
                amount,
            },
        );
        return;
    }
    for (name, to) in recipients(to) {
        add(
            Kind::DamageRecipients,
            name,
            Effect::DealDamage { to, amount },
        );
    }
    for (name, amount) in amounts(amount) {
        add(Kind::Amount, name, Effect::DealDamage { to, amount });
    }
}

pub(super) fn preventing(
    to: Recipients,
    prevention: Prevention,
    from: Option<&'static Filter>,
    add: &mut impl FnMut(Kind, String, Effect),
) {
    let prevent = |to, prevention, from| Effect::PreventDamage {
        to,
        prevention,
        from,
    };
    for (name, to) in recipients(to) {
        add(Kind::DamageRecipients, name, prevent(to, prevention, from));
    }
    let preventions = match prevention {
        Prevention::NextTime => vec![("the next 1".to_owned(), Prevention::Next(1))],
        Prevention::Next(n) => {
            let mut near = vec![
                ("the next time".to_owned(), Prevention::NextTime),
                (format!("the next {}", n + 1), Prevention::Next(n + 1)),
            ];
            if n > 1 {
                near.push((format!("the next {}", n - 1), Prevention::Next(n - 1)));
            }
            near
        }
    };
    for (name, prevention) in preventions {
        add(Kind::Prevention, name, prevent(to, prevention, from));
    }
    if from.is_some() {
        add(
            Kind::PreventionSource,
            "from any source".to_owned(),
            prevent(to, prevention, None),
        );
    }
}

/// The neighbours of who is dealt damage: other players or objects, and for both, only the
/// players or only the permanents of the two.
fn recipients(to: Recipients) -> Neighbours<Recipients> {
    match to {
        Recipients::DividedTwo(_) => Vec::new(),
        Recipients::Players(p) => players(p)
            .into_iter()
            .map(|(n, p)| (n, Recipients::Players(p)))
            .collect(),
        Recipients::Permanents(o) => objects(o)
            .into_iter()
            .map(|(n, o)| (n, Recipients::Permanents(o)))
            .collect(),
        Recipients::Both(p, o) => vec![
            ("only players".to_owned(), Recipients::Players(p)),
            ("only permanents".to_owned(), Recipients::Permanents(o)),
        ],
    }
}
