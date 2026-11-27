//! Changes to the rules of the game rather than to objects' characteristics ([`RuleChange`]).

use crate::characteristics::Types;
use crate::effect::{Objects, Players};
use crate::filter::Filter;
use crate::mana::ManaCost;

/// A change to a rule of the game rather than to an object's characteristics
/// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): "Some continuous
/// effects affect game rules rather than objects." A static ability makes one all the time
/// ([CR 604.1](https://yawgatog.com/resources/magic-rules/#R6041): "Static abilities do
/// something all the time rather than being activated or triggered."), a resolved spell or
/// ability for as long as it says.
///
/// It says who may or may not do what, and when: "Each opponent can cast spells only any
/// time they could cast a sorcery." is each opponent, casting spells, only as a sorcery. A
/// static ability's "as long as" is its [`condition`](crate::Ability::condition), and how
/// long a resolved one lasts is its effect's duration.
#[derive(Clone, Copy, Debug)]
pub struct RuleChange {
    /// Who it applies to, read for the controller of the ability that makes it: of a static
    /// ability, whoever that is now, and of a resolving one, as it resolves.
    pub players: Players,
    /// What they do that it governs.
    pub what: Governed,
    /// When they may do it, or may only.
    pub when: When,
}

/// What a [`RuleChange`] governs the timing of.
#[derive(Clone, Copy, Debug)]
pub enum Governed {
    /// Casting the spells a filter accepts, read of each spell's own characteristics.
    CastSpells(Filter),
    /// Taking a named step. With [`When::Never`], each affected player skips that step.
    TakeStep(crate::TurnStep),
    /// Activating the loyalty abilities of these permanents.
    ActivateLoyaltyAbilities(Objects),
    /// Blocking with these creatures, as "This creature can't block." says
    /// ([CR 509.1a]): "The defending player chooses which creatures they control, if any,
    /// will block." The players are those who would declare the block: "you", the
    /// creatures' controller.
    ///
    /// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
    Block(Objects),
    /// Untapping these permanents during their controller's untap step.
    UntapInUntapStep(Objects),
    /// The creature may block any number of attackers. This changes the limit of one in
    /// [CR 509.1a], which says "For each of the chosen creatures, the defending player chooses one
    /// creature for it to block". Use with [`When::AsUsual`] and read it during declaration.
    ///
    /// [CR 509.1a]: https://yawgatog.com/resources/magic-rules/#R5091a
    BlockAnyNumber(Objects),
    /// Attacking with these creatures, as "This creature can't attack." says, or with
    /// `unless_defender_controls`, "This creature can't attack unless defending player
    /// controls an Island." ([CR 508.1c]): "The active player checks each creature they
    /// control to see whether it's affected by any restrictions (effects that say a creature
    /// can't attack, or that it can't attack unless some condition is met)." The filter is
    /// the permanents the defending player must control one of, read for each player the
    /// creature could attack, through the words of the text of the permanent with the
    /// ability: when a text change makes Dandân's "Island" read as "Swamp", it asks for a
    /// Swamp. The players are those who would declare the attack: "you", the creatures'
    /// controller.
    ///
    /// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
    Attack {
        attackers: Objects,
        unless_defender_controls: Option<Filter>,
    },
    /// Activating the activated abilities of the sources these name, as "Activated
    /// abilities of sources with the chosen name can't be activated unless they're mana
    /// abilities." says ([CR 602.5]): "A player can't begin to activate an ability that's
    /// prohibited from being activated." With `mana_abilities` false, mana abilities are
    /// the exception.
    ///
    /// [CR 602.5]: https://yawgatog.com/resources/magic-rules/#R6025
    ActivateAbilities {
        sources: Named,
        mana_abilities: bool,
    },
    /// Countering these spells ([CR 701.6a]): "To counter a spell or ability means to cancel
    /// it, removing it from the stack." With [`When::Never`], "that spell can't be
    /// countered": the players are those whose spells or abilities would counter it, each
    /// player. A spell or ability may still target the spell, and does nothing to it.
    ///
    /// [CR 701.6a]: https://yawgatog.com/resources/magic-rules/#R7016a
    Counter(Objects),
    /// Preventing damage. With `When::Never` for each player, "Damage can't be prevented"
    /// (see [CR 615.12]). This also stops protection from preventing damage. Protection's
    /// targeting restrictions still apply (see [CR 702.16b]).
    ///
    /// [CR 615.12]: https://yawgatog.com/resources/magic-rules/#R61512
    /// [CR 702.16b]: https://yawgatog.com/resources/magic-rules/#R70216b
    PreventDamage,
    /// What casting the spells a filter accepts costs, read of each spell's own
    /// characteristics, as "Noncreature spells cost {1} more to cast." says. It changes the
    /// total cost ([CR 601.2f]): "The total cost is the mana cost or alternative cost (as
    /// determined in rule 601.2b), plus all additional costs and cost increases, and minus
    /// all cost reductions." Made with [`When::AsUsual`]: the spells are cast when they
    /// always may be.
    ///
    /// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
    SpellCosts { spells: Filter, change: SpellCost },
    /// The players themselves: what [`PlayerRules`] names, as "your life total can't change and
    /// you gain protection from everything" says. Made with [`When::AsUsual`]: the change is to
    /// the players, not to when they do something.
    Player(PlayerRules),
    /// Spending mana as though it were mana of any color to activate abilities of the
    /// permanents a filter accepts, as "You may spend mana as though it were mana of any color
    /// to activate abilities of creatures you control." says. Made with [`When::AsUsual`]:
    /// colored and hybrid symbols in those activation costs may be paid with mana of any type,
    /// while `{C}` symbols still require colorless mana (see [CR 107.4c]).
    ///
    /// [CR 107.4c]: https://yawgatog.com/resources/magic-rules/#R1074c
    SpendAsAnyColor(Filter),
    /// "All creatures able to block [objects] do so.": each creature able to block one of
    /// these attackers has, for each of them, a requirement to block it ([CR 509.1c]): "The
    /// defending player checks each creature they control to see whether it's affected by
    /// any requirements (effects that say a creature must block, or that it must block if
    /// some condition is met)." Made with [`When::AsUsual`]. Of the creature with the
    /// ability ([`Objects::This`]) or the one its Aura or Equipment is attached to
    /// ([`Objects::AttachedTo`]).
    ///
    /// [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
    MustBlock(Objects),
    /// "[Objects] must be blocked if able": one requirement for each of these attackers,
    /// obeyed when one or more creatures block it ([CR 509.1c]): "If the number of
    /// requirements that are being obeyed is fewer than the maximum possible number of
    /// requirements that could be obeyed without disobeying any restrictions, the
    /// declaration of blockers is illegal." Made with [`When::AsUsual`], usually by an effect
    /// about its target for a turn.
    ///
    /// [CR 509.1c]: https://yawgatog.com/resources/magic-rules/#R5091c
    MustBeBlocked(Objects),
    /// "[Objects] attacks each combat if able": a requirement of each of these creatures
    /// ([CR 508.1d]): "The active player checks each creature they control to see whether
    /// it's affected by any requirements (effects that say a creature attacks if able, or
    /// that it attacks if some condition is met)." Made with [`When::AsUsual`].
    ///
    /// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
    MustAttack(Objects),
    /// "[Creatures] can't attack or block alone": a restriction on each of these creatures
    /// as an attacker if `attack`, and as a blocker if `block`, broken when it is the only
    /// creature declared ([CR 508.1c]): "The active player checks each creature they control
    /// to see whether it's affected by any restrictions (effects that say a creature can't
    /// attack, or that it can't attack unless some condition is met)." And blocking
    /// ([CR 509.1b]): "If any restrictions are being disobeyed, the declaration of blockers
    /// is illegal." Made with [`When::AsUsual`].
    ///
    /// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
    /// [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
    Alone {
        attack: bool,
        block: bool,
        creatures: Objects,
    },
    /// "No more than [n] creatures can attack each combat", and the same of blocking: a
    /// restriction on the declaration as a whole ([CR 508.1c], [CR 509.1b]), at most
    /// `attackers` attackers and at most `blockers` blockers where each is given. Made with
    /// [`When::AsUsual`] for each player.
    ///
    /// [CR 508.1c]: https://yawgatog.com/resources/magic-rules/#R5081c
    /// [CR 509.1b]: https://yawgatog.com/resources/magic-rules/#R5091b
    CombatLimit {
        attackers: Option<u8>,
        blockers: Option<u8>,
    },
    /// "Creatures can't attack you unless their controller pays [cost] for each creature they
    /// control that's attacking you": a cost to attack the players it applies to, paid once
    /// for each creature attacking them ([CR 508.1h]): "If any of the chosen creatures require
    /// paying costs to attack, or if any optional costs to attack were chosen, the active
    /// player determines the total cost to attack." A requirement to attack that only paying
    /// it could obey need not be ([CR 508.1d]): "If a creature can't attack unless a player
    /// pays a cost, that player is not required to pay that cost". Made with
    /// [`When::AsUsual`] for "you".
    ///
    /// [CR 508.1h]: https://yawgatog.com/resources/magic-rules/#R5081h
    /// [CR 508.1d]: https://yawgatog.com/resources/magic-rules/#R5081d
    AttackCost(ManaCost),
}

