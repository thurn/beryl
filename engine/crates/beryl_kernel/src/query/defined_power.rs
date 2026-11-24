//! Characteristic-defining abilities off the battlefield.
//!
//! Implements: CR 604.3
//!
//! A characteristic-defining ability works in every zone ([CR 604.3]): "Characteristic-defining
//! abilities function in all zones. They also function outside the game and before the game
//! begins." On the battlefield the layers apply it in layer 7a (`beryl_rules`' `layers`). A
//! card in a graveyard, hand, library or exile, or a spell on the stack, has no layer results,
//! so a read of its power or toughness evaluates the ability here: Tarmogoyf in a graveyard
//! counts the card types among cards in all graveyards, itself included.
//!
//! The card table admits a "power is equal to [amount]" only as a characteristic-defining
//! ability (`static_ability::read_amounts`), and only amounts the layers can read as they
//! apply, so this module evaluates exactly those. A game with no such card pays one test of
//! [`Capability::DefinesPt`] for each read off the battlefield.
//!
//! [CR 604.3]: https://yawgatog.com/resources/magic-rules/#R6043

use beryl_card::{Amount, Modification, Objects, Types, Words, Zone};

use crate::filter::Evaluate;
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::State;
use crate::table::{Capability, GameTable};

impl<L: Layout> State<L> {
    /// The power and toughness of the object in `slot`, which is not a permanent: its
    /// copiable values ([`State::copiable_pt`]), then what its characteristic-defining
    /// abilities define (see [CR 613.4a]): "Layer 7a: Effects from characteristic-defining
    /// abilities that define power and/or toughness are applied." The effects that reach other
    /// zones change no power ([`crate::query::zone_characteristics`]), so nothing else applies. One test while
    /// no card of the game has such an ability.
    ///
    /// [CR 613.4a]: https://yawgatog.com/resources/magic-rules/#R6134a
    #[inline]
    pub(crate) fn pt_elsewhere(&self, t: &GameTable, slot: L::Slot) -> (i8, i8) {
        if t.can(Capability::DefinesPt) {
            self.defined_pt(t, slot)
        } else {
            self.copiable_pt(t, slot)
        }
    }

    /// [`State::pt_elsewhere`] in a game with a card that defines its power. Out of line: a
    /// game comes here only with such a card in it.
    #[cold]
    #[inline(never)]
    fn defined_pt(&self, t: &GameTable, slot: L::Slot) -> (i8, i8) {
        let (mut power, mut toughness) = self.copiable_pt(t, slot);
        let values = self.copiable(slot);
        if values.omits_pt_definitions(t) {
            return (power, toughness);
        }
        // see CR 108.4a: "If anything asks for the controller of a card that doesn't have one
        // (because it's not a permanent or spell), use its owner instead." The query answers
        // that, and a count of "creatures you control" counts its owner's.
        let you = self.controller(slot);
        let words = self.words_of(slot);
        let number = |amount| i8::try_from(self.defined(t, amount, you, &words)).unwrap_or(i8::MAX);
        for ability in values.face(t).abilities {
            let Some(spec) = ability.layer() else {
                continue;
            };
            // The table admits these only on the object itself with no condition.
            for modification in spec.modifications {
                match *modification {
                    Modification::SetPower(amount) => power = number(amount),
                    Modification::SetPt(p, tough) => {
                        (power, toughness) = (number(p), number(tough));
                    }
                    Modification::Control
                    | Modification::AddTypes(_)
                    | Modification::RemoveTypes(_)
                    | Modification::SetTypes(_)
                    | Modification::AddSubtypeWord(_)
                    | Modification::SetSubtypeWord(_)
                    | Modification::SetLandTypeWords(_)
                    | Modification::AddKeywords(_)
                    | Modification::Grant(_)
                    | Modification::ModifyPt(..)
                    | Modification::LoseAllAbilities
                    | Modification::LoseAbility(_)
                    | Modification::SetColorWords(_)
                    | Modification::AddColorWords(_)
                    | Modification::AddChosenColor
                    | Modification::SetBasePt(..)
                    | Modification::SwitchPt
                    | Modification::AddProtection(_)
                    | Modification::GrantEscape => {}
                }
            }
        }
        (power, toughness)
    }

    /// The value of `amount`, which a characteristic-defining ability of an object off the
    /// battlefield reads for `you`, its owner, through `words`, its text changes: the amounts
    /// `static_ability::read_amounts` admits. A count of permanents reads their layer results,
    /// as they stand.
    fn defined(&self, t: &GameTable, amount: &Amount, you: Seat, words: &Words) -> usize {
        match *amount {
            Amount::Fixed(n) => usize::from(n),
            Amount::Times(n, amount) => {
                usize::from(n).saturating_mul(self.defined(t, amount, you, words))
            }
            Amount::Plus(n, amount) => self
                .defined(t, amount, you, words)
                .saturating_add(usize::from(n)),
            Amount::Half { of, round_up } => {
                let whole = self.defined(t, of, you, words);
                if round_up {
                    whole.div_ceil(2)
                } else {
                    whole / 2
                }
            }
            Amount::Count(&Objects::All {
                zone: Zone::Battlefield,
                filter,
            }) => (self.battlefield().iter())
                .filter(|&&slot| filter.matches(self, t, slot, you, words))
                .count(),
            Amount::CardTypesAmong(_) => self.card_types_in_graveyards(t),
            Amount::X
            | Amount::ThatMany
            | Amount::Carried
            | Amount::ThoseOf(_)
            | Amount::Count(_)
            | Amount::Power(_)
            | Amount::Toughness(_)
            | Amount::ManaValue(_)
            | Amount::Counters(..)
            | Amount::Life(_)
            | Amount::Most(..)
            | Amount::SpellsCast(_)
            | Amount::CardsDrawn(_)
            | Amount::SpellsCastBefore => unreachable!(
                "the card table refuses an amount a characteristic-defining ability cannot read"
            ),
        }
    }

    /// The number of card types among the cards in all graveyards (see [CR 300.1]): "The card
    /// types are artifact, battle, conspiracy, creature, dungeon, enchantment, instant,
    /// kindred, land, phenomenon, plane, planeswalker, scheme, sorcery, and vanguard." Each type
    /// counts once, however many cards have it. A token in a graveyard is not a card
    /// (see [CR 108.2]) and counts for nothing in the moment before it ceases to exist
    /// (see [CR 111.7]): "A token that's in a zone other than the battlefield ceases to exist." The
    /// player each graveyard belongs to is nothing to the count, so they are read in seat
    /// order. The layers read it too, for a permanent's characteristic-defining ability.
    ///
    /// [CR 300.1]: https://yawgatog.com/resources/magic-rules/#R3001
    /// [CR 108.2]: https://yawgatog.com/resources/magic-rules/#R1082
    /// [CR 111.7]: https://yawgatog.com/resources/magic-rules/#R1117
    pub fn card_types_in_graveyards(&self, t: &GameTable) -> usize {
        let mut types = Types::NONE;
        for player in self.players().in_seat_order() {
            for &slot in self.graveyard(player) {
                if !self.object(slot).is_token() {
                    types = types.union(self.definition(slot).face(t).types);
                }
            }
        }
        usize::try_from(types.0.count_ones()).expect("a bit count fits in usize")
    }
}
