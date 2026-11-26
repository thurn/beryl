//! Neighbouring values of the instructions that change objects for a duration.

use beryl_card::{Duration, Effect};

use super::super::super::continuous::modify_neighbours;
use super::super::super::operands::objects;
use super::Kind;

/// The objects an instruction modifies, and its modifications and their duration, each changed
/// in turn.
pub(super) fn modify(effect: &Effect, add: &mut impl FnMut(Kind, String, Effect)) {
    let Effect::Modify {
        objects: which,
        modifications,
        duration,
    } = *effect
    else {
        return;
    };
    for (name, which) in objects(which) {
        add(
            Kind::EffectObjects,
            name,
            Effect::Modify {
                objects: which,
                modifications,
                duration,
            },
        );
    }
    for (kind, name, modifications, duration) in modify_neighbours(modifications, duration) {
        add(
            kind,
            name,
            Effect::Modify {
                objects: which,
                modifications,
                duration,
            },
        );
    }
}

/// The objects whose text an instruction changes, and whether the change lasts the turn or
/// indefinitely, each changed in turn.
pub(super) fn change_text(effect: &Effect, add: &mut impl FnMut(Kind, String, Effect)) {
    let Effect::ChangeText {
        objects: which,
        words,
        duration,
    } = *effect
    else {
        return;
    };
    for (name, which) in objects(which) {
        let objects = which;
        add(
            Kind::EffectObjects,
            name,
            Effect::ChangeText {
                objects,
                words,
                duration,
            },
        );
    }
    for other in [Duration::EndOfTurn, Duration::Indefinite] {
        if other != duration {
            let (objects, duration) = (which, other);
            let mutant = Effect::ChangeText {
                objects,
                words,
                duration,
            };
            add(
                Kind::TextChangeDuration,
                format!("lasting {other:?}"),
                mutant,
            );
        }
    }
}
