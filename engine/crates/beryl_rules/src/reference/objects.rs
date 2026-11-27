//! The reference for spells that creatures or artifacts help pay for, convoke's and
//! improvise's: every set of the objects the spell could tap, each paying one unit of its own
//! kinds, with the mana of the rest matched symbol by symbol as [`super::reference_pays`]
//! matches it, or, in a game whose cards have priced mana abilities or change mana abilities,
//! paid by every sequence of mana abilities ([`sequences::reference_sequences_with`]). It reads the keywords off the spell's face and the objects off the battlefield,
//! and shares no code with the casting steps' check (`Funds::pays_with_objects`), which merges
//! sources and objects instead of trying each set.
//!
//! Implements: none. Checks the payability of convoke and improvise spells by brute force.

use crate::bitboard::Bitboard;
use crate::card::{AbilityKind, Keywords, ManaCost, Types};
use crate::casting;
use crate::ids::Seat;
use crate::invariants::{At, Violation};
use crate::layout::{Layout, Queue};
use crate::mana::Paying;
use crate::state::State;
use crate::table::{Capability, GameTable};

use super::{check, reference_pays, reference_units, sequences};

/// The most objects a spell could tap for the brute force to try every set of them: past it,
/// the spell is not checked.
const MOST: usize = 12;

/// Whether `player` can pay the mana cost of each card in their hand with convoke or improvise
/// for casting it, as the casting steps say (`casting::pays_with_objects`), against
/// [`reference_pays_with_objects`].
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face a reference cast pays for"
)]
pub(super) fn check_objects<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Result<(), Violation> {
    for &slot in s.hand_by_definition(player).iter() {
        let Some(def) = s.card_def(t, slot) else {
            continue;
        };
        for face in def.face_ixs() {
            // X is announced before payment, as 0 here.
            let cost = ManaCost {
                x: 0,
                ..def.face(face).cost
            };
            let Some(expected) = reference_pays_with_objects(s, t, player, slot, face, cost) else {
                continue;
            };
            let Ok(Some(found)) = casting::pays_with_objects(s, t, player, slot, face, cost) else {
                continue;
            };
            check(
                found == expected,
                "the payment with creatures or artifacts disagrees with the reference",
                Some(At::Player(player)),
            )?;
        }
    }
    Ok(())
}

/// Whether `player` can pay `cost` for casting the card in `slot` as its face `face`, tapping
/// some of the untapped creatures they control for its convoke, or artifacts for its
/// improvise, and paying the rest with mana: `None` for a face with neither, where
/// [`reference_units`] cannot say the units, or past [`MOST`] objects.
///
/// See [CR 702.51a](https://yawgatog.com/resources/magic-rules/#R70251a): "For each colored
/// mana in this spell's total cost, you may tap an untapped creature of that color you
/// control rather than pay that mana. For each generic mana in this spell's total cost, you
/// may tap an untapped creature you control rather than pay that mana." See
/// [CR 702.126a](https://yawgatog.com/resources/magic-rules/#R702126a): "For each generic mana
/// in this spell's total cost, you may tap an untapped artifact you control rather than pay
/// that mana."
#[allow(
    clippy::disallowed_methods,
    reason = "reads the face a reference cast pays for"
)]
pub(crate) fn reference_pays_with_objects<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    slot: L::Slot,
    face: crate::card::FaceIx,
    cost: ManaCost,
) -> Option<bool> {
    let keywords = s.card_def(t, slot)?.face(face).keywords;
    let (types, convoke) = if keywords.contains(Keywords::CONVOKE) {
        (Types::CREATURE, true)
    } else if keywords.contains(Keywords::IMPROVISE) {
        (Types::ARTIFACT, false)
    } else {
        return None;
    };
    let mut objects = [slot; MOST];
    let mut len = 0;
    for &o in s.battlefield() {
        let d = s.derived(o);
        if d.controller != player || s.object(o).is_tapped() || !d.types.intersects(types) {
            continue;
        }
        if len == MOST {
            return None;
        }
        objects[len] = o;
        len += 1;
    }
    let objects = &objects[..len];
    let paying = Paying::casting(s, t, slot, face);
    let rich = t.can_either(Capability::PricedMana, Capability::ManaChanges);
    // In a game with priced mana abilities or abilities that change mana, an object with no mana
    // ability is always tapped: it only adds to what pays, and the sequences are slow to try.
    let idle = (0..len)
        .filter(|&i| rich && !makes_mana(s, t, objects[i]))
        .fold(0u32, |m, i| m | (1 << i));
    let mut known = true;
    for tapped in 0u32..1 << len {
        if tapped & idle != idle {
            continue;
        }
        let chosen: Bitboard<L::Slot> = (0..len)
            .filter(|&i| tapped & (1 << i) != 0)
            .map(|i| objects[i])
            .collect();
        if rich {
            let spare: Vec<u8> = (0..len)
                .filter(|&i| tapped & (1 << i) != 0)
                .map(|i| kinds_of(s, convoke, objects[i]))
                .collect();
            match sequences::reference_sequences_with(s, t, player, cost, paying, chosen, &spare) {
                Some(true) => return Some(true),
                Some(false) => {}
                None => known = false,
            }
            continue;
        }
        let Some(mut units) = reference_units(s, t, player, paying, chosen) else {
            known = false;
            continue;
        };
        let mut full = false;
        for (i, &o) in objects.iter().enumerate() {
            if tapped & (1 << i) != 0 {
                full |= units.push(kinds_of(s, convoke, o)).is_err();
            }
        }
        if full {
            known = false;
            continue;
        }
        if reference_pays(units.as_slice(), cost) {
            return Some(true);
        }
    }
    known.then_some(false)
}

/// The kinds of mana the object in `slot` pays when tapped: a creature's colors for convoke, as
/// the layers give them; generic mana only for a colorless creature or an artifact. Never a
/// colorless symbol.
fn kinds_of<L: Layout>(s: &State<L>, convoke: bool, slot: L::Slot) -> u8 {
    if convoke { s.derived(slot).colors.0 } else { 0 }
}

/// Whether the permanent in `slot` has a mana ability, its own or the intrinsic one of a basic
/// land type (see CR 305.6).
fn makes_mana<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    s.intrinsic_mana(t, slot) != 0
        || s.abilities(t, slot)
            .any(|(_, a)| a.kind() == AbilityKind::Mana)
}
