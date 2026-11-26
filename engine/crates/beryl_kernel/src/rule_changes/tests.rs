//! The numbering and classing of rule changes.

use super::fixtures::{FLASH, LIMIT, LOYALTY, table};
use super::*;

#[test]
fn a_game_numbers_the_rule_changes_its_cards_name_once_each() {
    let t = table();
    // In order of first appearance, whichever kind of ability names one.
    let [loyalty, limit, flash] = [&LOYALTY, &LIMIT, &FLASH].map(|rule| t.rule_id(rule).unwrap());
    assert_eq!([loyalty.index(), limit.index(), flash.index()], [0, 1, 2]);
    assert_eq!(t.rule_id(t.rule(limit)), Some(limit));
    assert_eq!(
        t.rules_of(RuleKind::CastOnlyAsSorcery),
        Rules::NONE.with(limit)
    );
    assert_eq!(t.rules_of(RuleKind::CastAsInstant), Rules::NONE.with(flash));
    assert_eq!(
        t.rules_of(RuleKind::LoyaltyAsInstant),
        Rules::NONE.with(loyalty)
    );
    assert_eq!(t.player_rules(), Rules::NONE.with(limit).with(flash));
    let all = Rules::NONE.with(loyalty).with(limit).with(flash);
    assert_eq!(all.iter().collect::<Vec<_>>(), [loyalty, limit, flash]);
    assert!(all.without(t.player_rules()).contains(loyalty));
    assert!(Rules::NONE.is_empty() && Rules::NONE.common(all).is_empty());
}

/// "This creature can't block" is checked of the creature with the ability; blocking
/// with other creatures, or a change to when one blocks, is refused.
#[test]
fn only_a_creatures_own_cant_block_is_checked() {
    let block = |players, what, when| {
        RuleKind::of(&RuleChange {
            players,
            what: Governed::Block(what),
            when,
        })
    };
    assert_eq!(
        block(Players::You, Objects::This, When::Never),
        Ok(RuleKind::CantBlock)
    );
    assert!(!RuleKind::CantBlock.of_players());
    assert_eq!(
        block(Players::EachOpponent, Objects::This, When::Never),
        Err(Unchecked::Players)
    );
    assert_eq!(
        block(Players::You, Objects::Carried, When::Never),
        Err(Unchecked::Rule)
    );
    assert_eq!(
        block(Players::You, Objects::This, When::AsInstant),
        Err(Unchecked::Rule)
    );
}

/// "This creature can't attack unless defending player controls an Island" is checked of
/// the creature with the ability; attacking with other creatures, or a change to when one
/// attacks, is refused.
#[test]
fn only_a_creatures_own_attack_restriction_is_checked() {
    let attack = |players, attackers, when| {
        RuleKind::of(&RuleChange {
            players,
            what: Governed::Attack {
                attackers,
                unless_defender_controls: Some(Filter::Any),
            },
            when,
        })
    };
    assert_eq!(
        attack(Players::You, Objects::This, When::Never),
        Ok(RuleKind::CantAttack)
    );
    assert!(!RuleKind::CantAttack.of_players());
    assert_eq!(
        attack(Players::EachOpponent, Objects::This, When::Never),
        Err(Unchecked::Players)
    );
    assert_eq!(
        attack(Players::You, Objects::Carried, When::Never),
        Err(Unchecked::Rule)
    );
    assert_eq!(
        attack(Players::You, Objects::This, When::OnlyAsSorcery),
        Err(Unchecked::Rule)
    );
}

/// "Activated abilities of sources with the chosen name can't be activated unless they're
/// mana abilities" applies to players; mana abilities, or a change to when the abilities
/// may be activated, are refused.
#[test]
fn a_prohibition_on_named_sources_applies_to_players() {
    let activate = |players, mana_abilities, when| {
        RuleKind::of(&RuleChange {
            players,
            what: Governed::ActivateAbilities {
                sources: Named::Chosen,
                mana_abilities,
            },
            when,
        })
    };
    let kind = activate(Players::EachPlayer, false, When::Never);
    assert_eq!(kind, Ok(RuleKind::CantActivateNamed));
    assert!(kind.unwrap().of_players());
    assert_eq!(
        activate(Players::EachPlayer, true, When::Never),
        Err(Unchecked::Rule)
    );
    assert_eq!(
        activate(Players::EachPlayer, false, When::AsInstant),
        Err(Unchecked::Rule)
    );
}

/// A change to what a spell costs is asked of the permanent that makes it, so its
/// condition is kept; one said with a change of timing, or whose filter reads more than
/// a spell's own characteristics, is refused.
#[test]
fn a_cost_change_is_asked_of_its_permanent() {
    use beryl_card::SpellCost;
    let rule = RuleChange::spell_costs(Players::EachPlayer, Filter::Any, SpellCost::AtLeast(3));
    assert_eq!(RuleKind::of(&rule), Ok(RuleKind::SpellCost));
    assert!(!RuleKind::SpellCost.of_players());
    let timed = RuleChange {
        when: When::AsInstant,
        ..rule
    };
    assert_eq!(RuleKind::of(&timed), Err(Unchecked::Rule));
    let tapped = RuleChange::spell_costs(Players::You, Filter::Tapped, SpellCost::More(1));
    assert_eq!(RuleKind::of(&tapped), Err(Unchecked::Filter));
    let cast_as_usual = RuleChange {
        what: Governed::CastSpells(Filter::Any),
        ..rule
    };
    assert_eq!(RuleKind::of(&cast_as_usual), Err(Unchecked::Rule));
}

/// Spending mana as any color for activations applies to its maker's controller as a player
/// rule with `When::AsUsual`, and rejects other players or timing changes.
#[test]
fn spend_as_any_color_is_a_player_rule_for_you_as_usual() {
    let rule = RuleChange {
        players: Players::You,
        what: Governed::SpendAsAnyColor(Filter::Types(beryl_card::Types::CREATURE)),
        when: When::AsUsual,
    };
    assert_eq!(RuleKind::of(&rule), Ok(RuleKind::SpendAsAnyColor));
    assert!(RuleKind::SpendAsAnyColor.of_players());
    let opponent = RuleChange {
        players: Players::EachOpponent,
        ..rule
    };
    assert_eq!(RuleKind::of(&opponent), Err(Unchecked::Players));
    let never = RuleChange {
        when: When::Never,
        ..rule
    };
    assert_eq!(RuleKind::of(&never), Err(Unchecked::Rule));
}
