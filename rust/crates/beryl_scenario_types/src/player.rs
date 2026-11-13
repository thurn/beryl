//! Shorthands that read as sentences about one player: `ME.casts(card)`, `ME.yes()`,
//! `THEM.has(Zone::Hand, card, 1)`. Each builds an [`Action`] the long way would spell out as
//! an `Action::answer(..)` or an `Action::assert(Assertion::..)` struct literal.

use beryl_card::{CounterKind, DecisionKind, ManaColor, Seat, TurnStep, Zone};

use crate::{Action, Assertion, Ending, Pick};

/// One player of a scenario, for the shorthands below.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Player(pub Seat);

/// Player 0, whose card the scenario is usually about.
pub const ME: Player = Player(Seat::P0);
/// Player 1.
pub const THEM: Player = Player(Seat::P1);

/// The top of the stack resolves: see [`Action::Resolve`].
pub use Action::Resolve;

/// Every player passes priority until the game reaches `step` of turn `turn`: see
/// [`Action::AdvanceTo`].
pub const fn advance_to(turn: u16, step: TurnStep) -> Action {
    Action::AdvanceTo { turn, step }
}

/// The open decision is the end of the game, which `player` has won.
pub const fn wins(player: Player) -> Action {
    Action::assert(Assertion::Outcome {
        outcome: Ending::Win { player: player.0 },
    })
}

/// How many spells and abilities are on the stack.
pub const fn stack(count: u8) -> Action {
    Action::assert(Assertion::Stack { count })
}

/// Decisions and actions.
impl Player {
    /// The seat, for the builder methods that take one.
    pub const fn seat(self) -> Seat {
        self.0
    }

    /// Answers a decision of `kind` with `pick`.
    pub const fn answers(self, kind: DecisionKind, pick: Pick) -> Action {
        Action::answer(self.0, kind, pick)
    }

    /// Chooses the card named `card` for a decision of `kind`.
    pub const fn chooses(self, kind: DecisionKind, card: &'static str) -> Action {
        Action::choose(self.0, kind, card)
    }

    /// Casts `card`'s front face with none of its casting options.
    pub const fn casts(self, card: &'static str) -> Action {
        Action::cast(self.0, card)
    }

    /// Plays the land `card` from their hand.
    pub const fn plays(self, card: &'static str) -> Action {
        Action::PlayLand {
            player: self.0,
            card,
        }
    }

    /// Activates ability `ability` (an index into the face's abilities, from 0) of `card`.
    pub const fn activates(self, card: &'static str, ability: u8) -> Action {
        Action::activate(self.0, card, ability)
    }

    /// Passes priority.
    pub const fn passes(self) -> Action {
        Action::pass(self.0)
    }

    /// Answers a yes-or-no question with yes.
    pub const fn yes(self) -> Action {
        self.answers(DecisionKind::YesNo, Pick::Yes)
    }

    /// Answers a yes-or-no question with no.
    pub const fn no(self) -> Action {
        self.answers(DecisionKind::YesNo, Pick::No)
    }

    /// Chooses the card named `card` where one object is chosen.
    pub const fn picks(self, card: &'static str) -> Action {
        self.chooses(DecisionKind::ChooseObject, card)
    }

    /// Chooses the card named `card` where any number up to a limit are chosen.
    pub const fn picks_up_to(self, card: &'static str) -> Action {
        self.chooses(DecisionKind::ChooseUpTo, card)
    }

    /// Finishes choosing "up to" a number of objects.
    pub const fn done_picking(self) -> Action {
        self.answers(DecisionKind::ChooseUpTo, Pick::Done)
    }

    /// Targets the card named `card`.
    pub const fn targets(self, card: &'static str) -> Action {
        self.chooses(DecisionKind::Target, card)
    }

    /// Targets the player `player`.
    pub const fn targets_player(self, player: Player) -> Action {
        self.answers(DecisionKind::Target, Pick::Player { player: player.0 })
    }

    /// Chooses how many targets.
    pub const fn target_count(self, n: u8) -> Action {
        self.answers(DecisionKind::TargetCount, Pick::Number { n })
    }

