//! What a prevention shield and a replacement of damage say: "prevent the next 4 damage that
//! would be dealt to any target this turn", "the next time a red source of your choice would
//! deal damage to you this turn, prevent that damage".

use super::Ctx;
use super::filters::Kind;
use super::words::number;
use crate::effect::{Objects, Players, Prevention, Recipients};
use crate::filter::Filter;

impl Ctx {
    /// Who is dealt damage: "you", "target creature", "any target".
    pub(super) fn recipients(&mut self, to: Recipients) -> String {
        match to {
            Recipients::Players(players) => self.player(players),
            Recipients::Permanents(objects) => self.objects(objects),
            // "Any target": the one target, said once.
            Recipients::Both(Players::Target(a), Objects::Target(b)) if a == b => {
                self.objects(Objects::Target(b))
            }
            Recipients::Both(players, objects) => {
                format!("{} and {}", self.objects(objects), self.player(players))
            }
        }
    }

    /// The clause of an [`crate::Effect::PreventDamage`].
    pub(super) fn prevention(
        &mut self,
        to: Recipients,
        prevention: Prevention,
        from: Option<&'static Filter>,
    ) -> String {
        let to = self.recipients(to);
        let source = from.map(|filter| {
            let noun = self.noun(*filter, Kind::Source).indefinite(Kind::Source);
            format!("{noun} of your choice")
        });
        match (prevention, source) {
            (Prevention::NextTime, source) => {
                let source = source.unwrap_or_else(|| "a source".to_owned());
                format!(
                    "the next time {source} would deal damage to {to} this turn, prevent that \
                     damage"
                )
            }
            (Prevention::Next(n), None) => {
                format!("prevent the next {n} damage that would be dealt to {to} this turn")
            }
            (Prevention::Next(n), Some(source)) => {
                format!("prevent the next {n} damage that {source} would deal to {to} this turn")
            }
        }
    }
}

/// The clause of a replacement that multiplies damage ([`crate::Rewrite::Multiply`]): "if a
/// source would deal damage to a permanent or player, it deals double that damage to that
/// permanent or player instead".
pub(super) fn multiplied(n: u8) -> String {
    let times = match n {
        2 => "double".to_owned(),
        3 => "triple".to_owned(),
        n => format!("{} times", number(n)),
    };
    format!(
        "if a source would deal damage to a permanent or player, it deals {times} that damage \
         to that permanent or player instead"
    )
}