impl Governed {
    /// The objects this is about, if it names some: "this creature" of "This creature can't
    /// block.", "target creature" of "Target creature must be blocked this turn if able."
    pub const fn about(&self) -> Option<Objects> {
        match *self {
            Governed::ActivateLoyaltyAbilities(objects)
            | Governed::Block(objects)
            | Governed::UntapInUntapStep(objects)
            | Governed::BlockAnyNumber(objects)
            | Governed::Attack {
                attackers: objects, ..
            }
            | Governed::Counter(objects)
            | Governed::MustBlock(objects)
            | Governed::MustBeBlocked(objects)
            | Governed::MustAttack(objects)
            | Governed::Alone {
                creatures: objects, ..
            } => Some(objects),
            Governed::CastSpells(_)
            | Governed::TakeStep(_)
            | Governed::ActivateAbilities { .. }
            | Governed::PreventDamage
            | Governed::SpellCosts { .. }
            | Governed::Player(_)
            | Governed::CombatLimit { .. }
            | Governed::AttackCost(_)
            | Governed::SpendAsAnyColor(_) => None,
        }
    }
}

/// How a [`Governed::SpellCosts`] changes a spell's total cost ([CR 601.2f]).
///
/// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SpellCost {
    /// "Cost {N} more to cast": a cost increase of N generic mana.
    More(u8),
    /// "Cost {N} less to cast": a cost reduction, which reduces only generic mana and never
    /// below none ([CR 601.2f]): "If the mana component of the total cost is reduced to
    /// nothing by cost reduction effects, it is considered to be {0}. It can't be reduced to
    /// less than {0}."
    ///
    /// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
    Less(u8),
    /// "Each spell that would cost less than N mana to cast costs N mana to cast": a floor on
    /// the total mana, X counted at its value, applied after every increase and reduction,
    /// as an effect that directly affects the total cost ([CR 601.2f]): "Once the total
    /// cost is determined, any effects that directly affect the total cost are applied."
    /// The difference is generic mana, which any type of mana pays.
    ///
    /// [CR 601.2f]: https://yawgatog.com/resources/magic-rules/#R6012f
    AtLeast(u8),
}

