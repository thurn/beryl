//! Prompts: how a player answers a question and what the question is for (plan K10,
//! `docs/project-50k.md`, Target architecture, Prompts).
//!
//! The engine asks one factored pick at a time (`docs/engine-architecture.md` §3, §7.1): an
//! object among objects, an option among options. A player answers a *prompt*: a whole scry, a
//! whole declaration of attackers, a search they may decline. A prompt is a run of picks, and
//! every pick of the run carries the same description of it, a [`Prompt`]:
//!
//! - its [`Purpose`], what the question is for, from a closed list. A purpose tunes wording,
//!   sound and motion, and nothing else;
//! - its [`Shape`], how the player answers, which the purpose fixes ([`Purpose::shape`]): two
//!   questions answered differently are two shapes, or one shape with different parameters;
//! - the shape's parameters ([`Prompt::count`], [`Prompt::most`], [`Prompt::places`],
//!   [`Prompt::cost`]), and, beside the record in the decision, the set of objects the prompt
//!   is *about* beyond the pick's own candidates;
//! - the [`Part`] of the shape this pick answers.
//!
//! A consumer folds a run into one prompt from these alone and never from the card asking: a
//! card whose question fits a shape and a purpose here needs no code anywhere else. A new
//! purpose is one entry of the list below and its wording.
//!
//! # The object set
//!
//! This crate knows no slots, so the set lives beside the record, in the decision. What it
//! holds is fixed by the shape and the part, never by the purpose:
//!
//! - at a [`Part::Whether`] or [`Part::Instead`], the objects the picks after a yes choose
//!   among; the pick after a [`Part::Whether`] carries the same set, of which its candidates
//!   are one of each group of interchangeable objects;
//! - at a [`Part::Aim`], the objects being aimed;
//! - for a [`Shape::YesNo`] or a [`Shape::Mode`], the objects the question is about;
//! - for a [`Shape::Divide`], the objects still to be given a share, the first of them next;
//! - otherwise nothing.
//!
//! The cards a prompt shows are those of the zone its objects lie in (the set's, or else the
//! candidates'): the top [`Prompt::count`] cards of that library when a count is given, the
//! whole hand or graveyard otherwise, and a permanent where it lies.
//!
//! # Runs
//!
//! Each shape says which picks its run is made of and in what order they come; the rules that
//! ask them are cited at each purpose. A pick the engine does not ask, because one answer was
//! left, is skipped in the run and its answer taken as given (§7.2).

use crate::mana::ManaCost;

/// How a player answers a prompt. The list is closed: the UI builds each shape once.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Shape {
    /// Pick objects, then confirm: as few as none, when a pick offers its end or the run
    /// opens with a [`Part::Whether`], and as many as [`Prompt::most`]. The run is, in order:
    /// a [`Part::Whether`] or [`Part::Instead`] (a yes or no), where the prompt may be
    /// declined or has an alternative; a [`Part::Count`] (a number), where the count is
    /// announced before the objects; then one [`Part::Pick`] for each object, and for the
    /// end. Objects are answered in the order the engine offers them.
    Pick,
    /// Put things in an order: one [`Part::Pick`] for each, the first taken first.
    Order,
    /// Deal cards out between two destinations ([`Prompt::places`]), in order where the
    /// destination keeps one. The run asks for the first destination's cards
    /// ([`Part::First`]), which takes at most [`Prompt::most`] of them, exactly that many
    /// when its picks offer no end; then for the order of the second's ([`Part::Second`]),
    /// which takes the rest. A run that opens at [`Part::Second`] has its first destination
    /// settled.
    Split,
    /// Yes or no, in one [`Part::Pick`].
    YesNo,
    /// A number, in one [`Part::Pick`].
    Number,
    /// One of a few options, in one [`Part::Pick`].
    Mode,
    /// One entry of a catalog: a color, a creature type, a card name. One [`Part::Pick`],
    /// whose candidates name the catalog's entries in the catalog's own order.
    Catalog,
    /// Divide an amount among recipients
    /// ([CR 601.2d](https://yawgatog.com/resources/magic-rules/#R6012d)): "If the spell
    /// requires the player to divide or distribute an effect (such as damage or counters)
    /// among one or more targets, the player announces the division. Each of these targets
    /// must receive at least one of whatever is being divided." One [`Part::Pick`], a number,
    /// for each recipient in turn; [`Prompt::count`] is the whole amount and [`Prompt::most`]
    /// what is left of it.
    Divide,
    /// Pay a cost with permanents: tap them one [`Part::Pick`] at a time until the cost is
    /// met or the player ends, or take a whole payment the engine worked out
    /// ([`Part::Plan`]). [`Prompt::cost`] is the mana still to pay, [`Prompt::count`] an
    /// amount that is not mana (the power a crew cost asks) and [`Prompt::most`] what is left
    /// of it.
    Pay,
    /// Choose objects and what each is aimed at: one [`Part::Pick`] for an object, or for the
    /// end, each followed by a [`Part::Aim`] where the object has more than one thing to be
    /// aimed at.
    Declare,
}

