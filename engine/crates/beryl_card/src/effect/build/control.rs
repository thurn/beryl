//! Builders for continuous effects, control changes and a permanent's status: tapped,
//! phased out, attached, prepared or regenerating.

use crate::continuous::{Duration, ManaChange, Modification, RuleChange};
use crate::effect::{Amount, Effect, Objects, Players};

/// [`Effect::Modify`]: "[objects] get, gain or become [modifications] [duration]", the
/// parts of one effect that begin together.
pub const fn modify(
    objects: Objects,
    modifications: &'static [Modification],
    duration: Duration,
) -> Effect {
    Effect::Modify {
        objects,
        modifications,
        duration,
    }
}

/// [`Effect::ChangeText`]: "change the text of [objects] by replacing all instances of
/// [words] [duration]", such as "one basic land type with another"
/// ([`crate::TextWords::LandTypes`]).
pub const fn change_text(objects: Objects, words: crate::TextWords, duration: Duration) -> Effect {
    Effect::ChangeText {
        objects,
        words,
        duration,
    }
}

/// [`Effect::ExchangeControl`]: "exchange control of [first] and [second]".
pub const fn exchange_control(first: Objects, second: Objects) -> Effect {
    Effect::ExchangeControl(first, second)
}

/// [`Effect::ChangeRule`]: "[rule change] [duration]".
pub const fn change_rule(rule: &'static RuleChange, duration: Duration) -> Effect {
    Effect::ChangeRule { rule, duration }
}

/// [`Effect::ChangeMana`]: "[duration], whenever a player taps ... for mana, that player adds
/// an additional ...".
pub const fn change_mana(change: &'static ManaChange, duration: Duration) -> Effect {
    Effect::ChangeMana { change, duration }
}

/// [`Effect::SetPrepared`]: "[permanents] become prepared."
pub const fn prepare(objects: Objects) -> Effect {
    Effect::SetPrepared {
        objects,
        prepared: true,
    }
}

/// [`Effect::SetPrepared`]: "[permanents] become unprepared."
pub const fn unprepare(objects: Objects) -> Effect {
    Effect::SetPrepared {
        objects,
        prepared: false,
    }
}

/// [`Effect::PhaseOut`]: "[permanents] phase out".
pub const fn phase_out(objects: Objects) -> Effect {
    Effect::PhaseOut(objects)
}

/// [`Effect::Regenerate`]: "regenerate [permanents]".
pub const fn regenerate(objects: Objects) -> Effect {
    Effect::Regenerate(objects)
}

/// [`Effect::Attach`]: "attach [objects] to [host]".
pub const fn attach(objects: Objects, to: Objects) -> Effect {
    Effect::Attach { objects, to }
}

/// [`Effect::UntapUpTo`]: "[player] untaps up to [count] [permanents]".
pub const fn untap_up_to(player: Players, count: u8, permanents: Objects) -> Effect {
    Effect::UntapUpTo {
        player,
        count: Amount::Fixed(count),
        permanents,
    }
}
