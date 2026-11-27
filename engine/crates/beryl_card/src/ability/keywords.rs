//! Keyword abilities that are a fixed ability of their own: cycling, crew, equip,
//! offspring, storm, cascade, impending and evoke.

use crate::characteristics::Types;
use crate::continuous::{Duration, LayerSpec, Modification};
use crate::cost::{CastingKind, Cost, CostPart};
use crate::counter::CounterKind;
use crate::effect::build::{create_copy_except, noncreature};
use crate::effect::{
    Amount, Comparison, Condition, Effect, ExileMatch, LibraryPosition, Objects, Players,
};
use crate::ids::{TurnStep, Zone, ZoneSet};
use crate::index::{ExceptIx, SpecIx};
use crate::mana::ManaCost;
use crate::target::{TargetKind, TargetSpec};
use crate::trigger::Trigger;

use super::Ability;
use crate::effect::build::nonland;
use crate::phrases::creature_you_control;

pub(crate) static CASCADE: Effect = Effect::Sequence(&[
    Effect::ExileUntil {
        player: Players::You,
        matching: ExileMatch::ManaValueBelow {
            filter: nonland(),
            below: Amount::ManaValue(Objects::This),
        },
        when_found: &Effect::MayCast {
            player: Players::You,
            card: Objects::Those,
            free: true,
            free_below: Some(Amount::ManaValue(Objects::This)),
            copy: false,
            face: crate::CastFace::Up,
            within_x: false,
        },
    },
    Effect::PutIntoLibrary {
        objects: Objects::Those,
        position: LibraryPosition::BottomRandom,
    },
]);

/// Equip's target: "target creature you control" (CR 702.6a).
const EQUIP_TARGET: [TargetSpec; 1] = [TargetSpec::one(
    TargetKind::Permanent,
    creature_you_control(),
)];

/// Equip's effect: "Attach this permanent to target creature you control" (CR 702.6a).
const ATTACH_TO_TARGET: Effect = Effect::Attach {
    objects: Objects::This,
    to: Objects::Target(SpecIx::new(0)),
};

/// Crew's effect (CR 702.122a): "This permanent becomes an artifact creature until end of turn."
const BECOME_ARTIFACT_CREATURE: Effect = Effect::Modify {
    objects: Objects::This,
    modifications: &[Modification::AddTypes(ARTIFACT_CREATURE)],
    duration: Duration::EndOfTurn,
};
const ARTIFACT_CREATURE: Types = Types::ARTIFACT.union(Types::CREATURE);

/// Prowess's trigger: "Whenever you cast a noncreature spell" (CR 702.108a).
const PROWESS_TRIGGER: Trigger = Trigger::casts(Players::You, noncreature());

/// Prowess's effect: this creature gets +1/+1 until end of turn (CR 702.108a).
pub(crate) static PROWESS_BONUS: Effect = Effect::Modify {
    objects: Objects::This,
    modifications: &[Modification::ModifyPt(1, 1)],
    duration: Duration::EndOfTurn,
};

/// "At the beginning of your end step", for [`Ability::impending_countdown`].
const YOUR_END_STEP: Trigger = Trigger::step(TurnStep::End, Players::You);

/// Impending's "if this permanent's impending cost was paid and it has a time counter on
/// it" (CR 702.176a), which its static ability and its triggered one both say.
const IMPENDING: Condition = Condition::All(&[
    Condition::Paid(CastingKind::Impending),
    Condition::Compare(
        Amount::Counters(Objects::This, CounterKind::Time),
        Comparison::AtLeast,
        Amount::Fixed(1),
    ),
]);

impl Ability {
    /// Cycling ([CR 702.29a](https://yawgatog.com/resources/magic-rules/#R70229a)): "Cycling
    /// is an activated ability that functions only while the card with cycling is in a
    /// player's hand. 'Cycling [cost]' means '[Cost], Discard this card: Draw a card.'"
    pub const fn cycling(mana: ManaCost) -> Ability {
        Ability::activated(Cost::of(mana, &[CostPart::DISCARD]))
            .in_zones(ZoneSet::of(Zone::Hand))
            .effect(&Effect::Draw {
                player: Players::You,
                count: Amount::Fixed(1),
            })
    }

    /// Crew N (CR 702.122a), an activated ability any time its controller has priority:
    /// "This permanent becomes an artifact creature until end of turn." It keeps its other
    /// types and subtypes
    /// ([CR 205.1b](https://yawgatog.com/resources/magic-rules/#R2051b)): "Some effects state
    /// that an object becomes an 'artifact creature'; these effects also allow the object to
    /// retain all of its prior card types and subtypes."
    pub const fn crew(n: u8) -> Ability {
        Ability::activated(Cost::crew(n)).effect(&BECOME_ARTIFACT_CREATURE)
    }

