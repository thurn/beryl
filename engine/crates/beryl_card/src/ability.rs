//! An ability of a face: its interface as data, and what it does.

use crate::continuous::{LayerSpec, Replacement, RuleChange};
use crate::cost::Cost;
use crate::effect::{Condition, Effect, Objects, Players};
use crate::ids::ZoneSet;
use crate::index::SpecIx;
use crate::mana::{ManaSet, Term};
use crate::target::{Takes, TargetKind, TargetSpec};
use crate::trigger::Trigger;

mod behavior;
mod chapter;
mod designation;
mod keywords;
mod leveler;
mod of_kind;
mod zones;

pub use behavior::{Behavior, Custom, Says};
pub use designation::Designation;
pub(crate) use keywords::CASCADE;
pub use of_kind::DeckCondition;
use of_kind::OfKind;

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
    /// A mana ability's "Activate only as an instant": only while its controller has
    /// priority, as an instant is cast ([CR 117.1a]): "A player may cast an instant spell
    /// any time they have priority." Not while a cost is paid, where any other mana ability
    /// may be activated ([CR 605.3a]): "A player may activate an activated mana ability
    /// whenever they have priority, whenever they are casting a spell or activating an
    /// ability that requires a mana payment".
    ///
    /// [CR 117.1a]: https://yawgatog.com/resources/magic-rules/#R1171a
    /// [CR 605.3a]: https://yawgatog.com/resources/magic-rules/#R6053a
    AsInstant,
    /// "Activate only during your turn": while its controller is the active player.
    YourTurn,
}

/// The most mana of one kind [`Ability::tap_for_mana`] makes.
const MOST_MANA: u8 = 3;