impl Shape {
    /// Every shape.
    pub const ALL: &[Shape] = &[
        Shape::Pick,
        Shape::Order,
        Shape::Split,
        Shape::YesNo,
        Shape::Number,
        Shape::Mode,
        Shape::Catalog,
        Shape::Divide,
        Shape::Pay,
        Shape::Declare,
    ];
}

/// The purposes: each with its name, as wording and data key it, and the shape it is answered
/// in. A `const fn` cannot enumerate an enum's variants.
macro_rules! purposes {
    ($($(#[$doc:meta])* $variant:ident = $name:literal => $shape:ident),* $(,)?) => {
        /// What a prompt is for. The list is closed. A purpose says nothing of how the prompt
        /// is answered, only what its words, sounds and motion are, so adding one is an entry
        /// here and its wording.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Purpose {
            $($(#[$doc])* $variant),*
        }

        impl Purpose {
            /// Every purpose.
            pub const ALL: &[Purpose] = &[$(Purpose::$variant),*];

            /// Its name, in wording tables, text snapshots and on the wire.
            pub const fn name(self) -> &'static str {
                match self {
                    $(Purpose::$variant => $name),*
                }
            }

            /// How a prompt for this purpose is answered.
            pub const fn shape(self) -> Shape {
                match self {
                    $(Purpose::$variant => Shape::$shape),*
                }
            }
        }
    };
}

purposes! {
    /// What to do with priority
    /// ([CR 117.1](https://yawgatog.com/resources/magic-rules/#R1171)): "The player with
    /// priority may cast spells, activate abilities, and take special actions." One pick: a
    /// card to play or use, or the pass.
    Priority = "priority" => Pick,
    /// The targets of a spell or ability
    /// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): "The player
    /// announces their choice of an appropriate object or player for each target the spell
    /// requires." A variable number of them is a [`Part::Count`] first.
    Target = "target" => Pick,
    /// An object a resolving spell or ability has its player choose, for nothing a purpose
    /// below names ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an
    /// effect of a spell or ability offers any choices ..., the player announces these while
    /// applying the effect."
    Choose = "choose" => Pick,
    /// Cards to discard
    /// ([CR 701.9b](https://yawgatog.com/resources/magic-rules/#R7019b)): "By default,
    /// effects that cause a player to discard a card allow the affected player to choose
    /// which card to discard." With a [`Part::Instead`], one card of the object set is
    /// discarded in place of them all.
    Discard = "discard" => Pick,
    /// Cards to discard in the cleanup step
    /// ([CR 514.1](https://yawgatog.com/resources/magic-rules/#R5141)): "if the active
    /// player's hand contains more cards than their maximum hand size (normally seven), they
    /// discard enough cards to reduce their hand size to that number."
    Cleanup = "cleanup" => Pick,
    /// Cards from the hand to put on the bottom of the library after a mulligan
    /// ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): the player "puts a
    /// number of those cards equal to the number of times that player has taken a mulligan on
    /// the bottom of their library in any order."
    PutOnBottom = "put-on-bottom" => Pick,
    /// The legendary permanent to keep
    /// ([CR 704.5j](https://yawgatog.com/resources/magic-rules/#R7045j)): "If two or more
    /// legendary permanents with the same name are controlled by the same player, that player
    /// chooses one of them, and the rest are put into their owners' graveyards."
    LegendRule = "legend-rule" => Pick,
    /// A card among cards being looked at, the top of a library or a hand, to move to the
    /// first of [`Prompt::places`]. It opens with a [`Part::Whether`] when taking none is
    /// allowed.
    Look = "look" => Pick,
    /// The cards a search finds
    /// ([CR 701.23a](https://yawgatog.com/resources/magic-rules/#R70123a)): "To search for a
    /// card in a zone, look at all cards in that zone (even if it's a hidden zone) and find a
    /// card that matches the given description." Finding none is allowed
    /// ([CR 701.23b](https://yawgatog.com/resources/magic-rules/#R70123b)): "that player
    /// isn't required to find some or all of those cards".
    Search = "search" => Pick,
    /// Permanents to untap.
    Untap = "untap" => Pick,
    /// The card to manifest of the two looked at
    /// ([CR 701.62a](https://yawgatog.com/resources/magic-rules/#R70162a)): "Look at the top
    /// two cards of your library. Manifest one of them, then put the cards you looked at that
    /// were not manifested this way into your graveyard."
    ManifestDread = "manifest-dread" => Pick,
    /// Cards to exile to pay a cost, from a hand or a graveyard
    /// ([CR 601.2h](https://yawgatog.com/resources/magic-rules/#R6012h)): "The player pays
    /// the total cost."
    ExileCost = "exile-cost" => Pick,
    /// The blockers an attacker assigns lethal damage to
    /// ([CR 510.1c](https://yawgatog.com/resources/magic-rules/#R5101c)): "If two or more
    /// creatures are blocking it, it assigns its combat damage to those creatures divided as
    /// its controller chooses among them."
    CombatDamage = "combat-damage" => Pick,
    /// The order a player's waiting triggered abilities go on the stack
    /// ([CR 603.3b](https://yawgatog.com/resources/magic-rules/#R6033b)): "each player, in
    /// APNAP order, puts each triggered ability they control ... on the stack in any order
    /// they choose." The first taken goes on the stack first and so resolves last
    /// ([CR 405.2](https://yawgatog.com/resources/magic-rules/#R4052)): "Each time an object
    /// is put on the stack, it's put on top of all objects already there".
    TriggerOrder = "trigger-order" => Order,
    /// A scry ([CR 701.22a](https://yawgatog.com/resources/magic-rules/#R70122a)): "look at
    /// the top N cards of your library, then put any number of them on the bottom of your
    /// library in any order and the rest on top of your library in any order."
    Scry = "scry" => Split,
    /// Cards being looked at dealt out between two places, as "put two of them into your hand
    /// and the rest on the bottom of your library in any order"
    /// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two
    /// or more cards in a specific position in a library at the same time, the owner of those
    /// cards may arrange them in any order."
    Arrange = "arrange" => Split,
    /// Cards from the hand put on top of the library in an order, the rest staying.
    PutBack = "put-back" => Split,
    /// Whether to do what a "may" allows, where no purpose below names it (CR 608.2d).
    May = "may" => YesNo,
    /// Whether to search a library at all. Declining differs from finding nothing only by
    /// the shuffle ([CR 701.24a](https://yawgatog.com/resources/magic-rules/#R70124a)): "To
    /// shuffle a library ..., randomize the cards within it so that no player knows their
    /// order." The library is not looked at until the answer is yes, so this cannot open the
    /// search's own prompt.
    MaySearch = "may-search" => YesNo,
    /// Whether to move the card the question is about to the first of [`Prompt::places`], as
    /// "you may exile that card from your graveyard".
    Move = "move" => YesNo,
    /// Whether the top card of a library goes to its bottom, a fateseal of one
    /// ([CR 701.29a](https://yawgatog.com/resources/magic-rules/#R70129a)): "To 'fateseal N'
    /// means to look at the top N cards of an opponent's library, then put any number of them
    /// on the bottom of that library in any order and the rest on top of that library in any
    /// order."
    Fateseal = "fateseal" => YesNo,
    /// Whether a card goes on the top (yes) or the bottom of its owner's library.
    TopOrBottom = "top-or-bottom" => YesNo,
    /// Whether the card an explore revealed goes to the graveyard
    /// ([CR 701.44a](https://yawgatog.com/resources/magic-rules/#R70144a)): the controller
    /// "may put the revealed card into their graveyard."
    Explore = "explore" => YesNo,
    /// Whether to pay [`Prompt::cost`] so that an effect does not happen
    /// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "[Effect] unless
    /// [player] pays [cost]".
    UnlessPays = "unless-pays" => YesNo,
    /// Whether to pay [`Prompt::count`] life so that the permanent the question is about
    /// does not enter tapped.
    PayLife = "pay-life" => YesNo,
    /// The value of X ([CR 601.2b](https://yawgatog.com/resources/magic-rules/#R6012b)): "If
    /// the spell has a variable cost that will be paid as it's being cast (such as an {X} in
    /// its mana cost; see rule 107.3), the player announces the value of that variable."
    X = "x" => Number,
    /// Who takes the first turn
    /// ([CR 103.1](https://yawgatog.com/resources/magic-rules/#R1031)): "the players
    /// determine which one of them will choose who takes the first turn".
    PlayOrDraw = "play-or-draw" => Mode,
    /// Keep the hand or take a mulligan
    /// ([CR 103.5](https://yawgatog.com/resources/magic-rules/#R1035)): "First, the starting
    /// player declares whether they will take a mulligan."
    Mulligan = "mulligan" => Mode,
    /// Whether, and how, to cast a card a resolved spell or ability lets its player cast
    /// ([CR 601.3](https://yawgatog.com/resources/magic-rules/#R6013)): "A player can begin to
    /// cast a spell only if a rule or effect allows that player to cast it".
    CastOffer = "cast-offer" => Mode,
    /// Which of a card's ways of being played or used to take, at a priority. No decision
    /// asks it: a session opens it over a [`Purpose::Priority`] when the card picked offers
    /// more than one.
    Act = "act" => Mode,
    /// A color ([CR 105.4](https://yawgatog.com/resources/magic-rules/#R1054)): "If a player
    /// is asked to choose a color, they must choose one of the five colors." No card asks it
    /// yet.
    Color = "color" => Catalog,
    /// A creature type ([CR 205.3m](https://yawgatog.com/resources/magic-rules/#R2053m)):
    /// "Creatures and kindreds share their lists of subtypes; these subtypes are called
    /// creature types." No card asks it yet.
    CreatureType = "creature-type" => Catalog,
    /// A card name ([CR 201.4](https://yawgatog.com/resources/magic-rules/#R2014)): "If an
    /// effect instructs a player to choose a card name, the player must choose the name of a
    /// card in the Oracle card reference." No card asks it yet.
    CardName = "card-name" => Catalog,
    /// Damage divided among targets (CR 601.2d). No card asks it yet.
    DivideDamage = "divide-damage" => Divide,
    /// Counters distributed among targets (CR 601.2d). No card asks it yet.
    DistributeCounters = "distribute-counters" => Divide,
    /// A mana cost paid by tapping sources or spending floating mana
    /// ([CR 601.2g](https://yawgatog.com/resources/magic-rules/#R6012g)): "they then have a
    /// chance to activate mana abilities."
    Mana = "mana" => Pay,
    /// A crew cost ([CR 702.122a](https://yawgatog.com/resources/magic-rules/#R702122a)):
    /// "Tap any number of untapped creatures you control with total power N or greater".
    Crew = "crew" => Pay,
    /// A waterbend cost ([CR 701.67a](https://yawgatog.com/resources/magic-rules/#R70167a)):
    /// "For each generic mana in that cost, you may tap an untapped artifact or creature you
    /// control rather than pay that mana."
    Waterbend = "waterbend" => Pay,
    /// The attackers and what each attacks
    /// ([CR 508.1a](https://yawgatog.com/resources/magic-rules/#R5081a)): "The active player
    /// chooses which creatures that they control, if any, will attack."
    /// ([CR 508.1b](https://yawgatog.com/resources/magic-rules/#R5081b)): the player
    /// "announces which player, planeswalker, or battle each of the chosen creatures is
    /// attacking." A creature put onto the battlefield attacking is an aim alone
    /// ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): "their controller
    /// chooses which defending player, which planeswalker ... each one is attacking".
    Attack = "attack" => Declare,
    /// The blockers and the attacker each blocks
    /// ([CR 509.1a](https://yawgatog.com/resources/magic-rules/#R5091a)): "The defending
    /// player chooses which creatures they control, if any, will block. ... For each of the
    /// chosen creatures, the defending player chooses one creature for it to block".
    Block = "block" => Declare,
}

/// Which part of its prompt's shape one pick answers. Before this said so, the kind of the
/// decision did: in a scry a pick that offered an end chose for the bottom, and one that did
/// not chose for the top.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Part {
    /// The shape's ordinary pick: an object, an option, a number, or the end of the picks.
    #[default]
    Pick,
    /// Yes or no, before the picks: whether to make them at all. The object set holds what a
    /// yes then picks among; a no ends the prompt.
    Whether,
    /// Yes or no, before the picks: whether to answer with one of the object set in place of
    /// the [`Prompt::most`] picks a no asks for among all the cards of their zone, as "discard
    /// two cards unless you discard a basic land card"
    /// ([CR 118.12a](https://yawgatog.com/resources/magic-rules/#R11812a)): "This means the
    /// same thing as '[A player may do something else]. If [that player doesn't], [do
    /// something].'"
    Instead,
    /// A number, before the picks: how many of them follow, at most [`Prompt::most`]
    /// ([CR 601.2c](https://yawgatog.com/resources/magic-rules/#R6012c)): "If the spell has a
    /// variable number of targets, the player announces how many targets they will choose
    /// before they announce those targets".
    Count,
    /// A card for the first of a split's destinations, or the end of them.
    First,
    /// A card for the second of a split's destinations.
    Second,
    /// What the object just picked, or each object of the set, is aimed at: the player or
    /// planeswalker an attacker attacks, the attacker a blocker blocks.
    Aim,
    /// A whole payment at once, one of those the engine worked out.
    Plan,
}

/// Where a prompt puts a card: a zone, and for a library which end and in what order.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Place {
    /// Nowhere: the card stays where it is, or the prompt has no such destination.
    #[default]
    None,
    /// The top of its owner's library, each card under the one picked before: the first
    /// picked ends on top.
    Top,
    /// The top of its owner's library, each card on the one picked before: the last picked
    /// ends on top.
    OnTop,
    /// The bottom of its owner's library, each card under the one picked before.
    Bottom,
    Hand,
    Graveyard,
    Exile,
    Battlefield,
}

