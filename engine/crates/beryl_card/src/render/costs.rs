//! The template of a cost ([`Cost`]) as it is written before an activated ability's colon
//! or after "as an additional cost to cast this spell".

use super::Ctx;
use super::filters::{Kind, zone_name};
use super::words::{capitalized, counted, list, mana_text, number, with_article};
use crate::cost::{Chosen, Cost, CostAction, CostPart, Measure};
use crate::ids::Zone;

impl Ctx {
    /// A cost as it is written before the colon
    /// ([CR 602.1a](https://yawgatog.com/resources/magic-rules/#R6021a)): "{2}, {T}",
    /// "{T}, Sacrifice this land", "+2", "Waterbend {3}".
    pub(super) fn cost_text(&mut self, cost: &Cost) -> String {
        if let Some(loyalty) = cost.loyalty_change() {
            // A minus sign, as printed, rather than a hyphen.
            return match loyalty {
                0 => "0".to_owned(),
                l if l > 0 => format!("+{l}"),
                l => format!("\u{2212}{}", l.unsigned_abs()),
            };
        }
        let mut parts = Vec::new();
        if matches!(cost.measure(), Some(Measure::ForGeneric(_))) {
            parts.push(format!("Waterbend {}", mana_text(cost.mana)));
        } else if !cost.mana.is_zero() {
            parts.push(mana_text(cost.mana));
        }
        parts.extend(self.cost_actions(*cost).iter().map(|a| capitalized(a)));
        if parts.is_empty() {
            "{0}".to_owned()
        } else {
            parts.join(", ")
        }
    }

    /// What paying a cost does besides its mana, each part as an instruction: "{T}",
    /// "sacrifice this land", "exile six cards from your graveyard", "pay 3 life".
    pub(super) fn cost_actions(&mut self, cost: Cost) -> Vec<String> {
        cost.parts()
            .iter()
            .filter_map(|&part| self.cost_part(part))
            .collect()
    }

    /// One part of a cost as an instruction, or `None` for one the cost says elsewhere: a
    /// loyalty change, or the tapping of a waterbend cost.
    fn cost_part(&mut self, part: CostPart) -> Option<String> {
        Some(match part {
            CostPart::This(CostAction::Tap) => "{T}".to_owned(),
            CostPart::This(CostAction::Sacrifice) => format!("sacrifice {}", self.this),
            CostPart::This(CostAction::Discard) => "discard this card".to_owned(),
            CostPart::This(CostAction::Exile) => format!("exile {}", self.this),
            CostPart::This(CostAction::Reveal) => "reveal this card".to_owned(),
            CostPart::This(CostAction::RemoveCounter(kind)) => {
                format!(
                    "remove {} counter from {}",
                    with_article(kind.name()),
                    self.this
                )
            }
            CostPart::Chosen(action, chosen) => return self.chosen(action, chosen),
            CostPart::Loyalty(_) => return None,
            CostPart::Life(n) => format!("pay {n} life"),
            CostPart::DiscardHand => "discard your hand".to_owned(),
            CostPart::OneOf(costs) => {
                let each: Vec<String> = costs.iter().map(|c| self.cost_text(c)).collect();
                list(&each, "or")
            }
        })
    }

    /// A cost part that acts on objects its payer chooses: "sacrifice another creature",
    /// "exile a blue card from your hand", "tap any number of untapped creatures you
    /// control with total power 5 or more".
    fn chosen(&mut self, action: CostAction, chosen: Chosen) -> Option<String> {
        let verb = match action {
            CostAction::Tap => "tap",
            CostAction::Sacrifice => "sacrifice",
            CostAction::Discard => "discard",
            CostAction::Exile => "exile",
            CostAction::Reveal => "reveal",
            CostAction::RemoveCounter(_) => "remove a counter from",
        };
        let kind = match chosen.zone {
            Zone::Battlefield | Zone::Nowhere | Zone::Stack => Kind::Permanent,
            Zone::Library | Zone::Hand | Zone::Graveyard | Zone::Exile | Zone::Merged => {
                Kind::Card(None)
            }
        };
        let noun = self.noun(chosen.filter, kind);
        let from = match chosen.zone {
            Zone::Library | Zone::Graveyard | Zone::Exile => {
                format!(" from your {}", zone_name(chosen.zone))
            }
            Zone::Hand if action != CostAction::Discard => " from your hand".to_owned(),
            Zone::Hand | Zone::Battlefield | Zone::Stack | Zone::Nowhere | Zone::Merged => {
                String::new()
            }
        };
        let things = match chosen.measure {
            Measure::Count(1) if chosen.other => format!("another {}", noun.singular(kind)),
            Measure::Count(1) => noun.indefinite(kind),
            Measure::Count(n) if chosen.other => {
                format!("{} other {}", number(n), noun.plural(kind))
            }
            Measure::Count(n) => counted(n, &noun.singular(kind), &noun.plural(kind)),
            Measure::TotalPower(n) => format!(
                "any number of {}untapped {} with total power {n} or more",
                if chosen.other { "other " } else { "" },
                noun.plural(kind)
            ),
            Measure::ForGeneric(_) => return None,
        };
        Some(format!("{verb} {things}{from}"))
    }
}
