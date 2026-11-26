//! Replacement effects a static ability makes: what event they replace and what happens
//! instead.

use crate::counter::CounterKind;
use crate::effect::Players;
use crate::filter::Filter;
use crate::ids::Zone;
use crate::index::ExceptIx;
use crate::prompt::Purpose;

/// A replacement effect of a static ability
/// ([CR 614.1](https://yawgatog.com/resources/magic-rules/#R6141)): "Some continuous effects
/// are replacement effects." It is what it replaces ([`Replaced`]) and what happens instead
/// ([`Rewrite`]). Whether it applies at a given moment is also its ability's
/// [`condition`](crate::Ability::condition), and a [`Rewrite::Instead`] is its ability's
/// [`body`](crate::Ability::body).
///
/// The engine applies the pairs it has code for and refuses the rest when it builds a game's
/// card table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Replacement {
    pub replaces: Replaced,
    pub rewrite: Rewrite,
}

impl Replacement {
    /// No replacement: the ability is of another kind.
    pub const NONE: Replacement = Replacement {
        replaces: Replaced::None,
        rewrite: Rewrite::None,
    };

    /// "This enters tapped."
    pub const ENTERS_TAPPED: Replacement = Replacement::this_enters(Rewrite::EntersTapped);

    /// "This enters prepared" (see CR 723.3a).
    pub const ENTERS_PREPARED: Replacement = Replacement::this_enters(Rewrite::EntersPrepared);

    /// If you would begin your turn while this permanent is tapped, you may skip that turn
    /// instead. If you do, untap this permanent.
    pub const SKIP_TURN_TO_UNTAP: Replacement = Replacement {
        replaces: Replaced::BeginTurn,
        rewrite: Rewrite::MaySkipTurnToUntap,
    };

    /// A replacement of how the permanent with the ability enters.
    pub const fn this_enters(rewrite: Rewrite) -> Replacement {
        Replacement {
            replaces: Replaced::ThisEnters,
            rewrite,
        }
    }

    /// "This enters tapped unless you control N or fewer other lands." (Seachrome Coast)
    pub const fn enters_tapped_unless_few_lands(n: u8) -> Replacement {
        Replacement::this_enters(Rewrite::EntersTappedUnlessFewLands(n))
    }

    /// "This enters tapped unless you control N or more other [permanents the filter
    /// accepts]" (Mystic Sanctuary), or with N of one, "unless you control a [permanent the
    /// filter accepts]" (Castle Vantress). The filter is read for the entering permanent's
    /// controller and need not say "you control".
    pub const fn enters_tapped_unless_you_control(n: u8, filter: &'static Filter) -> Replacement {
        Replacement::this_enters(Rewrite::EntersTappedUnlessYouControl(n, filter))
    }

    /// "As this enters, you may pay N life. If you don't, it enters tapped." (Soporific
    /// Springs)
    pub const fn enters_tapped_unless_pay_life(n: u8) -> Replacement {
        Replacement::this_enters(Rewrite::EntersTappedUnlessPayLife(n))
    }

    /// "This enters with N [kind] counters on it" (Dark Depths), a replacement effect
    /// ([CR 614.1c]): "Effects that read '[This permanent] enters with . . . ,' 'As [this
    /// permanent] enters . . . ,' or '[This permanent] enters as . . . ' are replacement
    /// effects." A permanent that becomes a copy of it without entering does not get them.
    ///
    /// [CR 614.1c]: https://yawgatog.com/resources/magic-rules/#R6141c
    pub const fn enters_with_counters(kind: CounterKind, n: u8) -> Replacement {
        Replacement::this_enters(Rewrite::EntersWithCounters(kind, n))
    }

    /// "As this enters, choose a color" (or a creature type, or a card name)
    /// ([CR 614.12a](https://yawgatog.com/resources/magic-rules/#R61412a)): "If a replacement
    /// effect that modifies how a permanent enters the battlefield requires a choice, that
    /// choice is made before the permanent enters the battlefield." The permanent remembers
    /// the value for as long as it stays.
    pub const fn choose_as_enters(kind: ChosenKind) -> Replacement {
        Replacement::this_enters(Rewrite::Choose(kind))
    }

