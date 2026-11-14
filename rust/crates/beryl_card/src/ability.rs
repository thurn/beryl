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
use crate::ids::{TurnStep, ZoneSet};
use crate::index::SpecIx;
use crate::mana::{ManaCost, ManaSet};
use crate::target::{Takes, TargetKind, TargetSpec};
use crate::trigger::Trigger;

mod zones;

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

/// What an ability does as it resolves: effect data, or a Rust function where the
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

/// An ability's Rust body: a value this crate carries without knowing its type, the engine's
/// function pointers, which take engine types this crate cannot name, and what the body's
/// registration says of it, since nothing can read code. The crate that made one reads the
/// value back with [`Custom::get`].
#[derive(Clone, Copy)]
pub struct Custom {
    body: &'static (dyn Any + Send + Sync + RefUnwindSafe),
    says: &'static Says,
}

/// What a Rust body's registration says of it, which effect data says of itself.
#[derive(Clone, Copy, Debug)]
pub struct Says {
    /// Whether it may ask a player a question mid-resolution ([`Ability::asks`]).
    pub asks: bool,
    /// How it takes what is chosen for each instance of the word "target" from its
    /// controller, in the order of the ability's targets, with none for one it leaves alone
    /// and for any past the list's end ([`Ability::takes_target`]).
    pub takes: &'static [Option<Takes>],
}

impl Says {
    /// A body that asks nothing and takes no target.
    pub const NOTHING: Says = Says {
        asks: false,
        takes: &[],
    };
}

impl Custom {
    pub const fn new<T: Any + Send + Sync + RefUnwindSafe>(
        body: &'static T,
        says: &'static Says,
    ) -> Custom {
        Custom { body, says }
    }

    /// The value, if it is a `T`.
    pub fn get<T: Any>(self) -> Option<&'static T> {
        let body: &'static dyn Any = self.body;
        body.downcast_ref()
    }

    /// What its registration says of it.
    pub const fn says(self) -> &'static Says {
        self.says
    }
}

impl fmt::Debug for Custom {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Custom(..)")
    }
}

/// Equip's target: "target creature you control" (CR 702.6a).
const EQUIP_TARGET: [TargetSpec; 1] = [TargetSpec::one(
    TargetKind::Permanent,
    Filter::All(&[Filter::Types(Types::CREATURE), Filter::Whose(&Players::You)]),
)];

/// Equip's effect: "Attach this permanent to target creature you control" (CR 702.6a).
const ATTACH_TO_TARGET: Effect = Effect::Attach {
    objects: Objects::This,
    to: Objects::Target(SpecIx::new(0)),
};

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
/// Its fields are private: an ability is made by the builder for its kind and the methods
/// that say more of it, each of which refuses, as the definition compiles, what its kind does
/// not have. So a static ability has no trigger and no targets, and what a mana ability
/// produces is what its effect adds.
///
/// ```
/// use beryl_card::prelude::*;
///
/// // "Return target spell to its owner's hand. Draw a card."
/// const REPRIEVE: Ability = Ability::spell()
///     .target(&TargetSpec::spell())
///     .effect(&sequence(&[return_to_hand(target()), draw(you(), 1)]));
/// ```
///
/// The engine scans a face's abilities for every decision it builds, so the size is kept a
/// power of two, which makes stepping through them a shift: at 120 bytes the rollout
/// benchmark counted 0.11% more instructions than at 128. The alignment is what rounds it
/// up: the fields fill 106 bytes.
#[derive(Clone, Copy, Debug)]
#[repr(align(32))]
pub struct Ability {
    kind: AbilityKind,
    cost: Cost,
    timing: Timing,
    targets: &'static [TargetSpec],
    /// Mana abilities: what they produce.
    produces: ManaSet,
    /// Whether its effect data needs something to do anything ([`Ability::needs`]), settled
    /// as the effect is given: the engine asks of every activated ability at every priority.
    needy: bool,
    body: Behavior,
    /// Triggered abilities: what they trigger on. Held by reference, as the body and the
    /// condition are: a [`Trigger`] names a filter and players.
    trigger: Option<&'static Trigger>,
    /// A static or replacement ability: its "as long as" or "while", read as often as the
    /// ability is. Triggered abilities: an intervening "if" clause, checked when the ability
    /// triggers and again as it resolves
    /// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "The ability triggers
    /// only if it is; otherwise it does nothing. If the ability triggers, it checks the
    /// stated condition again as it resolves."
    condition: Option<&'static Condition>,
    /// Replacement abilities: the event they modify.
    replacement: Replacement,
    /// Static abilities: the continuous effect they generate on the battlefield. Held by
    /// reference, as the trigger is: a [`LayerSpec`] names objects and a condition.
    layer: Option<&'static LayerSpec>,
    /// The rule the ability changes
    /// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): a static ability
    /// all the time, as long as its [`condition`](Ability::condition) holds, and any other
    /// by its effect, for as long as that says. A game numbers the rule changes its cards
    /// name here, so one a body starts is named here too.
    rule: Option<&'static RuleChange>,
    /// Mana abilities: "Activate only if you control a [permanent this filter accepts]"
    /// ([CR 602.5](https://yawgatog.com/resources/magic-rules/#R6025)): "A player can't begin
    /// to activate an ability that's prohibited from being activated." Held by reference, as
    /// the trigger is: the ability's size is a power of two with its cost's parts in it.
    only_if_you_control: Option<&'static Filter>,
    /// Whether its card's rules text says it in the paragraph of the ability before it, as
    /// "Whenever this creature enters or attacks" says the two triggered abilities a face
    /// lists for it, so that a view finds its words. The rules make a paragraph one ability
    /// ([CR 113.2c](https://yawgatog.com/resources/magic-rules/#R1132c): "each paragraph
    /// break in a card's text marks a separate ability"); a face lists one ability for each
    /// event it triggers on. Read [`Ability::shares_paragraph`].
    shares_paragraph: bool,
    /// The zones it works in: its kind's unless its card says otherwise ([`zones`]).
    zones: ZoneSet,
}

