//! Trigger events: what a triggered ability triggers on.

use crate::characteristics::Types;
use crate::filter::Whose;
use crate::ids::TurnStep;

/// What a triggered ability triggers on
/// ([CR 603.2](https://yawgatog.com/resources/magic-rules/#R6032)): "Whenever a game event
/// or game state matches a triggered ability's trigger event, that ability automatically
/// triggers." The kinds the cube's cards use.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Trigger {
    None,
    /// "When this enters"
    /// ([CR 603.6a](https://yawgatog.com/resources/magic-rules/#R6036a)).
    Enters,
    /// "When this dies": a leaves-the-battlefield ability, which looks back in time
    /// ([CR 603.10a](https://yawgatog.com/resources/magic-rules/#R60310a)).
    Dies,
    /// "Whenever this attacks"
    /// ([CR 508.3a](https://yawgatog.com/resources/magic-rules/#R5083a)): it "triggers if
    /// that creature is declared as an attacker."
    Attacks,
    /// "Whenever you attack"
    /// ([CR 508.3d](https://yawgatog.com/resources/magic-rules/#R5083d)): it "triggers if
    /// one or more creatures that player controls are declared as attackers."
    YouAttack,
    /// "Whenever this deals combat damage to a player".
    CombatDamageToPlayer,
    /// "At the beginning of [whose] [step]"
    /// ([CR 500.6](https://yawgatog.com/resources/magic-rules/#R5006)): "When a phase or
    /// step begins, any abilities that trigger 'at the beginning of' that phase or step
    /// trigger."
    Step(TurnStep, Whose),
    /// "Whenever you create one or more tokens" of these types: once per event, however
    /// many tokens it creates.
    YouCreateTokens(Types),
    /// "Whenever you cast a spell".
    YouCast,
    /// "Whenever you discard a card": once for each card discarded
    /// ([CR 603.2c](https://yawgatog.com/resources/magic-rules/#R6032c)): "An ability triggers
    /// only once each time its trigger event occurs. However, it can trigger repeatedly if one
    /// event contains multiple occurrences." The discarding player is the card's owner
    /// ([CR 701.9a](https://yawgatog.com/resources/magic-rules/#R7019a)): "To discard a card,
    /// move it from its owner's hand to that player's graveyard."
    YouDiscard,
    /// "Whenever you draw a card": once for each card drawn (CR 603.2c), each draw one event
    /// of its own ([CR 121.2](https://yawgatog.com/resources/magic-rules/#R1212)): "If a
    /// player is instructed to draw multiple cards, that player performs that many individual
    /// card draws." A draw that a replacement effect replaced never happens
    /// ([CR 614.6](https://yawgatog.com/resources/magic-rules/#R6146)), so it triggers nothing.
    YouDraw,
    /// A delayed triggered ability (CR 603.7): never matched against events, but created by
    /// a body, to wait for the moment its text names.
    Delayed(Delayed),
}

/// The moment a delayed triggered ability waits for. It triggers once, the next time that
/// moment comes ([CR 603.7b](https://yawgatog.com/resources/magic-rules/#R6037b)): "A
/// delayed triggered ability will trigger only once--the next time its trigger event
/// occurs--unless it has a stated duration".
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Delayed {
    /// "At the beginning of your next main phase" (Mana Drain): the controller's next
    /// precombat or postcombat main phase.
    #[default]
    YourNextMainPhase,
    /// "At the beginning of the next end step" (Teferi, Hero of Dominaria): whoever's turn it
    /// is ([CR 513.1a](https://yawgatog.com/resources/magic-rules/#R5131a)).
    NextEndStep,
}
