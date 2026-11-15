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

use beryl_card::{CostAction, CostPart, Filter, Node};
use beryl_engine::Zone;
use beryl_engine::card::{
    Ability, AbilityKind, Behavior, CardDef, Castings, Effect, FaceIx, Objects, Players, Types,
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

/// A target that is an ability on the stack: player 0's Equipment and their one creature,
/// whose equip ability they activate for the card to respond to (`play.rs`), since nothing in
/// the starting positions puts an ability on the stack while its owner holds priority.
const ABILITY_TO_TARGET: Witness = Witness {
    words: "with an ability to target",
    mine: &[
        Permanent::new(props::EQUIPMENT),
        Permanent::new(props::THIEF),
    ],
    theirs: &[],
    hand: &[],
    prelude: &[],
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

/// The starting positions put creatures on the opponent's battlefield. A static
/// effect on creatures its source's controller controls needs one on that side too.
const OWN_CREATURE: Witness = Witness {
    words: "with a creature of one's own",
    mine: &[Permanent::new(props::THIEF)],
    theirs: &[],
    hand: &[],
    prelude: &[],
};

/// A casting option whose cost exiles a card from hand, as Force of Will's or an evoke cost:
/// another blue card in hand, Timetwister. The starting hand's blue card is the creature
/// spell player 0 casts for a counterspell to respond to, so without this a counterspell
/// with such a cost would have no card left to exile.
const CARD_TO_EXILE: Witness = Witness {
    words: "with another blue card in hand",
    mine: &[],
    theirs: &[],
    hand: &[props::UNTARGETED_SPELL],
    prelude: &[],
};

/// Whether a casting option of `def` exiles a card from its caster's hand.
fn exiles_from_hand(def: &CardDef) -> bool {
    (def.faces.iter().flat_map(|face| face.casting)).any(|option| {
        option.parts().iter().any(|part| {
            matches!(
                part,
                CostPart::Chosen(CostAction::Exile, chosen) if chosen.zone == Zone::Hand
            )
        })
    })
}

fn affects_own_creatures(ability: &Ability) -> bool {
    let Some(Objects::All {
        zone: Zone::Battlefield,
        filter,
    }) = ability.layer().map(|layer| layer.affected)
    else {
        return false;
    };
    let mut creature = false;
    let mut own = false;
    filter.walk(&mut |node| match node {
        Node::Filter(Filter::Types(types)) => creature |= types.intersects(Types::CREATURE),
        Node::Filter(Filter::Whose(Players::You)) => own = true,
        _ => {}
    });
    creature && own
}

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

/// The witnesses of `def`'s static abilities and resolving instructions, each once.
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
    if abilities.clone().any(affects_own_creatures) {
        found.borrow_mut().push(&OWN_CREATURE);
    }
    let targets_abilities =
        |a: &Ability| (a.targets().iter()).any(|t| t.kind.abilities().is_some());
    if abilities.clone().any(targets_abilities) {
        found.borrow_mut().push(&ABILITY_TO_TARGET);
    }
    if exiles_from_hand(def) {
        found.borrow_mut().push(&CARD_TO_EXILE);
    }
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