/// What a [`Governed::Player`] changes about the players it applies to, one bit for each.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct PlayerRules(u8);

impl PlayerRules {
    /// "Your life total can't change": the players can't gain life ([CR 119.7]) and can't lose
    /// life ([CR 119.8]): "If an effect says that a player can't lose life, that player can't
    /// make an exchange such that the player's life total would become lower ... In addition, a
    /// cost that involves having that player pay life can't be paid." Damage is still dealt to
    /// them.
    ///
    /// [CR 119.7]: https://yawgatog.com/resources/magic-rules/#R1197
    /// [CR 119.8]: https://yawgatog.com/resources/magic-rules/#R1198
    pub const LIFE_CANT_CHANGE: PlayerRules = PlayerRules(1 << 0);
    /// "You gain protection from everything" ([CR 702.16j]): "A permanent or player with
    /// protection from everything has protection from each object regardless of that object's
    /// characteristic values. Such a permanent or player can't be targeted by spells or
    /// abilities and can't be enchanted by Auras. ... All damage that would be dealt to such a
    /// permanent or player is prevented."
    ///
    /// [CR 702.16j]: https://yawgatog.com/resources/magic-rules/#R70216j
    pub const PROTECTION_FROM_EVERYTHING: PlayerRules = PlayerRules(1 << 1);

    /// "You have no maximum hand size" (see CR 514.1).
    pub const NO_MAXIMUM_HAND_SIZE: PlayerRules = PlayerRules(1 << 2);

    /// Both sets' changes.
    #[must_use]
    pub const fn union(self, other: PlayerRules) -> PlayerRules {
        PlayerRules(self.0 | other.0)
    }

    /// Whether it makes every change `other` makes.
    pub const fn contains(self, other: PlayerRules) -> bool {
        self.0 & other.0 == other.0
    }
}

