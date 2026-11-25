//! Builders for tokens, copies and emblems.

use crate::definition::CardDef;
use crate::effect::{Amount, Effect, Objects, Players};
use crate::index::ExceptIx;

/// [`Effect::CreateTokens`]: "[player] creates [count] [token] tokens".
pub const fn create_tokens(player: Players, token: &'static CardDef, count: u8) -> Effect {
    Effect::CreateTokens {
        player,
        token,
        count: Amount::Fixed(count),
    }
}

/// [`Effect::CreateAttackingTokens`]: "[player] creates [count] [token] tokens that are tapped
/// and attacking [attacking] or a planeswalker they control".
pub const fn create_attacking_tokens(
    player: Players,
    token: &'static CardDef,
    count: u8,
    attacking: Players,
) -> Effect {
    Effect::CreateAttackingTokens {
        player,
        token,
        count: Amount::Fixed(count),
        attacking,
    }
}

/// [`Effect::CreateCopies`] of one exact copy: "[player] creates a token that's a copy of
/// [object]".
pub const fn create_copy(player: Players, of: Objects) -> Effect {
    Effect::CreateCopies {
        player,
        of,
        count: Amount::Fixed(1),
        except: None,
    }
}

/// [`Effect::CreateCopies`] of one copy made with the exception at `except` among those the
/// ability's face lists: "[player] creates a token that's a copy of [object], except ...".
pub const fn create_copy_except(player: Players, of: Objects, except: ExceptIx) -> Effect {
    Effect::CreateCopies {
        player,
        of,
        count: Amount::Fixed(1),
        except: Some(except),
    }
}

/// [`Effect::BecomeCopy`] with the exception at `except` among those the ability's face lists:
/// "[objects] becomes a copy of [object], except ...".
pub const fn become_copy_except(objects: Objects, of: Objects, except: ExceptIx) -> Effect {
    Effect::BecomeCopy {
        objects,
        of,
        except: Some(except),
    }
}

/// [`Effect::CopySpell`] of one copy made with the exception at `except` among those the
/// ability's face lists, for which its controller may choose new targets: "Copy [spell], except
/// .... You may choose new targets for the copy."
pub const fn copy_spell_except(spells: Objects, except: ExceptIx) -> Effect {
    Effect::CopySpell {
        spells,
        count: Amount::Fixed(1),
        except: Some(except),
        new_targets: true,
    }
}

/// [`Effect::CopySpell`] of one copy made with the exception at `except` among those the
/// ability's face lists, with no choice of new targets: "Copy [spell], except ...."
pub const fn copy_spell_except_keeping_targets(spells: Objects, except: ExceptIx) -> Effect {
    Effect::CopySpell {
        spells,
        count: Amount::Fixed(1),
        except: Some(except),
        new_targets: false,
    }
}

/// [`Effect::GetEmblem`]: "[player] gets an emblem with [abilities]".
pub const fn get_emblem(player: Players, emblem: &'static CardDef) -> Effect {
    Effect::GetEmblem { player, emblem }
}