impl Place {
    /// Whether the order of the cards put here matters: the ends of a library keep one
    /// ([CR 401.4](https://yawgatog.com/resources/magic-rules/#R4014)): "If an effect puts two
    /// or more cards in a specific position in a library at the same time, the owner of those
    /// cards may arrange them in any order."
    pub const fn ordered(self) -> bool {
        matches!(self, Place::Top | Place::OnTop | Place::Bottom)
    }
}

/// What a decision says of the prompt its pick belongs to, in place of a kind of question per
/// card. Every pick of a run carries the prompt's purpose and its parameters as they stand at
/// that pick: a count still to choose falls as the run goes.
///
/// It is plain data with no slot in it, the same in every layout of the state, and small: a
/// decision holds it beside its player and kind, and its object set, in the bytes the old
/// per-card question took.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Prompt {
    pub purpose: Purpose,
    pub part: Part,
    /// How much the prompt is about as a whole: the number of cards on top of a library being
    /// looked at, the power a crew cost asks, the life a payment costs, the amount to divide.
    /// Zero says nothing: every card of the zone is shown.
    pub count: u8,
    /// How much this part still takes at most: the objects still to pick, the cards still to
    /// go to a split's first destination, the power still to tap, the amount still to divide.
    /// Zero where the shape counts nothing (yes or no, a mode, a number) or sets no limit.
    pub most: u8,
    /// Where the cards go. For a split, its two destinations, in the order the run asks for
    /// them. For a pick, where a picked card goes, first. For a yes or no about a card, where
    /// it goes on a yes, then on a no.
    pub places: [Place; 2],
    /// The mana the prompt is about: what an "unless" asks, what is still to pay.
    pub cost: ManaCost,
}