/// The sources a [`Governed::ActivateAbilities`] names, by their names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Named {
    /// Those with the card name the permanent with the ability chose as it entered
    /// ([`Replacement::choose_as_enters`]), read from each source's copiable values: a copy
    /// of a card of that name has it, and a face-down permanent has no name
    /// ([CR 708.2a](https://yawgatog.com/resources/magic-rules/#R7082a)): it "becomes a 2/2
    /// face-down creature with no text, no name, no subtypes, and no mana cost."
    Chosen,
}

/// When a [`RuleChange`] has what it governs done.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum When {
    /// "Only any time they could cast a sorcery": a restriction, which beats a permission
    /// ([CR 101.2](https://yawgatog.com/resources/magic-rules/#R1012)): "When a rule or
    /// effect allows or directs something to happen, and another effect states that it
    /// can't happen, the 'can't' effect takes precedence."
    OnlyAsSorcery,
    /// "Any time they could cast an instant", "as though it had flash"
    /// ([CR 702.8a](https://yawgatog.com/resources/magic-rules/#R7028a)): "'Flash' means
    /// 'You may play this card any time you could cast an instant.'" A permission.
    AsInstant,
    /// "Can't": never, a restriction that beats any permission (CR 101.2), as "This
    /// creature can't block." is.
    Never,
    /// When the rules already have it done: the change is to something else about it, as a
    /// [`Governed::SpellCosts`] is to what a spell costs.
    AsUsual,
}

impl RuleChange {
    /// "[Spells] [players] cast cost ...": a change to what casting the spells `spells`
    /// accepts costs `players` ([`Governed::SpellCosts`]).
    pub const fn spell_costs(players: Players, spells: Filter, change: SpellCost) -> RuleChange {
        RuleChange {
            players,
            what: Governed::SpellCosts { spells, change },
            when: When::AsUsual,
        }
    }

    /// The change in words, said of who it applies to, as in "player 1 casting sorceries
    /// as though they had flash": how a description of a game names it, as a save's record
    /// of what a checkpoint would lose does.
    pub fn words(&self) -> String {
        match (self.what, self.when) {
            (Governed::CastSpells(filter), When::OnlyAsSorcery) => {
                format!(
                    "casting {} only when they could cast a sorcery",
                    spells(filter)
                )
            }
            (Governed::CastSpells(filter), When::AsInstant) => {
                format!("casting {} as though they had flash", spells(filter))
            }
            (Governed::ActivateLoyaltyAbilities(_), When::OnlyAsSorcery) => {
                "with loyalty abilities to activate only as a sorcery".to_owned()
            }
            (Governed::ActivateLoyaltyAbilities(_), When::AsInstant) => {
                "with loyalty abilities to activate at instant speed".to_owned()
            }
            (Governed::CastSpells(filter), When::Never) => {
                format!("unable to cast {}", spells(filter))
            }
            (Governed::ActivateLoyaltyAbilities(_), When::Never) => {
                "with loyalty abilities that can't be activated".to_owned()
            }
            (Governed::UntapInUntapStep(_), _) => {
                "with permanents that don't untap during their untap step".to_owned()
            }
            (Governed::BlockAnyNumber(_), _) => {
                "with creatures that can block any number of creatures".to_owned()
            }
            (Governed::Block(_), When::Never) => "with creatures that can't block".to_owned(),
            (Governed::Block(_), When::OnlyAsSorcery | When::AsInstant) => {
                "with creatures that block at a changed time".to_owned()
            }
            (Governed::Attack { .. }, When::Never) => {
                "with creatures that can't attack, or only some players".to_owned()
            }
            (Governed::Attack { .. }, When::OnlyAsSorcery | When::AsInstant) => {
                "with creatures that attack at a changed time".to_owned()
            }
            (Governed::ActivateAbilities { .. }, When::Never) => {
                "unable to activate the abilities of sources with a chosen name".to_owned()
            }
            (Governed::ActivateAbilities { .. }, When::OnlyAsSorcery | When::AsInstant) => {
                "with abilities of named sources to activate at a changed time".to_owned()
            }
            (Governed::Counter(_), When::Never) => {
                "with a spell that can't be countered".to_owned()
            }
            (Governed::Counter(_), When::OnlyAsSorcery | When::AsInstant) => {
                "with a spell countered at a changed time".to_owned()
            }
            (
                Governed::SpellCosts {
                    spells: filter,
                    change,
                },
                _,
            ) => {
                let spells = spells(filter);
                match change {
                    SpellCost::More(n) => format!("paying {n} more for {spells}"),
                    SpellCost::Less(n) => format!("paying {n} less for {spells}"),
                    SpellCost::AtLeast(n) => format!("paying at least {n} for {spells}"),
                }
            }
            (Governed::PreventDamage, _) => "unable to prevent damage".to_owned(),
            (Governed::Player(changes), _) => {
                if changes == PlayerRules::NO_MAXIMUM_HAND_SIZE {
                    return "with no maximum hand size".to_owned();
                }
                let life = changes.contains(PlayerRules::LIFE_CANT_CHANGE);
                let protection = changes.contains(PlayerRules::PROTECTION_FROM_EVERYTHING);
                match (life, protection) {
                    (true, true) => {
                        "with a life total that can't change and protection from everything"
                    }
                    (true, false) => "with a life total that can't change",
                    (false, true) => "with protection from everything",
                    (false, false) => "with no change to themselves",
                }
                .to_owned()
            }
            (
                what @ (Governed::MustBlock(_)
                | Governed::MustBeBlocked(_)
                | Governed::MustAttack(_)
                | Governed::Alone { .. }
                | Governed::CombatLimit { .. }
                | Governed::AttackCost(_)),
                _,
            ) => combat_words(what),
            (Governed::TakeStep(step), When::Never) => format!("skipping their {step:?} steps"),
            (Governed::TakeStep(step), When::OnlyAsSorcery | When::AsInstant) => {
                format!("taking their {step:?} steps at a changed time")
            }
            (Governed::SpendAsAnyColor(_), _) => {
                "spending mana as though it were mana of any color to activate some abilities"
                    .to_owned()
            }
            (_, When::AsUsual) => "with a rule changed in no way".to_owned(),
        }
    }
}

