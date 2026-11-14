//! Witnesses: fragments of a starting position under which two values of one field of a
//! vocabulary entry give different outcomes.
//!
//! The positions of [`super::position`] exist to make a card playable. Nothing in them puts a
//! regeneration shield in front of a destroy effect, or a permanent its owner does not control
//! in front of "its controller". A witness adds that: permanents and cards in hand, and a
//! fixed prelude of actions that player 0 takes before the opening, such as casting Death Ward
//! on a creature. The generator plays every opening of a card again from each witness of each
//! entry its effect data uses, whichever value the card's definition chose for the field: a
//! Damn written as a plain destroy still meets a shielded creature, and `XMage` and Forge, which
//! read the Oracle text for themselves, then disagree with Beryl.
//!
//! The mutation check (`tests/mutation.rs`) finds a field no witness covers: a mutant of the
//! card with that field changed survives every scenario.

use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Castings, Effect, FaceIx, Players,
};
use beryl_scenario::{Action, P0, Permanent};
use beryl_scenario_types::{ME, Resolve};

use crate::{names, props};

/// A fragment of a starting position, played before an opening.
pub(super) struct Witness {
    /// What it adds, in words, for the names of the scenarios it starts.
    pub(super) words: &'static str,
    /// Player 0's added permanents, which also pay for the prelude.
    pub(super) mine: &'static [Permanent],
    /// Player 1's added permanents.
    pub(super) theirs: &'static [Permanent],
    /// Player 0's added cards in hand, which the prelude casts.
    pub(super) hand: &'static [&'static str],
    /// What player 0 does before the opening, every question answered.
    pub(super) prelude: &'static [Action],
}

/// `Effect::Destroy`'s `regeneration`: a creature that is also a land, with a regeneration
/// shield from Death Ward, so that "can't be regenerated" is tested on a destroy effect that
/// takes creatures and on one that takes lands. With `Regeneration::Allowed` the shield
/// applies and the permanent stays; with `Forbidden` it is destroyed ([CR 701.19c]): "they
/// cause regeneration shields to not be applied."
///
/// [CR 701.19c]: https://yawgatog.com/resources/magic-rules/#R70119c
const REGENERATION_SHIELD: Witness = Witness {
    words: "with a regeneration shield",
    mine: &[
        Permanent::new(names::PLAINS),
        Permanent::new(names::DRYAD_ARBOR),
    ],
    theirs: &[],
    hand: &[props::REGENERATION],
    prelude: &[
        ME.casts(props::REGENERATION),
        ME.targets(names::DRYAD_ARBOR),
        Resolve,
    ],
};

/// A `Players::ControllerOf` or `Players::OwnerOf` in any field: a permanent player 1 owns
/// and player 0 controls, an artifact creature Thieving Skydiver stole, so that "its
/// controller" and "its owner" name different players.
const STOLEN: Witness = Witness {
    words: "with a stolen creature",
    mine: &[
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
        Permanent::new(names::ISLAND),
    ],
    theirs: &[Permanent::new(names::ORNITHOPTER)],
    hand: &[props::THIEF],
    prelude: &[
        Action::Cast {
            player: P0,
            card: props::THIEF,
            face: FaceIx::FRONT,
            costs: Castings::only(0),
        },
        ME.x(1),
        Resolve,
        ME.targets(names::ORNITHOPTER),
        Resolve,
    ],
};

/// `Effect::ExchangeControl`: an artifact player 0 controls, Portable Hole, already on the
/// battlefield, so that an exchange has a permanent of each player's to trade. The opening
/// positions give player 0 lands alone, and an exchange of a permanent "you control" for one
/// an opponent controls then has nothing to target.
const OWN_ARTIFACT: Witness = Witness {
    words: "with an artifact of one's own",
    mine: &[Permanent::new(props::ARTIFACT)],
    theirs: &[],
    hand: &[],
    prelude: &[],
};

/// The witnesses of one instruction's own fields, its children aside.
fn of(effect: &Effect) -> Vec<&'static Witness> {
    let mut out = Vec::new();
    match effect {
        Effect::Destroy { .. } => out.push(&REGENERATION_SHIELD),
        Effect::ExchangeControl(..) => out.push(&OWN_ARTIFACT),
        _ => {}
    }
    let operands = effect.operands();
    let names_an_object_s_player = [operands.players, operands.others]
        .into_iter()
        .flatten()
        .any(|p| matches!(p, Players::ControllerOf(_) | Players::OwnerOf(_)));
    if names_an_object_s_player {
        out.push(&STOLEN);
    }
    out
}

/// Every witness of every instruction of the effect data of `def`'s abilities that resolve,
/// each once.
pub(super) fn witnesses(def: &'static CardDef) -> Vec<&'static Witness> {
    let found = std::cell::RefCell::new(Vec::<&'static Witness>::new());
    let resolves = |a: &Ability| {
        matches!(
            a.kind(),
            AbilityKind::Spell | AbilityKind::Activated | AbilityKind::Triggered
        )
    };
    let abilities = def
        .faces
        .iter()
        .flat_map(|face| face.abilities.iter().chain(face.grants));
    for ability in abilities.filter(|a| resolves(a)) {
        let Behavior::Data(effect) = ability.body() else {
            continue;
        };
        effect.any(&|instruction| {
            let mut found = found.borrow_mut();
            for witness in of(instruction) {
                if !found.iter().any(|&w| std::ptr::eq(w, witness)) {
                    found.push(witness);
                }
            }
            false
        });
    }
    found.into_inner()
}
