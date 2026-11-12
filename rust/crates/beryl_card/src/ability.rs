//! An ability of a face: its interface as data, and what it does.

use std::any::Any;
use std::fmt;
use std::panic::RefUnwindSafe;

use crate::characteristics::Types;
use crate::continuous::{Duration, LayerSpec, Modification, Replacement, RuleChange};
use crate::cost::{CastingKind, Cost, CostPart};
use crate::counter::CounterKind;
use crate::effect::{Amount, Comparison, Condition, CopyException, Effect, Objects, Players};
use crate::filter::Filter;
use crate::ids::TurnStep;
use crate::index::SpecIx;
use crate::mana::{ManaCost, ManaSet};
use crate::target::{Takes, TargetKind, TargetSpec};
use crate::trigger::Trigger;

/// What kind of ability this is ([CR 113.3](https://yawgatog.com/resources/magic-rules/#R1133)):
/// "There are four general categories of abilities": spell, activated, triggered and static.
/// Mana abilities ([CR 605](https://yawgatog.com/resources/magic-rules/#R605)) are activated
/// or triggered abilities with special rules, so they are a kind of their own.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AbilityKind {
    Spell,
    Activated,
    Mana,
    Triggered,
    Static,
    /// A static ability that creates a replacement effect (CR 614).
    Replacement,
}

/// When an ability may be activated or a spell cast.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Timing {
    /// Any time its controller has priority
    /// ([CR 117.1a](https://yawgatog.com/resources/magic-rules/#R1171a)): "A player may cast an
    /// instant spell any time they have priority."
    Instant,
    /// Only during its controller's main phase while the stack is empty
    /// ([CR 307.1](https://yawgatog.com/resources/magic-rules/#R3071)).
    Sorcery,
}

/// What an ability does as it resolves (plan K1): effect data, or a Rust function where the
/// vocabulary cannot say it yet.
#[derive(Clone, Copy, Debug)]
pub enum Behavior {
    /// Typed data, which the engine's interpreter runs and which tools read: what the engine,
    /// AI and UI need to know about the ability comes from here, not from a declaration
    /// beside it.
    Data(&'static Effect),
    /// The escape hatch: a Rust function only the engine can run and nothing can read.
    Custom(Custom),
}

/// A value this crate carries without knowing its type: the engine's function pointers for
/// an ability's Rust body or condition, which take engine types this crate cannot name. The
/// crate that made one reads it back with [`Custom::get`].
#[derive(Clone, Copy)]
pub struct Custom(&'static (dyn Any + Send + Sync + RefUnwindSafe));

impl Custom {
    pub const fn new<T: Any + Send + Sync + RefUnwindSafe>(value: &'static T) -> Custom {
        Custom(value)
    }

    /// The value, if it is a `T`.
    pub fn get<T: Any>(self) -> Option<&'static T> {
        let value: &'static dyn Any = self.0;
        value.downcast_ref()
    }
}

impl fmt::Debug for Custom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Custom(..)")
    }
}

/// Crew's effect (CR 702.122a): "This permanent becomes an artifact creature until end of
/// turn."
const BECOME_ARTIFACT_CREATURE: Effect = Effect::Modify {
    objects: Objects::This,
    modification: Modification::AddTypes(Types::ARTIFACT.union(Types::CREATURE)),
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

/// The most mana of one kind [`Ability::tap_for_mana`] makes.
const MOST_MANA: usize = 3;

/// "Add [mana]" for every [`ManaSet`] of up to [`MOST_MANA`] mana, by amount less one and
/// then by its kinds. An ability holds its effect by reference, and a `const fn` cannot
/// make a reference to a value it builds from an argument, so [`Ability::tap_for_mana`]
/// points here instead.
static ADD_MANA: [[Effect; 64]; MOST_MANA] = {
    let mut table = [[Effect::NOTHING; 64]; MOST_MANA];
    let mut amount = 0;
    while amount < MOST_MANA {
        let mut colors = 0;
        while colors < 64 {
            table[amount][colors] = Effect::AddMana {
                player: Players::You,
                mana: ManaSet {
                    colors: colors as u8,
                    amount: amount as u8 + 1,
                },
            };
            colors += 1;
        }
        amount += 1;
    }
    table
};

/// "Add [mana]", for the controller of the ability.
const fn add_mana(mana: ManaSet) -> &'static Effect {
    assert!(
        mana.amount >= 1 && mana.amount as usize <= MOST_MANA && mana.colors < 64,
        "more mana than this builder makes: write the ability's effect out"
    );
    &ADD_MANA[mana.amount as usize - 1][mana.colors as usize]
}

