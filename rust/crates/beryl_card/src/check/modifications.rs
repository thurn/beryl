//! What the card table refuses of a continuous effect's modifications: those of a resolving
//! effect, and those every continuous effect checks.

use super::{Checked, Malformed, Scope, both};
use crate::continuous::{MODIFY_PARTS, Modification};
use crate::effect::Objects;

impl Scope<'_> {
    /// The parts of a resolving effect: at least one, each checked.
    pub(super) const fn resolved_modifications(
        self,
        objects: &Objects,
        modifications: &[Modification],
    ) -> Checked {
        let mut checked = match modifications.len() {
            0 => Err(Malformed::NoModification),
            n if n > MODIFY_PARTS => Err(Malformed::Parts),
            _ => Ok(()),
        };
        let mut i = 0;
        while i < modifications.len() {
            let part = match modifications[i] {
                Modification::AddChosenColor => Err(Malformed::ChosenColor),
                // "It loses [an ability of its own]": a resolving effect may name an ability
                // of the face of the permanent it is on, as Animate Dead's enters ability
                // does of its enchant ability.
                Modification::LoseAbility(ability) => {
                    if matches!(objects, Objects::This)
                        && ability.index() < self.face.abilities.len()
                    {
                        Ok(())
                    } else {
                        Err(Malformed::Lost)
                    }
                }
                Modification::Control
                | Modification::AddTypes(_)
                | Modification::RemoveTypes(_)
                | Modification::SetTypes(_)
                | Modification::AddSubtypeWord(_)
                | Modification::SetSubtypeWord(_)
                | Modification::SetLandTypeWords(_)
                | Modification::AddKeywords(_)
                | Modification::Grant(_)
                | Modification::SetPower(_)
                | Modification::SetPt(..)
                | Modification::ModifyPt(..)
                | Modification::LoseAllAbilities
                | Modification::SetColorWords(_)
                | Modification::AddColorWords(_)
                | Modification::SetBasePt(..)
                | Modification::SwitchPt
                | Modification::AddProtection(_)
                | Modification::GrantEscape => self.modification(&modifications[i]),
            };
            checked = both(checked, part);
            i += 1;
        }
        checked
    }

    pub(super) const fn modification(self, modification: &Modification) -> Checked {
        match modification {
            Modification::Grant(grant) if grant.index() >= self.face.grants.len() => {
                Err(Malformed::Grant)
            }
            Modification::SetPower(amount) => self.amount(amount),
            Modification::SetPt(power, toughness) | Modification::SetBasePt(power, toughness) => {
                both(self.amount(power), self.amount(toughness))
            }
            // A resolving effect names an ability of its own permanent's face only of that
            // permanent (`resolved_modifications`).
            Modification::LoseAbility(_) => Err(Malformed::Lost),
            Modification::Control
            | Modification::AddTypes(_)
            | Modification::RemoveTypes(_)
            | Modification::SetTypes(_)
            | Modification::AddSubtypeWord(_)
            | Modification::SetSubtypeWord(_)
            | Modification::SetLandTypeWords(_)
            | Modification::AddKeywords(_)
            | Modification::Grant(_)
            | Modification::ModifyPt(..)
            | Modification::LoseAllAbilities
            | Modification::SetColorWords(_)
            | Modification::AddColorWords(_)
            | Modification::AddChosenColor
            | Modification::SwitchPt
            | Modification::AddProtection(_)
            | Modification::GrantEscape => Ok(()),
        }
    }
}