/// "Add [mana]" for every [`ManaSet`] of up to [`MOST_MANA`] mana, by amount less one and
/// then by its kinds. An ability holds its effect by reference, and a `const fn` cannot
/// make a reference to a value it builds from an argument, so [`Ability::tap_for_mana`]
/// points here instead.
static ADD_MANA: [[Effect; 64]; MOST_MANA as usize] = {
    let mut table = [[Effect::NOTHING; 64]; MOST_MANA as usize];
    let mut amount = 0;
    while amount < MOST_MANA {
        let mut colors = 0;
        while colors < 64 {
            table[amount as usize][colors as usize] = Effect::AddMana {
                player: Players::You,
                mana: ManaSet {
                    colors,
                    amount: amount + 1,
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
        mana.amount >= 1 && mana.amount <= MOST_MANA && mana.colors < 64,
        "more mana than this builder makes: write the ability's effect out"
    );
    &ADD_MANA[mana.amount as usize - 1][mana.colors as usize]
}

/// The one term of every [`ManaSet`] [`ADD_MANA`] adds, in its order: the product
/// ([`Ability::product`]) of an ability [`Ability::mana`] makes, for the same reason.
static ONE_TERM: [[[Term; 1]; 64]; MOST_MANA as usize] = {
    let mut table = [[[Term::of_set(ManaSet::NONE)]; 64]; MOST_MANA as usize];
    let mut amount = 0;
    while amount < MOST_MANA {
        let mut colors = 0;
        while colors < 64 {
            table[amount as usize][colors as usize] = [Term::of_set(ManaSet {
                colors,
                amount: amount + 1,
            })];
            colors += 1;
        }
        amount += 1;
    }
    table
};

/// The most terms a mana ability's product has ([`Ability::mana_of`]).
const MOST_TERMS: usize = 3;

/// "Add [terms]", for the controller of a mana ability ([`Ability::mana_of`]), whose terms
/// the ability holds.
static ADD_PRODUCT: Effect = Effect::AddProduct {
    player: Players::You,
};

/// What an ability takes or makes, which no ability does both of: a spell's, activated or
/// triggered ability's targets, or what a mana ability produces. A mana ability has no
/// targets ([CR 605.1a]): "it doesn't require a target". One field for both keeps the
/// ability's size a power of two.
///
/// [CR 605.1a]: https://yawgatog.com/resources/magic-rules/#R6051a
#[derive(Clone, Copy, Debug)]
enum Interface {
    Targets(&'static [TargetSpec]),
    /// A mana ability's terms, and the [`ManaSet`] that says the same where one does, which
    /// the mana solver reads of every source ([`Ability::produces`]).
    Product {
        terms: &'static [Term],
        set: ManaSet,
    },
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
/// up to 128 bytes.
#[derive(Clone, Copy, Debug)]
#[repr(align(32))]
pub struct Ability {
    kind: AbilityKind,
    cost: Cost,
    timing: Timing,
    /// Its targets, or a mana ability's product ([`Interface`]).
    interface: Interface,
    /// Whether its effect data needs something to do anything ([`Ability::needs`]), settled
    /// as the effect is given: the engine asks of every activated ability at every priority.
    needy: bool,
    body: Behavior,
    /// A static or replacement ability: its "as long as" or "while", read as often as the
    /// ability is. Triggered abilities: an intervening "if" clause, checked when the ability
    /// triggers and again as it resolves
    /// ([CR 603.4](https://yawgatog.com/resources/magic-rules/#R6034)): "The ability triggers
    /// only if it is; otherwise it does nothing. If the ability triggers, it checks the
    /// stated condition again as it resolves."
    condition: Option<&'static Condition>,
    /// What only one kind of ability has: a triggered ability's trigger, a static ability's
    /// continuous effect, a replacement ability's replacement, or a mana ability's terms.
    /// Each is held by reference: a [`Trigger`] names a filter and players, and a
    /// [`LayerSpec`] names objects and a condition. One field for them all keeps the
    /// ability's size a power of two.
    of_kind: OfKind,
    /// The rule the ability changes
    /// ([CR 613.11](https://yawgatog.com/resources/magic-rules/#R61311)): a static ability
    /// all the time, as long as its [`condition`](Ability::condition) holds, and any other
    /// by its effect, for as long as that says. A game numbers the rule changes its cards
    /// name here, so one a body starts is named here too.
    rule: Option<&'static RuleChange>,
    /// Whether its card's rules text says it in the paragraph of the ability before it, as
    /// "Whenever this creature enters or attacks" says the two triggered abilities a face
    /// lists for it, so that a view finds its words. The rules make a paragraph one ability
    /// ([CR 113.2c](https://yawgatog.com/resources/magic-rules/#R1132c): "each paragraph
    /// break in a card's text marks a separate ability"); a face lists one ability for each
    /// event it triggers on. Read [`Ability::shares_paragraph`].
    shares_paragraph: bool,
    /// The zones it works in: its kind's unless its card says otherwise ([`zones`]).
    zones: ZoneSet,
    designation: Designation,
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
            interface: Interface::Targets(&[]),
            needy: false,
            body: Behavior::Data(&Effect::NOTHING),
            condition: None,
            of_kind: OfKind::None,
            rule: None,
            shares_paragraph: false,
            zones: zones::usual_zones(kind),
            designation: Designation::None,
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
            interface: Interface::Product {
                terms: &ONE_TERM[produces.amount as usize - 1][produces.colors as usize],
                set: produces,
            },
            body: Behavior::Data(add_mana(produces)),
            ..Ability::of(AbilityKind::Mana)
        }
    }

    /// A mana ability whose product is up to [`MOST_TERMS`] terms, where one [`ManaSet`]
    /// cannot say it ([CR 106.3]): "A spell or ability that produces mana instructs a player
    /// to add that mana." "{1}, {T}: Add {W}{U}." is two terms of one mana, and "{2}, {T}: Add
    /// {B} for each Swamp you control." one term whose amount counts. Its effect is to add
    /// the terms ([`Effect::AddProduct`]). A product one [`ManaSet`] says is
    /// [`Ability::mana`]'s, so that each product has one form.
    ///
    /// [CR 106.3]: https://yawgatog.com/resources/magic-rules/#R1063
    pub const fn mana_of(cost: Cost, terms: &'static [Term]) -> Ability {
        assert!(
            !terms.is_empty() && terms.len() <= MOST_TERMS,
            "a mana ability's product is one to three terms"
        );
        assert!(
            terms.len() > 1 || terms[0].as_set().is_none(),
            "one term a `ManaSet` says: use `Ability::mana`"
        );
        let mut i = 0;
        while i < terms.len() {
            assert!(
                terms[i].kinds != 0 && terms[i].kinds < 64,
                "a term makes kinds of mana"
            );
            i += 1;
        }
        Ability {
            cost,
            interface: Interface::Product {
                terms,
                set: ManaSet::NONE,
            },
            body: Behavior::Data(&ADD_PRODUCT),
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
            of_kind: OfKind::Trigger(trigger),
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
            of_kind: OfKind::Layer(layer),
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
    pub const fn replacing(replacement: &'static Replacement) -> Ability {
        Ability {
            of_kind: OfKind::Replacement(replacement),
            ..Ability::of(AbilityKind::Replacement)
        }
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
        Ability {
            interface: Interface::Targets(targets),
            ..self
        }
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

    /// "Activate only during your turn" ([`Timing::YourTurn`]).
    #[must_use]
    pub const fn during_your_turn(self) -> Ability {
        assert!(
            matches!(self.kind, AbilityKind::Activated),
            "only an activated ability has an activation turn restriction"
        );
        Ability {
            timing: Timing::YourTurn,
            ..self
        }
    }

    /// "Activate only as an instant", of a mana ability ([`Timing::AsInstant`]).
    #[must_use]
    pub const fn as_an_instant(self) -> Ability {
        assert!(
            matches!(self.kind, AbilityKind::Mana),
            "only a mana ability says it is activated as an instant"
        );
        Ability {
            timing: Timing::AsInstant,
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
        match self.interface {
            Interface::Targets(targets) => targets,
            Interface::Product { .. } => &[],
        }
    }

    /// A mana ability made by [`Ability::mana`]: what it produces, the one term of its
    /// product as a [`ManaSet`]. [`ManaSet::NONE`] for one whose product a set cannot say
    /// ([`Ability::mana_of`]), and for any other kind of ability.
    #[inline]
    pub const fn produces(&self) -> ManaSet {
        match self.interface {
            Interface::Product { set, .. } => set,
            Interface::Targets(_) => ManaSet::NONE,
        }
    }

    /// A mana ability: what it produces, as its terms ([`Term`]). One term for an ability
    /// [`Ability::mana`] makes, the one [`Ability::produces`] says. Empty for any other kind
    /// of ability.
    #[inline]
    pub const fn product(&self) -> &'static [Term] {
        match self.interface {
            Interface::Product { terms, .. } => terms,
            Interface::Targets(_) => &[],
        }
    }

    #[inline]
    pub const fn body(&self) -> Behavior {
        self.body
    }

    /// Its intervening "if" or "as long as" ([`Ability::only_if`]).
    #[inline]
    pub const fn condition(&self) -> Option<&'static Condition> {
        self.condition
    }

    /// The rule it changes ([`Ability::rule_change`], [`Ability::starting`]).
    #[inline]
    pub const fn rule(&self) -> Option<&'static RuleChange> {
        self.rule
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

    /// Whether resolving this may put a permanent onto the battlefield other than as the
    /// resolving permanent spell: what its effect data says ([`Effect::may_enter_permanent`]),
    /// and always for a Rust body, which nothing can read.
    pub fn may_enter_permanent(&self) -> bool {
        match self.body {
            Behavior::Data(effect) => effect.may_enter_permanent(),
            Behavior::Custom(_) => true,
        }
    }

    /// Whether this body can suspend resolution to play a new game.
    pub const fn starts_subgame(&self) -> bool {
        match self.body {
            Behavior::Custom(custom) => custom.says().starts_subgame,
            Behavior::Data(effect) => effect.starts_subgame(),
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
            Behavior::Data(_) | Behavior::Custom(_) => None,
        }
    }

    /// Whether this takes what is chosen for its `target`th instance of the word "target"
    /// from its controller in the way `how` names, a permanent removed from the battlefield
    /// or a spell countered: what its effect data does to a target that may be one
    /// ([`Effect::takes`]), or for a Rust body what its registration says
    /// ([`Says::takes`]).
    pub fn takes_target(&self, target: SpecIx, how: Takes) -> bool {
        let Some(spec) = self.targets().get(target.index()) else {
            return false;
        };
        match self.body {
            Behavior::Data(effect) => {
                let may_be = match how {
                    Takes::Removal => spec.kind == TargetKind::Permanent,
                    Takes::Counter => {
                        spec.kind == TargetKind::Spell || spec.kind.abilities().is_some()
                    }
                };
                (may_be || spec.kind == TargetKind::SpellOrPermanent) && effect.takes(target, how)
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
mod tests;