const _: () = assert!(size_of::<Ability>().is_power_of_two());

/// The builders: one for each kind of ability.
impl Ability {
    /// An ability of `kind` with nothing else said of it.
    const fn of(kind: AbilityKind) -> Ability {
        Ability {
            kind,
            cost: Cost::NONE,
            timing: Timing::Instant,
            targets: &[],
            produces: ManaSet::NONE,
            needy: false,
            body: Behavior::Data(&Effect::NOTHING),
            trigger: None,
            condition: None,
            replacement: Replacement::None,
            layer: None,
            rule: None,
            only_if_you_control: None,
            shares_paragraph: false,
            zones: zones::usual_zones(kind),
        }
    }

    /// The spell ability of an instant or sorcery
    /// ([CR 113.3a](https://yawgatog.com/resources/magic-rules/#R1133a)): "Spell abilities
    /// are abilities that are followed as instructions while an instant or sorcery spell is
    /// resolving." Of an Aura spell, what it targets, with no effect
    /// ([CR 303.4a](https://yawgatog.com/resources/magic-rules/#R3034a)): "An Aura spell
    /// requires a target, which is defined by its enchant ability."
    pub const fn spell() -> Ability {
        Ability::of(AbilityKind::Spell)
    }

    /// An activated ability
    /// ([CR 602.1](https://yawgatog.com/resources/magic-rules/#R6021)): "Activated abilities
    /// have a cost and an effect. They are written as '[Cost]: [Effect.] [Activation
    /// instructions (if any).]'"
    pub const fn activated(cost: Cost) -> Ability {
        Ability {
            cost,
            ..Ability::of(AbilityKind::Activated)
        }
    }

    /// A loyalty ability, which adds or removes `n` loyalty counters as its cost
    /// ([CR 606.1](https://yawgatog.com/resources/magic-rules/#R6061)).
    pub const fn loyalty(n: i8) -> Ability {
        Ability::activated(Cost::loyalty(n))
    }

    /// A mana ability
    /// ([CR 605.1a](https://yawgatog.com/resources/magic-rules/#R6051a)): an activated
    /// ability that "could add mana to a player's mana pool when it resolves". Its effect is
    /// "Add [produces]", so what it is known to produce and what it adds cannot disagree.
    pub const fn mana(cost: Cost, produces: ManaSet) -> Ability {
        Ability {
            cost,
            produces,
            body: Behavior::Data(add_mana(produces)),
            ..Ability::of(AbilityKind::Mana)
        }
    }

    /// A mana ability with the cost {T}, such as the intrinsic ability of a basic land type
    /// ([CR 305.6](https://yawgatog.com/resources/magic-rules/#R3056)): "An object with the
    /// land card type and a basic land type has the intrinsic ability '{T}: Add [mana
    /// symbol],' even if the text box doesn't actually contain that text".
    pub const fn tap_for_mana(produces: ManaSet) -> Ability {
        Ability::mana(Cost::TAP, produces)
    }

