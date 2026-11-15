//! [`Capability`]: what a game's cards may make the stores keep.

use beryl_card::{
    Amount, Condition, Draws, Face, Modification, Node, Replaced, Replacement, Rewrite,
};

use crate::static_ability::Inputs;

/// Something a game's cards may make the stores keep (architecture §6.4, the crate's
/// contract, "Stores"), found once per game from every face of every card it can contain.
/// What no card of a game can need is not kept in it and costs it one AND.
///
/// The kinds of continuous effect have no bit here: the Effects store knows exactly which
/// are live ([`crate::effects`]), which a table could only bound, since a Rust body is
/// opaque to it. These are what no live entry can say: whether to keep a count nothing has
/// yet read. Whether a static ability changes a rule for players is no capability of the
/// game either: it is a bit of each card's replacement mask ([`crate::event::Replaces::PLAYER_RULE_CHANGE`]),
/// which says it of the permanents on the battlefield now.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Capability {
    /// A card reads whether a permanent entered the battlefield this turn, so that is
    /// tracked.
    EnteredThisTurn,
    /// A card has a loyalty ability, so whether one of a permanent's was activated this turn is
    /// tracked ([CR 606.3]): "A player may activate a loyalty ability of a permanent
    /// they control any time they have priority and the stack is empty during a main phase of their
    /// turn, but only if no player has previously activated a loyalty ability of that permanent
    /// that turn."
    ///
    /// [CR 606.3]: https://yawgatog.com/resources/magic-rules/#R6063
    LoyaltyActivated,
    /// A card reads how many spells a player cast this turn
    /// ([`Amount::SpellsCast`]), so that is counted for each player.
    SpellsCast,
    /// A card reads how many cards a player drew this turn ([`Amount::CardsDrawn`]), so that
    /// is counted for each player.
    CardsDrawn,
    /// A card asks whether a player has drawn the first card of their draw step this turn
    /// ([`Condition::DrewInDrawStep`]), or replaces every draw but that one
    /// ([`Draws::ExceptFirstInDrawStep`]), so the cards each player draws in their own draw
    /// step are counted.
    DrewInDrawStep,
    /// A card grants another permanent an ability, so the abilities an object has
    /// ([`crate::State::abilities`]) include those its layer results say it was granted.
    Grants,
    /// A card grants another permanent a static ability with a continuous effect, so the
    /// layers may find an ability in layer 6 whose effect belongs to an earlier layer, and
    /// run once more with it present (`beryl_rules`' `layers`).
    GrantsLayerEffect,
    /// A card's static ability reads as the layers apply something no layer writes
    /// ([`Inputs`]), so the recompute finds what the permanents on the battlefield read.
    LayerInputs,
    /// A card has dredge ([`beryl_card::Replacement::dredge`]), a replacement of draws that
    /// works from a graveyard, so a draw looks there for one.
    Dredge,
    /// A card chooses a value as it enters ([`beryl_card::Replacement::choose_as_enters`]), a
    /// choice that may come up wherever a permanent enters, so every resolution runs where it
    /// can pause for it (`beryl_rules`' `resolve`).
    ChoosesAsEnters,
    /// A card's static ability affects objects in every zone
    /// ([`beryl_card::Objects::Everywhere`]), so the recompute lists such effects for the
    /// reads of characteristics off the battlefield ([`crate::elsewhere`]).
    ReachesElsewhere,
    /// A card has a mana ability whose mana is restricted ([`beryl_card::Ability::spend`]),
    /// so pools may hold mana that remembers ([`crate::mana_memory`]) and the mana solver
    /// looks for it.
    RemembersMana,
    /// A card has a characteristic-defining ability that defines its power or toughness
    /// ([`beryl_card::Modification::SetPower`], [`beryl_card::Modification::SetPt`]), which
    /// works in every zone, so a read of the power of an object off the battlefield looks for
    /// one ([`crate::defined`]).
    DefinesPt,
    /// A card has delve ([`beryl_card::Keywords::DELVE`]), so paying a spell's mana looks
    /// for cards in its caster's graveyard to pay generic mana with, and whether a spell is
    /// castable counts them.
    Delve,
}

impl Capability {
    /// The capabilities a face gives a game that can contain it.
    pub(super) fn of(face: &Face) -> u16 {
        let mut found = 0;
        if face.keywords.contains(beryl_card::Keywords::DELVE) {
            found |= Capability::Delve.bit();
        }
        if Inputs::of(face) != Inputs::NONE {
            found |= Capability::LayerInputs.bit();
        }
        if !face.grants.is_empty() {
            found |= Capability::Grants.bit();
        }
        // An ability a permanent is granted reads what one of its own does.
        if face.grants.iter().any(|a| a.layer().is_some()) {
            found |= Capability::GrantsLayerEffect.bit();
        }
        for a in face.abilities.iter().chain(face.grants) {
            match a.replacement() {
                Some(&Replacement {
                    rewrite: Rewrite::Dredge(_),
                    ..
                }) => found |= Capability::Dredge.bit(),
                Some(&Replacement {
                    replaces: Replaced::Draw(_, Draws::ExceptFirstInDrawStep),
                    ..
                }) => found |= Capability::DrewInDrawStep.bit(),
                _ => {}
            }
            if let Some(&Replacement {
                rewrite: Rewrite::Choose(_),
                ..
            }) = a.replacement()
            {
                found |= Capability::ChoosesAsEnters.bit();
            }
            if let Some(spec) = a.layer()
                && matches!(spec.affected, beryl_card::Objects::Everywhere(_))
            {
                found |= Capability::ReachesElsewhere.bit();
            }
            if let Some(spec) = a.layer()
                && (spec.modifications.iter())
                    .any(|m| matches!(m, Modification::SetPower(_) | Modification::SetPt(..)))
            {
                found |= Capability::DefinesPt.bit();
            }
            if a.spend().is_some() {
                found |= Capability::RemembersMana.bit();
            }
            if a.cost().is_loyalty() {
                found |= Capability::LoyaltyActivated.bit();
            }
            // Wherever the ability holds the amount or condition that reads it: its own
            // condition, one its effect reads, its continuous effect's.
            a.walk(&mut |node| {
                found |= match node {
                    Node::Condition(Condition::EnteredThisTurn(_)) => {
                        Capability::EnteredThisTurn.bit()
                    }
                    Node::Amount(Amount::SpellsCast(_)) => Capability::SpellsCast.bit(),
                    Node::Amount(Amount::CardsDrawn(_)) => Capability::CardsDrawn.bit(),
                    Node::Condition(Condition::DrewInDrawStep(_)) => {
                        Capability::DrewInDrawStep.bit()
                    }
                    _ => 0,
                };
            });
        }
        found
    }

    #[inline]
    pub(super) const fn bit(self) -> u16 {
        1 << (self as u16)
    }
}
