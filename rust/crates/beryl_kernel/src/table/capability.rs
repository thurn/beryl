//! [`Capability`]: what a game's cards may make the stores keep.

use beryl_card::{Amount, Condition, Face, Node};

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
    /// ([`Condition::DrewInDrawStep`]), so the cards each player draws in their own draw
    /// step are counted.
    DrewInDrawStep,
    /// A card grants another permanent a static ability with a continuous effect, so the
    /// layers may find an ability in layer 6 whose effect belongs to an earlier layer, and
    /// run once more with it present (`beryl_rules`' `layers`).
    GrantsLayerEffect,
    /// A card's static ability reads as the layers apply something no layer writes
    /// ([`Inputs`]), so the recompute finds what the permanents on the battlefield read.
    LayerInputs,
}

impl Capability {
    /// The capabilities a face gives a game that can contain it.
    pub(super) fn of(face: &Face) -> u8 {
        let mut found = 0;
        if Inputs::of(face) != Inputs::NONE {
            found |= Capability::LayerInputs.bit();
        }
        // An ability a permanent is granted reads what one of its own does.
        if face.grants.iter().any(|a| a.layer().is_some()) {
            found |= Capability::GrantsLayerEffect.bit();
        }
        for a in face.abilities.iter().chain(face.grants) {
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
    pub(super) const fn bit(self) -> u8 {
        1 << (self as u8)
    }
}
