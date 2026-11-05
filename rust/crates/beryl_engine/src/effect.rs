//! `EffectCtx`: the only way a card body touches the state (architecture §6.3).

use crate::card::{Ability, ManaColor};
use crate::state::{ObjectRef, State};

/// What a body runs against: the working state, the ability's source and controller. The
/// primitives grow with the bodies that need them.
pub struct EffectCtx<'a> {
    pub(crate) state: &'a mut State,
    pub(crate) source: ObjectRef,
    pub(crate) controller: u8,
    pub(crate) ability: &'static Ability,
    /// For a mana ability that can produce more than one kind of mana, the kind its
    /// activator chose.
    pub(crate) mana: ManaColor,
}

impl EffectCtx<'_> {
    pub fn source(&self) -> ObjectRef {
        self.source
    }

    pub fn controller(&self) -> u8 {
        self.controller
    }

    /// Adds mana to a player's mana pool
    /// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064)): "When an effect
    /// instructs a player to add mana, that mana goes into a player's mana pool."
    pub fn add_mana(&mut self, player: u8, color: ManaColor, amount: u8) {
        let pool = &mut self.state.player_mut(player).mana[color as usize];
        *pool = pool.saturating_add(amount);
    }
}

/// The body of a mana ability: add what it produces, of the kind its activator chose
/// ([CR 106.3](https://yawgatog.com/resources/magic-rules/#R1063)): "A spell or ability that
/// produces mana instructs a player to add that mana."
pub fn add_produced_mana(ctx: &mut EffectCtx) -> crate::card::Flow {
    let produces = ctx.ability.produces;
    debug_assert!(produces.includes(ctx.mana));
    ctx.add_mana(ctx.controller, ctx.mana, produces.amount);
    Ok(())
}