/// One ability of a face: its interface as data, and what it does.
///
/// The engine scans a face's abilities for every decision it builds, so the size is kept a
/// power of two, which makes stepping through them a shift: at 120 bytes the rollout
/// benchmark counted 0.11% more instructions than at 128. The alignment is what rounds it
/// up: the fields fill 105 bytes.
#[derive(Debug)]
#[repr(align(32))]
pub struct Ability {
    pub kind: AbilityKind,
    pub cost: Cost,
    pub timing: Timing,
    pub targets: &'static [TargetSpec],
    /// Mana abilities: what they produce.
    pub produces: ManaSet,
    /// For a Rust body: whether it may ask a player a question mid-resolution. Effect data
    /// says so itself; read [`Ability::asks`].
    pub may_pause: bool,
    pub body: Behavior,
    /// Triggered abilities: what they trigger on. Held by reference, as the body and the
    /// condition are: a [`Trigger`] names a filter and players, and an ability has 23 bytes
    /// to spare before it is 256.
    pub trigger: Option<&'static Trigger>,
    /// A static or replacement ability: its "as long as" or "while", read as often as the
    /// ability is. Triggered abilities: an intervening "if" clause, checked when the ability
    /// triggers and again as it resolves
    /// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "The ability triggers
    /// only if it is; otherwise it does nothing. If the ability triggers, it checks the
    /// stated condition again as it resolves."
    pub condition: Option<&'static Condition>,
    /// Replacement abilities: the event they modify.
    pub replacement: Replacement,
    /// Static abilities: the continuous effect they generate on the battlefield. Held by
    /// reference, as the trigger is: a [`LayerSpec`] names objects and a condition.
    pub layer: Option<&'static LayerSpec>,
    /// The rule the ability changes
    /// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): a static ability
    /// all the time, as long as its [`condition`](Ability::condition) holds, and any other
    /// by its effect, for as long as that says. A game numbers the rule changes its cards
    /// name here, so one a body starts is named here too.
    pub rule: Option<&'static RuleChange>,
    /// Mana abilities: "Activate only if you control a [permanent this filter accepts]"
    /// ([CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)): "A player can't begin
    /// to activate an ability that's prohibited from being activated." Held by reference, as
    /// the trigger is: the ability's size is a power of two with its cost's parts in it.
    pub only_if_you_control: Option<&'static Filter>,
    /// Activated abilities: what the effect needs to do anything, so that a human isn't
    /// offered an activation that would do nothing ([`Needs`]).
    pub needs: Needs,
}

/// What an activated ability's effect needs to do anything. Ability bodies are code, which
/// nothing can read, so a card says it as data: one variant for each way an effect can be
/// empty, shared by every card whose effect is empty that way.
///
/// Activating such an ability is legal whatever it would do
/// ([CR 602.2](https://yawgatog.com/resources/magic-rules/#R6022): "To activate an ability is
/// to put it onto the stack and pay its costs"), and paying its cost (tapping, say) still
/// changes the game, so full control and the exact stops offer it. No player under the smart
/// stops is offered it, search and rollouts included:
/// every line stays legal, since a player may always pass instead ([CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Needs {
    /// Nothing: the effect may do something whatever the game.
    #[default]
    Nothing,
    /// A card exiled with the ability's source, which the effect chooses among
    /// ([CR 607.2a](https://yawgatog.com/resources/magic-rules/#R6072a)): "The second ability
    /// refers only to cards in the exile zone that were put there as a result of an
    /// instruction to exile them in the first ability." With none, it does nothing, as
    /// Currency Converter's "{T}: Put a card exiled with this artifact into its owner's
    /// graveyard. ..."
    ExiledWithSource,
}

const _: () = assert!(size_of::<Ability>().is_power_of_two());