    /// A triggered ability
    /// ([CR 603.1](https://yawgatog.com/resources/magic-rules/#R6031)): "Triggered abilities
    /// have a trigger condition and an effect."
    pub const fn triggered(trigger: &'static Trigger) -> Ability {
        Ability {
            trigger: Some(trigger),
            ..Ability::of(AbilityKind::Triggered)
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
            ..Ability::of(AbilityKind::Static)
        }
    }

    /// A static ability changing a rule (CR 613.11), all the time or
    /// [only if](Ability::only_if) a condition holds.
    pub const fn rule_change(rule: &'static RuleChange) -> Ability {
        Ability {
            rule: Some(rule),
            ..Ability::of(AbilityKind::Static)
        }
    }

    /// A static ability creating a replacement effect.
    pub const fn replacing(replacement: Replacement) -> Ability {
        Ability {
            replacement,
            ..Ability::of(AbilityKind::Replacement)
        }
    }

    /// Cycling ([CR 702.29a](https://yawgatog.com/resources/magic-rules/#R70229a)): "Cycling
    /// is an activated ability that functions only while the card with cycling is in a
    /// player's hand. 'Cycling [cost]' means '[Cost], Discard this card: Draw a card.'"
    pub const fn cycling(mana: ManaCost) -> Ability {
        Ability::activated(Cost::of(mana, &[CostPart::DISCARD])).effect(&Effect::Draw {
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
    /// except it's 1/1." The face lists [`CastingOption::Offspring`] too.
    pub const fn offspring() -> Ability {
        Ability::triggered(&Trigger::ENTERS)
            .only_if(&Condition::Paid(CastingKind::Offspring))
            .effect(&Effect::CreateCopies {
                player: Players::You,
                of: Objects::This,
                count: Amount::Fixed(1),
                except: CopyException::PowerToughness(1, 1),
            })
    }

    /// Impending's static ability (CR 702.176a): "As long as this permanent's impending cost
    /// was paid and it has a time counter on it, it's not a creature". The face lists
    /// [`CastingOption::Impending`], whose counters the permanent enters with, and
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
    /// lists [`CastingOption::Evoke`] too.
    pub const fn evoke() -> Ability {
        Ability::triggered(&Trigger::ENTERS)
            .only_if(&Condition::Paid(CastingKind::Evoke))
            .effect(&Effect::Sacrifice(Objects::This))
    }
}

/// What more a card says of an ability. Each method refuses, as the definition compiles, an
/// ability of a kind that does not have what it says.
impl Ability {
    /// Whether this goes on the stack and resolves: a spell, activated or triggered ability
    /// ([CR 113.3](https://yawgatog.com/resources/magic-rules/#R1133)).
    const fn resolves(&self) -> bool {
        matches!(
            self.kind,
            AbilityKind::Spell | AbilityKind::Activated | AbilityKind::Triggered
        )
    }

    /// Its targets, one [`TargetSpec`] for each instance of the word "target"
    /// ([CR 115.1](https://yawgatog.com/resources/magic-rules/#R1151)). A mana ability has
    /// none (CR 605.1a), and neither has a static ability, which is never on the stack.
    #[must_use]
    pub const fn targeting(self, targets: &'static [TargetSpec]) -> Ability {
        assert!(
            self.resolves(),
            "only a spell, activated or triggered ability has targets"
        );
        Ability { targets, ..self }
    }

    /// Its one target: [`Ability::targeting`] with one spec, as
    /// `.target(&TargetSpec::creature())`.
    #[must_use]
    pub const fn target(self, target: &'static TargetSpec) -> Ability {
        self.targeting(std::slice::from_ref(target))
    }

    /// What it does as it resolves, as effect data. Of a replacement ability, what happens
    /// instead of the event it replaces. A rule change the effect starts
    /// ([`Effect::ChangeRule`]) becomes the ability's ([`Ability::rule`]).
    #[must_use]
    pub const fn effect(self, effect: &'static Effect) -> Ability {
        assert!(
            self.resolves() || matches!(self.kind, AbilityKind::Replacement),
            "a static ability has no effect to carry out, and a mana ability's is what it \
             produces"
        );
        // The rule change its effect starts is the one the game numbers for it.
        let rule = match effect.starts() {
            Some(rule) => Some(rule),
            None => self.rule,
        };
        let needy = match effect.needs() {
            Some(objects) => !objects.needs_a_resolution(),
            None => false,
        };
        Ability {
            body: Behavior::Data(effect),
            rule,
            needy,
            ..self
        }
    }

    /// What it does as it resolves, as a Rust body, where the vocabulary cannot say it: the
    /// engine's `custom_body` of the card file's type, whose registration says what data says
    /// of itself ([`Says`]).
    #[must_use]
    pub const fn custom(self, body: Behavior) -> Ability {
        assert!(
            !matches!(self.kind, AbilityKind::Static),
            "a static ability has no effect to carry out"
        );
        assert!(
            matches!(body, Behavior::Custom(_)),
            "effect data is given with `effect`"
        );
        Ability { body, ..self }
    }

    /// Declares that its card's rules text says it in the paragraph of the ability before it
    /// in the face's list, as "Whenever this creature enters or attacks" says an enters
    /// trigger and an attacks trigger.
    #[must_use]
    pub const fn in_paragraph_before(self) -> Ability {
        Ability {
            shares_paragraph: true,
            ..self
        }
    }

    /// A triggered ability's intervening "if" (CR 603.4), or the "as long as" of a static
    /// ability that changes a rule or of a replacement effect. A static ability generating a
    /// continuous effect says its own in its [`LayerSpec`].
    #[must_use]
    pub const fn only_if(self, condition: &'static Condition) -> Ability {
        assert!(
            matches!(self.kind, AbilityKind::Triggered | AbilityKind::Replacement)
                || matches!(self.kind, AbilityKind::Static) && self.rule.is_some(),
            "only a triggered ability, a rule change or a replacement has a condition"
        );
        Ability {
            condition: Some(condition),
            ..self
        }
    }

    /// The rule its effect changes for a while (CR 613.11), as "Until your next turn, your
    /// opponents can cast spells only any time they could cast a sorcery." A static ability
    /// that changes one is a [`Ability::rule_change`].
    #[must_use]
    pub const fn starting(self, rule: &'static RuleChange) -> Ability {
        assert!(
            self.resolves(),
            "only an ability that resolves starts a rule change: a static one is `rule_change`"
        );
        Ability {
            rule: Some(rule),
            ..self
        }
    }

    /// "Activate only if you control a [permanent `filter` accepts]" (CR 602.5), of a mana
    /// ability.
    #[must_use]
    pub const fn if_you_control(self, filter: &'static Filter) -> Ability {
        assert!(
            matches!(self.kind, AbilityKind::Mana),
            "only a mana ability is restricted by what you control"
        );
        Ability {
            only_if_you_control: Some(filter),
            ..self
        }
    }

    /// "Activate only as a sorcery"
    /// ([CR 602.5d](https://yawgatog.com/resources/magic-rules/#R6025d)): "Activated
    /// abilities that read 'Activate only as a sorcery' mean the player must follow the
    /// timing rules for casting a sorcery spell".
    #[must_use]
    pub const fn as_a_sorcery(self) -> Ability {
        assert!(
            matches!(self.kind, AbilityKind::Activated),
            "only an activated ability is activated as a sorcery"
        );
        Ability {
            timing: Timing::Sorcery,
            ..self
        }
    }
}

/// What an ability is and says, for whoever reads a definition.
impl Ability {
    #[inline]
    pub const fn kind(&self) -> AbilityKind {
        self.kind
    }

    /// What activating it costs. [`Cost::NONE`] for an ability that is not activated.
    #[inline]
    pub const fn cost(&self) -> &Cost {
        &self.cost
    }

    #[inline]
    pub const fn timing(&self) -> Timing {
        self.timing
    }

    #[inline]
    pub const fn targets(&self) -> &'static [TargetSpec] {
        self.targets
    }

    /// A mana ability: what it produces.
    #[inline]
    pub const fn produces(&self) -> ManaSet {
        self.produces
    }

    #[inline]
    pub const fn body(&self) -> Behavior {
        self.body
    }

    /// A triggered ability: what it triggers on.
    #[inline]
    pub const fn trigger(&self) -> Option<&'static Trigger> {
        self.trigger
    }

    /// Its intervening "if" or "as long as" ([`Ability::only_if`]).
    #[inline]
    pub const fn condition(&self) -> Option<&'static Condition> {
        self.condition
    }

