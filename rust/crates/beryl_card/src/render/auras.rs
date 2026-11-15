//! Templates for what is said only of Auras: an enchant ability, and the card an Aura
//! enchants in a graveyard.

use super::effects::says_nothing;
use super::{Attached, Ctx};
use crate::ability::{Ability, AbilityKind};
use crate::effect::Objects;

impl Ctx {
    /// An Aura's enchant ability, such as "Enchant creature card in a graveyard" or "Enchant
    /// creature put onto the battlefield with this Aura": what its spell ability targets, with
    /// no effect ([CR 303.4a](https://yawgatog.com/resources/magic-rules/#R3034a)): "An Aura
    /// spell requires a target, which is defined by its enchant ability."
    pub(super) fn enchant(&mut self, ability: &Ability) -> Option<String> {
        if ability.kind() != AbilityKind::Spell || !says_nothing(ability.body()) {
            return None;
        }
        let &spec = ability.targets().first()?;
        let noun = self.target_noun(spec);
        let mut text = format!("Enchant {noun}");
        if spec.kind == crate::TargetKind::GraveyardCard {
            text.push_str(" in a graveyard");
        }
        if spec.put_with_this {
            text.push_str(" put onto the battlefield with ");
            text.push_str(&self.this);
        }
        Some(text)
    }

    /// [`Ctx::objects`], as an instruction that moves them names them: the card an Aura
    /// enchants in a graveyard is "enchanted creature card".
    pub(super) fn moved_card(&mut self, objects: Objects) -> String {
        let named = self.objects(objects);
        match (objects, self.attached) {
            (Objects::AttachedTo, Attached::Aura { card: true, .. }) => format!("{named} card"),
            _ => named,
        }
    }
}