/// [`RuleChange::words`] of a combat requirement, restriction or cost to attack.
fn combat_words(what: Governed) -> String {
    match what {
        Governed::MustBlock(_) => {
            "with creatures that all creatures able to block must block".to_owned()
        }
        Governed::MustBeBlocked(_) => "with creatures that must be blocked if able".to_owned(),
        Governed::MustAttack(_) => "with creatures that attack each combat if able".to_owned(),
        Governed::Alone { .. } => "with creatures that can't attack or block alone".to_owned(),
        Governed::CombatLimit {
            attackers,
            blockers,
        } => {
            let limit = |n: Option<u8>, verb| n.map(|n| format!("at most {n} {verb}")).into_iter();
            let limits: Vec<String> = limit(attackers, "attacking")
                .chain(limit(blockers, "blocking"))
                .collect();
            format!("with {} creatures each combat", limits.join(" and "))
        }
        Governed::AttackCost(cost) => {
            format!("attacked only by creatures whose controller pays {cost:?} for each")
        }
        Governed::CastSpells(_)
        | Governed::TakeStep(_)
        | Governed::ActivateLoyaltyAbilities(_)
        | Governed::Block(_)
        | Governed::UntapInUntapStep(_)
        | Governed::BlockAnyNumber(_)
        | Governed::Attack { .. }
        | Governed::ActivateAbilities { .. }
        | Governed::Counter(_)
        | Governed::PreventDamage
        | Governed::SpellCosts { .. }
        | Governed::Player(_)
        | Governed::SpendAsAnyColor(_) => unreachable!("a combat rule"),
    }
}

/// The spells a filter accepts, in a word or two.
fn spells(filter: Filter) -> &'static str {
    match filter {
        Filter::Any => "spells",
        Filter::Types(types) if types == Types::SORCERY => "sorceries",
        Filter::Types(types) if types == Types::INSTANT => "instants",
        Filter::Types(types) if types == Types::CREATURE => "creature spells",
        Filter::Types(_)
        | Filter::Supertypes(_)
        | Filter::SubtypeWord(_)
        | Filter::ColorWords(_)
        | Filter::Whose(_)
        | Filter::Owned(_)
        | Filter::AttackedThisTurn
        | Filter::WithCounter(_)
        | Filter::Tapped
        | Filter::Token
        | Filter::Named(_)
        | Filter::ManaValueAtMost(_)
        | Filter::PowerAtMost(_)
        | Filter::ManaValueAtMostX
        | Filter::ManaValueAtMostPower
        | Filter::Not(_)
        | Filter::All(_)
        | Filter::AnyOf(_)
        | Filter::InZone(_) => "some spells",
    }
}