    /// "You may have this enter as a copy of any [permanent the filter accepts] on the
    /// battlefield", "except [the exception at `except` of the face's list]"
    /// ([CR 707.9]): "Copy effects may include modifications or exceptions to the copying
    /// process." It becomes a copy as it enters ([CR 707.5]): "It doesn't enter the
    /// battlefield, and then become a copy of that permanent." Such an effect is applied
    /// before the other replacements of entering ([CR 616.1c]): "If any of the replacement
    /// and/or prevention effects would cause an object to become a copy of another object as
    /// it enters the battlefield, one of them must be chosen." The permanent is chosen, not
    /// targeted ([CR 115.10a]): "Unless that object or player is identified by the word
    /// 'target' in the text of that spell or ability, or the rule for that keyword ability,
    /// it's not a target." Its controller may choose none.
    ///
    /// [CR 707.9]: https://yawgatog.com/resources/magic-rules/#R7079
    /// [CR 707.5]: https://yawgatog.com/resources/magic-rules/#R7075
    /// [CR 616.1c]: https://yawgatog.com/resources/magic-rules/#R6161c
    /// [CR 115.10a]: https://yawgatog.com/resources/magic-rules/#R11510a
    pub const fn enter_as_copy(of: &'static Filter, except: Option<ExceptIx>) -> Replacement {
        Replacement::this_enters(Rewrite::EnterAsCopy { of, except })
    }

    /// "If [players] would draw a card, [the ability's body] instead."
    pub const fn draws(players: Players) -> Replacement {
        Replacement {
            replaces: Replaced::Draw(players, Draws::Each),
            rewrite: Rewrite::Instead,
        }
    }

    /// "If [players] would draw a card except the first one they draw in each of their draw
    /// steps, [the ability's body] instead." In the body, "that player" is the player who
    /// would draw.
    pub const fn draws_except_first_in_draw_step(players: Players) -> Replacement {
        Replacement {
            replaces: Replaced::Draw(players, Draws::ExceptFirstInDrawStep),
            rewrite: Rewrite::Instead,
        }
    }

    /// Dredge N ([CR 702.52a]): "Dredge is a static ability that functions only while the
    /// card with dredge is in a player's graveyard. 'Dredge N' means 'As long as you have at
    /// least N cards in your library, if you would draw a card, you may instead mill N cards
    /// and return this card from your graveyard to your hand.'"
    ///
    /// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
    pub const fn dredge(n: u8) -> Replacement {
        Replacement {
            replaces: Replaced::Draw(Players::You, Draws::Each),
            rewrite: Rewrite::Dredge(n),
        }
    }

    /// "If [objects] would be put into [zone] from anywhere, put it into [instead] instead":
    /// "If a card or token would be put into a graveyard from anywhere, exile it instead."
    pub const fn put_into(objects: &'static Filter, zone: Zone, instead: Zone) -> Replacement {
        Replacement {
            replaces: Replaced::PutInto(objects, zone),
            rewrite: Rewrite::ToZone(instead),
        }
    }

    /// "If [objects] would enter and it wasn't cast, put it into [instead] instead": "If a
    /// nontoken creature would enter and it wasn't cast, exile it instead."
    pub const fn enters_not_cast(objects: &'static Filter, instead: Zone) -> Replacement {
        Replacement {
            replaces: Replaced::Enters {
                objects,
                not_cast: true,
            },
            rewrite: Rewrite::ToZone(instead),
        }
    }

    /// "If a source would deal damage to a permanent or player, it deals [N times] that damage
    /// to that permanent or player instead." (Furnace of Rath)
    pub const fn multiplies_damage(n: u8) -> Replacement {
        Replacement {
            replaces: Replaced::Damage,
            rewrite: Rewrite::Multiply(n),
        }
    }

    /// "If an effect would create one or more tokens under [players'] control, it creates
    /// [rewrite] instead." (Parallel Lives)
    pub const fn tokens_created(players: Players, rewrite: Rewrite) -> Replacement {
        Replacement {
            replaces: Replaced::Tokens { players },
            rewrite,
        }
    }
}

