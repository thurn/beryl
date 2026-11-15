//! Conditions on the costs paid for a spell: "if it was kicked", "if it escaped".

use super::Ctx;
use crate::cost::CastingKind;

impl Ctx {
    /// "It was kicked", "its impending cost was paid" (CR 702.33d).
    pub(super) fn paid_condition(&mut self, kind: CastingKind, paid: bool) -> String {
        let it = if self.this_is_it || self.trigger.is_some() {
            "it".to_owned()
        } else {
            self.this.clone()
        };
        let name = match kind {
            CastingKind::Kicker => {
                return format!("{it} {} kicked", if paid { "was" } else { "wasn't" });
            }
            CastingKind::Offspring => "offspring",
            CastingKind::Impending => "impending",
            CastingKind::Overload => "overload",
            CastingKind::Evoke => "evoke",
            CastingKind::Alternative => "alternative",
            CastingKind::Escape => {
                return if paid {
                    format!("{it} escaped")
                } else {
                    format!("{it} didn't escape")
                };
            }
        };
        let whose = if it == "it" {
            "its".to_owned()
        } else {
            format!("{it}'s")
        };
        format!(
            "{whose} {name} cost {} paid",
            if paid { "was" } else { "wasn't" }
        )
    }
}