    /// A replacement ability: the event it modifies.
    #[inline]
    pub const fn replacement(&self) -> Replacement {
        self.replacement
    }

    /// A static ability: the continuous effect it generates.
    #[inline]
    pub const fn layer(&self) -> Option<&'static LayerSpec> {
        self.layer
    }

    /// The rule it changes ([`Ability::rule_change`], [`Ability::starting`]).
    #[inline]
    pub const fn rule(&self) -> Option<&'static RuleChange> {
        self.rule
    }

    /// A mana ability: what its controller must control to activate it
    /// ([`Ability::if_you_control`]).
    #[inline]
    pub const fn only_if_you_control(&self) -> Option<&'static Filter> {
        self.only_if_you_control
    }

    /// Whether its card's rules text says it in the paragraph of the ability before it
    /// ([`Ability::in_paragraph_before`]).
    #[inline]
    pub const fn shares_paragraph(&self) -> bool {
        self.shares_paragraph
    }

    /// Whether resolving this may ask a player to choose: what its effect data says
    /// ([`Effect::asks`]), or for a Rust body, which nothing can read, what its registration
    /// says ([`Says::asks`]).
    pub const fn asks(&self) -> bool {
        match self.body {
            Behavior::Data(effect) => effect.asks(),
            Behavior::Custom(custom) => custom.says().asks,
        }
    }

    /// The objects its effect needs one of to do anything, where the game can be asked for
    /// them before the ability is activated ([`Effect::needs`]): the cards exiled with its
    /// source, say, and not a target or what an instruction did. With none of them,
    /// activating it is legal all the same
    /// ([CR 602.2](https://yawgatog.com/resources/magic-rules/#R6022): "To activate an
    /// ability is to put it onto the stack and pay its costs"), and paying its cost still
    /// changes the game, so full control and the exact stops offer it. No player under the
    /// smart stops is offered it, search and rollouts included: every line stays legal, since
    /// a player may always pass instead
    /// ([CR 117.3d](https://yawgatog.com/resources/magic-rules/#R1173d)). A Rust body is
    /// code, which nothing can read.
    #[inline]
    pub const fn needs(&self) -> Option<Objects> {
        match self.body {
            Behavior::Data(effect) if self.needy => effect.needs(),
            _ => None,
        }
    }

    /// Whether this takes what is chosen for its `target`th instance of the word "target"
    /// from its controller in the way `how` names, a permanent removed from the battlefield
    /// or a spell countered: what its effect data does to a target that may be one
    /// ([`Effect::takes`]), or for a Rust body what its registration says
    /// ([`Says::takes`]).
    pub fn takes_target(&self, target: SpecIx, how: Takes) -> bool {
        let Some(spec) = self.targets.get(target.index()) else {
            return false;
        };
        match self.body {
            Behavior::Data(effect) => {
                let may_be = match how {
                    Takes::Removal => TargetKind::Permanent,
                    Takes::Counter => TargetKind::Spell,
                };
                (spec.kind == may_be || spec.kind == TargetKind::SpellOrPermanent)
                    && effect.takes(target, how)
            }
            Behavior::Custom(custom) => {
                custom.says().takes.get(target.index()).copied().flatten() == Some(how)
            }
        }
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
            modifications: &[Modification::AddKeywords(Keywords::FLYING)],
        });
    }

    /// A kind holds only what it has: each of these is a build failure in a card's `static`.
    #[test]
    #[should_panic(expected = "only a spell, activated or triggered ability has targets")]
    fn a_replacement_with_targets_is_refused() {
        let _ = Ability::replacing(Replacement::EntersTapped).targeting(&[]);
    }

    #[test]
    #[should_panic(expected = "a mana ability's is what it")]
    fn a_mana_ability_with_another_effect_is_refused() {
        let _ =
            Ability::tap_for_mana(ManaSet::one(crate::ManaColor::Blue)).effect(&Effect::NOTHING);
    }

    #[test]
    fn a_mana_ability_adds_what_it_produces() {
        let produces = ManaSet::one(crate::ManaColor::Green);
        let ability = Ability::tap_for_mana(produces);
        assert!(matches!(
            ability.body(),
            Behavior::Data(Effect::AddMana { mana, .. }) if *mana == produces
        ));
    }

    #[test]
    fn custom_gives_back_only_the_type_it_was_made_of() {
        static BODY: fn() -> u8 = || 7;
        let custom = Custom::new(&BODY, &Says::NOTHING);
        assert_eq!(custom.get::<fn() -> u8>().map(|body| body()), Some(7));
        assert!(custom.get::<u8>().is_none());
    }
}