    /// Equip ([CR 702.6a](https://yawgatog.com/resources/magic-rules/#R7026a)): "Equip is an
    /// activated ability of Equipment cards. 'Equip [cost]' means '[Cost]: Attach this
    /// permanent to target creature you control. Activate only as a sorcery.'"
    pub const fn equip(mana: ManaCost) -> Ability {
        Ability::activated(Cost::mana(mana))
            .targeting(&EQUIP_TARGET)
            .effect(&ATTACH_TO_TARGET)
            .as_a_sorcery()
    }

    /// Offspring's triggered ability
    /// ([CR 702.175a](https://yawgatog.com/resources/magic-rules/#R702175a)): "When this
    /// permanent enters, if its offspring cost was paid, create a token that's a copy of it,
    /// except it's 1/1." The face lists [`CastingOption::Offspring`](crate::CastingOption::Offspring) too, and first among its
    /// copy exceptions ([`crate::Face::excepts`]) the 1/1 one the copy is made with.
    pub const fn offspring() -> Ability {
        const COPY: Effect = create_copy_except(Players::You, Objects::This, ExceptIx::new(0));
        Ability::triggered(&Trigger::ENTERS)
            .only_if(&Condition::Paid(CastingKind::Offspring))
            .effect(&COPY)
    }

    /// Storm ([CR 702.40a]): "Storm is a triggered ability that functions on the stack. 'Storm'
    /// means 'When you cast this spell, copy it for each other spell that was cast before it
    /// this turn. If the spell has any targets, you may choose new targets for any of the
    /// copies.'" The count is read as the ability triggers ([`Amount::SpellsCastBefore`]).
    ///
    /// [CR 702.40a]: https://yawgatog.com/resources/magic-rules/#R70240a
    pub const fn storm() -> Ability {
        const COPY: Effect = Effect::CopySpell {
            spells: Objects::This,
            count: Amount::SpellsCastBefore,
            except: None,
            new_targets: true,
        };
        Ability::triggered(&Trigger::CAST)
            .on_the_stack()
            .effect(&COPY)
    }

    /// Cascade ([CR 702.85a]): "When you cast this spell, exile cards from the top of your
    /// library until you exile a nonland card whose mana value is less than this spell’s
    /// mana value." The eligible spell can be cast for free, then the remaining cards
    /// go on the bottom at random.
    ///
    /// [CR 702.85a]: https://yawgatog.com/resources/magic-rules/#R70285a
    pub const fn cascade() -> Ability {
        Ability::triggered(&Trigger::CAST)
            .on_the_stack()
            .effect(&CASCADE)
    }

    /// Prowess ([CR 702.108a]): "Prowess is a triggered ability. 'Prowess' means 'Whenever you
    /// cast a noncreature spell, this creature gets +1/+1 until end of turn.'" Each instance
    /// triggers separately ([CR 702.108b]), which a creature with two copies of this ability
    /// gets by listing it twice.
    ///
    /// [CR 702.108a]: https://yawgatog.com/resources/magic-rules/#R702108a
    /// [CR 702.108b]: https://yawgatog.com/resources/magic-rules/#R702108b
    pub const fn prowess() -> Ability {
        Ability::triggered(&PROWESS_TRIGGER).effect(&PROWESS_BONUS)
    }

    /// Impending's static ability (CR 702.176a): "As long as this permanent's impending cost
    /// was paid and it has a time counter on it, it's not a creature". The face lists
    /// [`CastingOption::Impending`](crate::CastingOption::Impending), whose counters the permanent enters with, and
    /// [`Ability::impending_countdown`].
    pub const fn impending() -> Ability {
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(IMPENDING),
            modifications: &[Modification::RemoveTypes(Types::CREATURE)],
        })
    }

    /// Impending's triggered ability (CR 702.176a): "At the beginning of your end step, if
    /// this permanent's impending cost was paid and it has a time counter on it, remove a
    /// time counter from it."
    pub const fn impending_countdown() -> Ability {
        Ability::triggered(&YOUR_END_STEP)
            .only_if(&IMPENDING)
            .effect(&Effect::RemoveCounters {
                objects: Objects::This,
                kind: CounterKind::Time,
                count: Amount::Fixed(1),
            })
    }

    /// Evoke's triggered ability
    /// ([CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)): "When this
    /// permanent enters, if its evoke cost was paid, its controller sacrifices it." The face
    /// lists [`CastingOption::Evoke`](crate::CastingOption::Evoke) too.
    pub const fn evoke() -> Ability {
        Ability::triggered(&Trigger::ENTERS)
            .only_if(&Condition::Paid(CastingKind::Evoke))
            .effect(&Effect::Sacrifice(Objects::This))
    }
}