// A decision is copied with the state, and this must fit beside its player and kind without
// growing it: 16 bytes, less those two.
const _: () = assert!(size_of::<Prompt>() <= 14 && align_of::<Prompt>() == 1);

impl Prompt {
    /// A prompt for `purpose` whose pick is the shape's ordinary one, with no parameters.
    pub const fn new(purpose: Purpose) -> Prompt {
        Prompt {
            purpose,
            part: Part::Pick,
            count: 0,
            most: 0,
            places: [Place::None; 2],
            cost: ManaCost::NONE,
        }
    }

    /// How the prompt is answered.
    pub const fn shape(self) -> Shape {
        self.purpose.shape()
    }

    /// This prompt, for the pick that answers `part` of it.
    #[must_use]
    pub const fn part(mut self, part: Part) -> Prompt {
        self.part = part;
        self
    }

    /// This prompt about `count` as a whole ([`Prompt::count`]).
    #[must_use]
    pub const fn count(mut self, count: u8) -> Prompt {
        self.count = count;
        self
    }

    /// This prompt taking at most `most` more ([`Prompt::most`]).
    #[must_use]
    pub const fn most(mut self, most: u8) -> Prompt {
        self.most = most;
        self
    }

    /// This prompt putting cards in `first`, then `second` ([`Prompt::places`]).
    #[must_use]
    pub const fn places(mut self, first: Place, second: Place) -> Prompt {
        self.places = [first, second];
        self
    }