/// The event a replacement effect replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Replaced {
    /// No event.
    None,
    /// The permanent with the ability entering the battlefield
    /// ([CR 614.1c](https://yawgatog.com/resources/magic-rules/#R6141c)): "Effects that read
    /// '[This permanent] enters with . . . ,' 'As [this permanent] enters . . . ,' or '[This
    /// permanent] enters as . . . ' are replacement effects."
    /// ([CR 614.12](https://yawgatog.com/resources/magic-rules/#R61412)): "Some replacement
    /// effects modify how a permanent enters the battlefield. ... Such effects may come from
    /// the permanent itself if they affect only that permanent".
    ThisEnters,
    /// A card drawn by these players, "you" being the permanent's controller
    /// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects that use the
    /// word 'instead' are replacement effects." It applies even to a draw from an empty
    /// library ([CR 121.6a](https://yawgatog.com/resources/magic-rules/#R1216a)): "An effect
    /// that replaces a card draw is applied even if no cards could be drawn because there are
    /// no cards in the affected player's library." Which of their draws it replaces is
    /// [`Draws`].
    Draw(Players, Draws),
    /// An object the filter accepts, read for the permanent's controller, being put into a
    /// zone from any zone.
    PutInto(&'static Filter, Zone),
    /// An object the filter accepts entering the battlefield from any zone, other than the
    /// permanent with the ability. The filter is read for the permanent's controller and asked
    /// of the object as it would exist on the battlefield
    /// ([CR 614.12](https://yawgatog.com/resources/magic-rules/#R61412)): "To determine which
    /// replacement effects apply and how they apply, check the characteristics of the
    /// permanent as it would exist on the battlefield". With `not_cast`, only an object that
    /// is not the permanent a resolving spell becomes ("and it wasn't cast").
    Enters {
        objects: &'static Filter,
        not_cast: bool,
    },
    /// The controller of this permanent beginning their turn.
    BeginTurn,
    /// Damage a source would deal to a permanent or a player, whichever the source and the
    /// recipient ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects
    /// that use the word 'instead' are replacement effects."
    Damage,
    /// One or more tokens an effect would create under the control of `players`, "you" being
    /// the permanent's controller
    /// ([CR 614.1a](https://yawgatog.com/resources/magic-rules/#R6141a)): "Effects that use the
    /// word 'instead' are replacement effects."
    Tokens { players: Players },
}

impl crate::Ability {
    /// Whether it replaces draws ([`Replaced::Draw`]). In its body, "that player"
    /// ([`crate::Players::ThatPlayer`]) is the player who would draw.
    #[inline]
    pub const fn replaces_draws(&self) -> bool {
        matches!(
            self.replacement(),
            Some(Replacement {
                replaces: Replaced::Draw(..),
                ..
            })
        )
    }
}

/// Which of a player's draws a replacement of draws ([`Replaced::Draw`]) replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Draws {
    /// Every card they would draw.
    Each,
    /// Every card but the first one they draw in each of their draw steps
    /// ([CR 504.1](https://yawgatog.com/resources/magic-rules/#R5041)): "First, the active
    /// player draws a card." A draw that was replaced was never made, so the first card they
    /// do draw in their draw step is that first one.
    ExceptFirstInDrawStep,
}

/// What a replacement effect has happen instead of the event it replaces.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Rewrite {
    /// Nothing.
    None,
    /// The object is put into this zone instead ("exile it instead").
    ToZone(Zone),
    /// It enters tapped.
    EntersTapped,
    /// It enters tapped unless its controller controls N or fewer other lands.
    EntersTappedUnlessFewLands(u8),
    /// It enters tapped unless its controller controls N or more permanents that the filter
    /// accepts, read for that controller. It checks as the permanent enters, before it is on
    /// the battlefield, so every permanent it counts is another one, and a permanent entering
    /// at the same time is not counted.
    EntersTappedUnlessYouControl(u8, &'static Filter),
    /// Its controller may pay N life as it enters; if they don't, it enters tapped. Whoever
    /// puts it onto the battlefield asks its controller first.
    EntersTappedUnlessPayLife(u8),
    /// It enters with N counters of a kind on it ([`Replacement::enters_with_counters`]).
    EntersWithCounters(CounterKind, u8),
    /// While this permanent is tapped, its controller may skip the turn to untap it.
    MaySkipTurnToUntap,
    /// The ability's [`body`](crate::Ability::body) happens instead.
    Instead,
    /// Dredge N ([`Replacement::dredge`]): its owner may mill N cards instead of drawing, if
    /// their library has at least N, and the card returns from their graveyard to their
    /// hand. It works only from its owner's graveyard and is optional, so it is a choice even
    /// when it is the only replacement of the draw.
    Dredge(u8),
    /// Its controller chooses a value of this kind as it enters, which the permanent
    /// remembers ([`Replacement::choose_as_enters`]).
    Choose(ChosenKind),
    /// Multiplies the replaced number by N, whatever it counts (damage dealt, tokens created or
    /// counters placed): "twice that many" or "double that damage" is 2.
    Multiply(u8),
    /// Adds N to the replaced number (tokens created or counters placed).
    Plus(u8),
    /// "You may have this enter as a copy of [a permanent the filter accepts] on the
    /// battlefield", with the exception at `except` of the face's list
    /// ([`crate::Face::excepts`]) if it has one ([`Replacement::enter_as_copy`]).
    EnterAsCopy {
        of: &'static Filter,
        except: Option<ExceptIx>,
    },
    /// Gives an entering permanent its prepared designation (see CR 723.3a).
    EntersPrepared,
}

