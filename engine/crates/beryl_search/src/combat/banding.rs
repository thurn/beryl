//! Bounded band attack plans for the combat planner (`combat.rs`): when no band option is
//! offered, this adds nothing; when one is, it adds at most [`MAX_BAND_PLANS`] plans so combat
//! search stays bounded.

use beryl_engine::layout::Queue;
use beryl_engine::{Candidates, Decision, Layout, Opt, State};

use super::Declare;

/// The most band plans added to one attacker decision.
const MAX_BAND_PLANS: usize = 6;

/// Appends bounded band plans when `d` offers band declarations, and does nothing otherwise.
pub(super) fn append_plans<L: Layout>(
    start: &State<L>,
    d: &Decision<L>,
    plans: &mut Vec<Vec<Declare<L>>>,
) {
    let Candidates::Options(options) = &d.candidates else {
        return;
    };
    if !options
        .iter()
        .any(|o| matches!(o, Opt::Attack { band: Some(_), .. }))
    {
        return;
    }
    let mut added = 0;
    // If a band is already started, consider joining it with the strongest offered creatures.
    let mut joins: Vec<(L::Slot, L::Slot)> = options
        .iter()
        .filter_map(|&o| match o {
            Opt::Attack {
                slot,
                band: Some(first),
            } if slot != first && { start.derived(slot).power }
                > <L::Pt as beryl_engine::Number>::ZERO =>
            {
                Some((slot, first))
            }
            _ => None,
        })
        .collect();
    joins.sort_by_key(|&(slot, _)| -beryl_engine::SignedNumber::to_i32(start.derived(slot).power));
    for (slot, first) in joins.into_iter().take(MAX_BAND_PLANS) {
        plans.push(vec![Declare::Band {
            slot,
            first,
            target: None,
        }]);
        added += 1;
    }
    if added >= MAX_BAND_PLANS {
        return;
    }
    // Otherwise consider starting a band with a bander and pairing it with another attacker.
    let mut starters: Vec<L::Slot> = options
        .iter()
        .filter_map(|&o| match o {
            Opt::Attack {
                slot,
                band: Some(first),
            } if slot == first && { start.derived(slot).power }
                > <L::Pt as beryl_engine::Number>::ZERO =>
            {
                Some(slot)
            }
            _ => None,
        })
        .collect();
    starters.sort_by_key(|&slot| -beryl_engine::SignedNumber::to_i32(start.derived(slot).power));
    let mut partners: Vec<L::Slot> = options
        .iter()
        .filter_map(|&o| match o {
            Opt::Attack { slot, band: None }
                if { start.derived(slot).power } > <L::Pt as beryl_engine::Number>::ZERO =>
            {
                Some(slot)
            }
            _ => None,
        })
        .collect();
    partners.sort_by_key(|&slot| -beryl_engine::SignedNumber::to_i32(start.derived(slot).power));
    for starter in starters.into_iter().take(2) {
        for &partner in partners.iter().filter(|&&p| p != starter).take(3) {
            if added >= MAX_BAND_PLANS {
                return;
            }
            plans.push(vec![
                Declare::Band {
                    slot: starter,
                    first: starter,
                    target: None,
                },
                Declare::Band {
                    slot: partner,
                    first: starter,
                    target: None,
                },
            ]);
            added += 1;
        }
    }
}