    /// This prompt about the mana cost `cost` ([`Prompt::cost`]).
    #[must_use]
    pub const fn cost(mut self, cost: ManaCost) -> Prompt {
        self.cost = cost;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_shape_has_a_purpose_and_names_are_distinct() {
        for &shape in Shape::ALL {
            assert!(
                Purpose::ALL.iter().any(|p| p.shape() == shape),
                "{shape:?} has no purpose"
            );
        }
        for (i, a) in Purpose::ALL.iter().enumerate() {
            assert!(Shape::ALL.contains(&a.shape()));
            let name = a.name();
            assert!(
                name.bytes().all(|b| b.is_ascii_lowercase() || b == b'-'),
                "{name} is not a lowercase name"
            );
            assert!(
                Purpose::ALL[i + 1..].iter().all(|b| b.name() != name),
                "{name} names two purposes"
            );
        }
    }

    #[test]
    fn a_scry_is_one_record_through_its_run() {
        // The bottom's picks, then the top's: the same purpose, places and count throughout.
        let bottom = Prompt::new(Purpose::Scry)
            .count(3)
            .most(3)
            .places(Place::Bottom, Place::Top)
            .part(Part::First);
        let top = bottom.most(0).part(Part::Second);
        assert_eq!(bottom.shape(), Shape::Split);
        assert_eq!(
            (top.purpose, top.count, top.places),
            (bottom.purpose, 3, bottom.places)
        );
        assert!(bottom.places.iter().all(|p| p.ordered()));
        assert!(!Place::Hand.ordered());
    }
}