impl Ability {
    /// An ability with no cost, interface or effect, for `..Ability::NONE` in card files.
    pub const NONE: Ability = Ability {
        kind: AbilityKind::Static,
        cost: Cost::NONE,
        timing: Timing::Instant,
        targets: &[],
        produces: ManaSet::NONE,
        may_pause: false,
        body: Behavior::Data(&Effect::NOTHING),
        trigger: None,
        condition: None,
        replacement: Replacement::None,
        layer: None,
        rule: None,
        only_if_you_control: None,
        needs: Needs::Nothing,
    };

    /// A triggered ability.
    pub const fn triggered(trigger: &'static Trigger, body: Behavior) -> Ability {
        Ability {
            kind: AbilityKind::Triggered,
            trigger: Some(trigger),
            body,
            ..Ability::NONE
        }
    }

    /// A static ability generating a continuous effect.
    ///
    /// # Panics
    ///
    /// As the definition compiles, if the effect says something only a resolving spell or
    /// ability has ([`LayerSpec::needs_a_resolution`]).
    pub const fn static_ability(layer: &'static LayerSpec) -> Ability {
        assert!(
            !layer.needs_a_resolution(),
            "a static ability has no targets, no X and no instruction before it"
        );
        Ability {
            layer: Some(layer),
            ..Ability::NONE
        }
    }

    /// A static ability creating a replacement effect.
    pub const fn replacing(replacement: Replacement) -> Ability {
        Ability {
            kind: AbilityKind::Replacement,
            replacement,
            ..Ability::NONE
        }
    }

    /// Cycling ([CR 702.29a](https://yawgatog.com/resources/magic-rules/#R70229a)): "Cycling
    /// is an activated ability that functions only while the card with cycling is in a
    /// player's hand. 'Cycling [cost]' means '[Cost], Discard this card: Draw a card.'"
    pub const fn cycling(mana: ManaCost) -> Ability {
        Ability {
            kind: AbilityKind::Activated,
            cost: Cost::of(mana, &[CostPart::DISCARD]),
            body: Behavior::Data(&Effect::Draw {
                player: Players::You,
                count: Amount::Fixed(1),
            }),
            ..Ability::NONE
        }
    }

    /// Crew N (CR 702.122a), an activated ability any time its controller has priority:
    /// "This permanent becomes an artifact creature until end of turn." It keeps its other
    /// types and subtypes
    /// ([CR 205.1b](https://yawgatog.com/resources/magic-rules/#R2051b)): "Some effects state
    /// that an object becomes an 'artifact creature'; these effects also allow the object to
    /// retain all of its prior card types and subtypes."
    pub const fn crew(n: u8) -> Ability {
        Ability {
            kind: AbilityKind::Activated,
            cost: Cost::crew(n),
            body: Behavior::Data(&BECOME_ARTIFACT_CREATURE),
            ..Ability::NONE
        }
    }

    /// Offspring's triggered ability
    /// ([CR 702.175a](https://yawgatog.com/resources/magic-rules/#R702175a)): "When this
    /// permanent enters, if its offspring cost was paid, create a token that's a copy of it,
    /// except it's 1/1." The face lists [`CastingOption::Offspring`] too.
    pub const fn offspring() -> Ability {
        Ability {
            condition: Some(&Condition::Paid(CastingKind::Offspring)),
            ..Ability::triggered(
                &Trigger::ENTERS,
                Behavior::Data(&Effect::CreateCopies {
                    player: Players::You,
                    of: Objects::This,
                    count: Amount::Fixed(1),
                    except: CopyException::PowerToughness(1, 1),
                }),
            )
        }
    }