/// The kind of value a permanent chooses as it enters ([`Rewrite::Choose`]): one entry of a
/// catalog, asked in the catalog prompt shape.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChosenKind {
    /// One of the five colors ([CR 105.4](https://yawgatog.com/resources/magic-rules/#R1054)):
    /// "If a player is asked to choose a color, they must choose one of the five colors."
    Color,
    /// A creature type ([CR 205.3e](https://yawgatog.com/resources/magic-rules/#R2053e)): "If
    /// an effect instructs a player to choose a subtype, that player must choose one, and
    /// only one, existing subtype".
    CreatureType,
    /// A card name ([CR 201.4](https://yawgatog.com/resources/magic-rules/#R2014)): "If an
    /// effect instructs a player to choose a card name, the player must choose the name of a
    /// card in the Oracle card reference."
    CardName,
    /// A color word or a basic land type, as a text-changing effect names them
    /// ([`crate::TextWord`]): "replacing all instances of one color word with another or one
    /// basic land type with another". No permanent chooses one as it enters.
    Word,
}

impl ChosenKind {
    /// The purpose the choice is asked with.
    pub const fn purpose(self) -> Purpose {
        match self {
            ChosenKind::Color => Purpose::Color,
            ChosenKind::CreatureType => Purpose::CreatureType,
            ChosenKind::CardName => Purpose::CardName,
            ChosenKind::Word => Purpose::ReplaceWord,
        }
    }

    /// The kind a question with `purpose` asks for, if it asks for a chosen value.
    pub const fn of_purpose(purpose: Purpose) -> Option<ChosenKind> {
        match purpose {
            Purpose::Color => Some(ChosenKind::Color),
            Purpose::CreatureType => Some(ChosenKind::CreatureType),
            Purpose::CardName => Some(ChosenKind::CardName),
            Purpose::ReplaceWord | Purpose::NewWord => Some(ChosenKind::Word),
            Purpose::Priority
            | Purpose::Target
            | Purpose::Choose
            | Purpose::Discard
            | Purpose::Cleanup
            | Purpose::PutOnBottom
            | Purpose::LegendRule
            | Purpose::Look
            | Purpose::Search
            | Purpose::Untap
            | Purpose::ManifestDread
            | Purpose::ExileCost
            | Purpose::CombatDamage
            | Purpose::TriggerOrder
            | Purpose::Scry
            | Purpose::Arrange
            | Purpose::PutBack
            | Purpose::May
            | Purpose::MaySearch
            | Purpose::Move
            | Purpose::Fateseal
            | Purpose::TopOrBottom
            | Purpose::Mutate
            | Purpose::Explore
            | Purpose::UnlessPays
            | Purpose::PayLife
            | Purpose::X
            | Purpose::PlayOrDraw
            | Purpose::Mulligan
            | Purpose::CastOffer
            | Purpose::Act
            | Purpose::DivideDamage
            | Purpose::DistributeCounters
            | Purpose::Mana
            | Purpose::Crew
            | Purpose::Waterbend
            | Purpose::Convoke
            | Purpose::Improvise
            | Purpose::Delve
            | Purpose::Attack
            | Purpose::Block
            | Purpose::EnterOrder
            | Purpose::PileOrder
            | Purpose::Replace
            | Purpose::Wish
            | Purpose::Companion => None,
        }
    }
}