    /// Chooses the value of X.
    pub const fn x(self, n: u8) -> Action {
        self.answers(DecisionKind::X, Pick::Number { n })
    }

    /// Declares the creature named `card` as an attacker.
    pub const fn attacks_with(self, card: &'static str) -> Action {
        self.chooses(DecisionKind::Attacker, card)
    }

    /// Finishes declaring attackers.
    pub const fn done_attacking(self) -> Action {
        self.answers(DecisionKind::Attacker, Pick::Done)
    }
}

/// Assertions about this player's cards and life.
impl Player {
    /// They own `count` cards named `card` in `zone`.
    pub const fn has(self, zone: Zone, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player: self.0,
            zone,
            card: Some(card),
            count,
        })
    }

    /// They own `count` cards in `zone`, whatever their names.
    pub const fn has_cards(self, zone: Zone, count: u8) -> Action {
        Action::assert(Assertion::Count {
            player: self.0,
            zone,
            card: None,
            count,
        })
    }

    /// They control `count` permanents named `card`.
    pub const fn controls(self, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Controls {
            player: self.0,
            card,
            count,
        })
    }

    /// `count` of the permanents named `card` they control are tapped.
    pub const fn tapped(self, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Tapped {
            player: self.0,
            card,
            count,
        })
    }

    /// `count` of the permanents named `card` they control are face down.
    pub const fn face_down(self, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::FaceDown {
            player: self.0,
            card,
            count,
        })
    }

    /// `count` of the permanents named `card` they control are attacking.
    pub const fn attacking(self, card: &'static str, count: u8) -> Action {
        Action::assert(Assertion::Attacking {
            player: self.0,
            card,
            count,
        })
    }

    /// Their life total is `life`.
    pub const fn life(self, life: i16) -> Action {
        Action::assert(Assertion::Life {
            player: self.0,
            life,
        })
    }

    /// The top cards of their library, top first.
    pub const fn library_top(self, top: &'static [&'static str]) -> Action {
        Action::assert(Assertion::Library {
            player: self.0,
            top,
        })
    }

    /// The first permanent named `card` they control, a card, has this power and toughness.
    pub const fn pt(self, card: &'static str, power: i8, toughness: i8) -> Action {
        Action::assert(Assertion::Pt {
            player: self.0,
            card,
            token: false,
            power,
            toughness,
        })
    }

    /// The first token named `card` they control has this power and toughness.
    pub const fn token_pt(self, card: &'static str, power: i8, toughness: i8) -> Action {
        Action::assert(Assertion::Pt {
            player: self.0,
            card,
            token: true,
            power,
            toughness,
        })
    }

    /// Whether the first permanent named `card` they control is a creature.
    pub const fn creature(self, card: &'static str, creature: bool) -> Action {
        Action::assert(Assertion::Creature {
            player: self.0,
            card,
            creature,
        })
    }

    /// The first permanent named `card` they control has `count` counters of `kind`.
    pub const fn counters(self, card: &'static str, kind: CounterKind, count: u8) -> Action {
        Action::assert(Assertion::Counters {
            player: self.0,
            card,
            kind,
            count,
        })
    }

    /// The first permanent named `card` they control has `count` loyalty counters. (Not
    /// `loyalty`: the card build refuses `.loyalty(` in a card file, which would type a
    /// printed characteristic.)
    pub const fn has_loyalty(self, card: &'static str, count: u8) -> Action {
        self.counters(card, CounterKind::Loyalty, count)
    }

    /// They have `amount` unspent mana of `color`.
    pub const fn mana(self, color: ManaColor, amount: u8) -> Action {
        Action::assert(Assertion::Mana {
            player: self.0,
            color,
            amount,
        })
    }

    /// The open decision is theirs, of `kind`, among `candidates` candidates.
    pub const fn deciding(self, kind: DecisionKind, candidates: u8) -> Action {
        Action::assert(Assertion::Decision {
            player: self.0,
            kind,
            candidates,
        })
    }
}