    /// Impending's static ability (CR 702.176a): "As long as this permanent's impending cost
    /// was paid and it has a time counter on it, it's not a creature". The face lists
    /// [`CastingOption::Impending`], whose counters the permanent enters with, and
    /// [`Ability::impending_countdown`].
    pub const fn impending() -> Ability {
        Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(IMPENDING),
            modification: Modification::RemoveTypes(Types::CREATURE),
        })
    }

    /// Impending's triggered ability (CR 702.176a): "At the beginning of your end step, if
    /// this permanent's impending cost was paid and it has a time counter on it, remove a
    /// time counter from it."
    pub const fn impending_countdown() -> Ability {
        Ability {
            condition: Some(&IMPENDING),
            ..Ability::triggered(
                &YOUR_END_STEP,
                Behavior::Data(&Effect::RemoveCounters {
                    objects: Objects::This,
                    kind: CounterKind::Time,
                    count: Amount::Fixed(1),
                }),
            )
        }
    }

    /// Evoke's triggered ability
    /// ([CR 702.74a](https://yawgatog.com/resources/magic-rules/#R70274a)): "When this
    /// permanent enters, if its evoke cost was paid, its controller sacrifices it." The face
    /// lists [`CastingOption::Evoke`] too.
    pub const fn evoke() -> Ability {
        Ability {
            condition: Some(&Condition::Paid(CastingKind::Evoke)),
            ..Ability::triggered(
                &Trigger::ENTERS,
                Behavior::Data(&Effect::Sacrifice(Objects::This)),
            )
        }
    }

    /// Whether resolving this may ask a player to choose: what its effect data says
    /// ([`Effect::asks`]), or for a Rust body, which nothing can read, what its card declared
    /// in [`Ability::may_pause`].
    pub const fn asks(&self) -> bool {
        match self.body {
            Behavior::Data(effect) => effect.asks(),
            Behavior::Custom(_) => self.may_pause,
        }
    }

    /// Whether this takes what is chosen for its `target`th instance of the word "target"
    /// from its controller in the way `how` names, a permanent removed from the battlefield
    /// or a spell countered: what its effect data does to a target that may be one
    /// ([`Effect::takes`]), or what its card declared with [`TargetSpec::removal`] or
    /// [`TargetSpec::counter`].
    pub fn takes_target(&self, target: SpecIx, how: Takes) -> bool {
        let Some(spec) = self.targets.get(target.index()) else {
            return false;
        };
        let may_be = match how {
            Takes::Removal => TargetKind::Permanent,
            Takes::Counter => TargetKind::Spell,
        };
        spec.takes == Some(how)
            || (spec.kind == may_be || spec.kind == TargetKind::SpellOrPermanent)
                && matches!(self.body, Behavior::Data(effect) if effect.takes(target, how))
    }

    /// Whether its effect or its intervening "if" reads what its cost took
    /// ([`Objects::Paid`]). A Rust body names nothing of the vocabulary.
    pub const fn names_paid(&self) -> bool {
        let condition = match self.condition {
            Some(condition) => condition.names_paid(),
            None => false,
        };
        condition
            || match self.body {
                Behavior::Data(effect) => effect.names_paid(),
                Behavior::Custom(_) => false,
            }
    }

    /// Whether this is a loyalty ability
    /// ([CR 606.1](https://yawgatog.com/resources/magic-rules/#R6061)).
    pub fn is_loyalty(&self) -> bool {
        self.cost.is_loyalty()
    }

    /// A mana ability with the cost {T}, such as the intrinsic ability of a basic land type
    /// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "An object with the
    /// land card type and a basic land type has the intrinsic ability '{T}: Add [mana
    /// symbol],' even if the text box doesn't actually contain that text".
    pub const fn tap_for_mana(produces: ManaSet) -> Ability {
        Ability {
            kind: AbilityKind::Mana,
            cost: Cost::TAP,
            produces,
            body: Behavior::Data(add_mana(produces)),
            ..Ability::NONE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::characteristics::Keywords;

    /// "As long as you do" is no static ability: nothing was done before it. In a card's
    /// `static` the same assertion stops the build.
    #[test]
    #[should_panic(expected = "a static ability has no targets")]
    fn a_static_ability_that_needs_a_resolution_is_refused() {
        let _ = Ability::static_ability(&LayerSpec {
            affected: Objects::This,
            condition: Some(Condition::Did),
            modification: Modification::AddKeywords(Keywords::FLYING),
        });
    }

    #[test]
    fn custom_gives_back_only_the_type_it_was_made_of() {
        static BODY: fn() -> u8 = || 7;
        let custom = Custom::new(&BODY);
        assert_eq!(custom.get::<fn() -> u8>().map(|body| body()), Some(7));
        assert!(custom.get::<u8>().is_none());
    }
}
