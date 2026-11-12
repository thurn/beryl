//! Carrying out effect data's instructions. [`Run::carry_out`] takes apart those that hold
//! others, and [`Run::instruction`] is the table of the rest: an entry's arm is in its
//! library function's module, named `instruction` (the crate's contract, "Modules").
//!
//! An instruction acts on every object it names at once where the engine has a primitive
//! that does, and otherwise on each permanent in the order they entered the battlefield, an
//! order the game can see (slots are none: the deal puts cards in them at random).

use beryl_card::{Effect, Objects, Players};

use crate::actions::{
    counter, destroy, discard, exile, explore, fateseal, manifest_dread, mill, sacrifice, scry,
    search, shuffle, tap, transform,
};
use crate::bitboard::Bitboard;
use crate::card::{AbilityIx, FaceAbility, Flow, Stop};
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::scene::{Outcome, Registers, Scene};
use crate::keywords::flashback;
use crate::layout::{Layout, Queue, Table};
use crate::resolution::Resolution;
use crate::state::{ObjectRef, PendingTrigger, State, Zone, placed};
use crate::zones;
use crate::{casting, choices, coins, continuous, costs, counters, damage, drawing, ending};
use crate::{delayed_triggers, emblems, layers, library, life, linked, mana, one_shot, tokens};

/// One resolution of an ability written as data: the game, what is resolving in it, and
/// what it keeps between instructions.
pub(crate) struct Run<'c, 'a, L: Layout> {
    game: &'c mut Game<'a, L>,
    resolving: &'c mut Resolution<'a, L>,
    subject: PendingTrigger<L>,
    registers: Registers<L>,
}

impl<'c, 'a, L: Layout> Run<'c, 'a, L> {
    /// Begins resolving the ability `resolving` is of, in `game`.
    pub(crate) fn new(game: &'c mut Game<'a, L>, resolving: &'c mut Resolution<'a, L>) -> Self {
        let legal = resolving.legal;
        let entry = &resolving.entry;
        let subject = PendingTrigger {
            source: entry.object,
            card: entry.card,
            face: entry.face,
            ability: match entry.ability {
                Some(FaceAbility::Own(index)) => index,
                _ => AbilityIx::default(),
            },
            controller: entry.controller,
            info: entry.info,
        };
        Run {
            game,
            resolving,
            subject,
            registers: Registers {
                legal,
                ..Registers::default()
            },
        }
    }

    /// The game the ability resolves in, to change.
    pub(crate) fn game(&mut self) -> &mut Game<'a, L> {
        &mut *self.game
    }

    /// The game and the resolution, for a library function that asks.
    pub(crate) fn resolving(&mut self) -> (&mut Game<'a, L>, &mut Resolution<'a, L>) {
        (&mut *self.game, &mut *self.resolving)
    }

    /// What is resolving, to read.
    pub(crate) fn resolution(&self) -> &Resolution<'a, L> {
        &*self.resolving
    }

    /// The state as it is now, to read.
    pub(crate) fn state(&self) -> &State<L> {
        self.game.state
    }

    /// What the ability's expressions are read against, as the game is now.
    pub(crate) fn scene(&self) -> Scene<'_, L> {
        let entry = &self.resolving.entry;
        Scene {
            s: self.game.state,
            t: self.game.table,
            subject: &self.subject,
            costs: entry.is_spell().then_some(entry.costs),
            x: entry.x,
            targets: entry.targets.as_slice(),
            registers: &self.registers,
        }
    }

    /// Carries out one instruction, or each of several in order
    /// ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller of
    /// the spell or ability follows its instructions in the order written."
    pub(crate) fn carry_out(&mut self, effect: &'static Effect) -> Flow {
        // For the bar of a library entry: one hand-written scenario carries it out (plan K3).
        crate::coverage::hit(effect);
        self.registers.outcome = match *effect {
            Effect::Sequence(effects) => {
                for effect in effects {
                    self.carry_out(effect)?;
                }
                return Ok(());
            }
            // CR 608.2h: "the answer is determined only once, when the effect is applied."
            Effect::If {
                condition,
                then,
                otherwise,
            } => {
                let branch = if self.scene().holds(&condition) {
                    Some(then)
                } else {
                    otherwise
                };
                return self.carry_out_or_nothing(branch);
            }
            // CR 608.2d: "If an effect of a spell or ability offers any choices ..., the
            // player announces these while applying the effect."
            Effect::May { player, effect } => {
                return choices::may_instruction(self, player, effect);
            }
            Effect::Unless {
                effect,
                player,
                cost,
            } => return costs::unless_instruction(self, effect, player, cost),
            Effect::ForEachPlayer { players, effect } => {
                return self.for_each_player(players, effect);
            }
            _ => self.instruction(effect)?,
        };
        Ok(())
    }

    /// Carries out one instruction that holds no other: the table of the entries, each
    /// naming its arm. The outcome is what the instruction after reads.
    #[inline]
    #[allow(clippy::too_many_lines)] // A line or five per entry.
    fn instruction(&mut self, effect: &'static Effect) -> Result<Outcome<L>, Stop> {
        Ok(match *effect {
            Effect::Sequence(_)
            | Effect::If { .. }
            | Effect::May { .. }
            | Effect::Unless { .. }
            | Effect::ForEachPlayer { .. } => unreachable!("`carry_out` takes these apart"),
            Effect::Choose {
                player,
                among,
                prompt,
                optional,
            } => choices::choose_instruction(self, player, among, prompt, optional)?,
            Effect::LookAt { player, cards } => zones::look_at_instruction(self, player, cards),
            Effect::Counter(objects) => counter::instruction(self, objects)?,
            Effect::CounterInto { spells, zone } => counter::into_instruction(self, spells, zone)?,
            Effect::Destroy(objects) => destroy::instruction(self, objects)?,
            Effect::Exile(objects) => exile::instruction(self, objects)?,
            Effect::ExileUntilThisLeaves(objects) => {
                one_shot::exile_until_this_leaves_instruction(self, objects)?
            }
            Effect::ExileLinked(cards) => linked::exile_instruction(self, cards)?,
            Effect::Sacrifice(objects) => sacrifice::instruction(self, objects)?,
            Effect::ReturnToHand(objects) => zones::return_to_hand_instruction(self, objects)?,
            Effect::PutIntoGraveyard(objects) => {
                zones::put_into_graveyard_instruction(self, objects)?
            }
            Effect::ReturnTransformed(cards) => transform::return_instruction(self, cards)?,
            Effect::PutIntoLibrary { objects, position } => {
                library::put_instruction(self, objects, position)?
            }
            Effect::PutBack { player, count } => {
                library::put_back_instruction(self, player, count)?
            }
            Effect::ShuffleInto { player, cards } => {
                shuffle::into_instruction(self, player, cards)?
            }
            Effect::Dig {
                player,
                look,
                take,
                rest,
            } => library::dig_instruction(self, player, look, take, rest)?,
            Effect::Fateseal {
                player,
                library,
                count,
            } => fateseal::instruction(self, player, library, count)?,
            Effect::Search {
                player,
                filter,
                count,
                found,
            } => search::instruction(self, player, filter, count, found)?,
            Effect::Draw { player, count } => drawing::instruction(self, player, count)?,
            Effect::Discard { player, count } => discard::instruction(self, player, count)?,
            Effect::Mill { player, count } => mill::instruction(self, player, count)?,
            Effect::Scry { player, count } => scry::instruction(self, player, count)?,
            Effect::Explore(objects) => explore::instruction(self, objects)?,
            Effect::ManifestDread(players) => manifest_dread::instruction(self, players)?,
            Effect::FlipCoin(players) => coins::instruction(self, players)?,
            Effect::DealDamage { to, amount } => damage::instruction(self, to, amount)?,
            Effect::GainLife { player, amount } => life::gain_instruction(self, player, amount)?,
            Effect::LoseLife { player, amount } => life::lose_instruction(self, player, amount)?,
            Effect::CreateTokens {
                player,
                token,
                count,
            } => tokens::create_instruction(self, player, token, count)?,
            Effect::CreateAttackingTokens {
                player,
                token,
                count,
                attacking,
            } => tokens::create_attacking_instruction(self, player, token, count, attacking)?,
            Effect::CreateCopies {
                player,
                of,
                count,
                except,
            } => tokens::create_copies_instruction(self, player, of, count, except)?,
            Effect::AddCounters {
                objects,
                kind,
                count,
            } => counters::add_instruction(self, objects, kind, count)?,
            Effect::RemoveCounters {
                objects,
                kind,
                count,
            } => counters::remove_instruction(self, objects, kind, count)?,
            Effect::AddMana { player, mana } => mana::add_instruction(self, player, mana),
            Effect::AddManaOfKind {
                player,
                kind,
                amount,
            } => mana::add_of_kind_instruction(self, player, kind, amount),
            Effect::Modify {
                objects,
                modification,
                duration,
            } => continuous::instruction(self, objects, modification, duration)?,
            Effect::ChangeRule { rule, duration } => {
                continuous::change_rule_instruction(self, rule, duration)?
            }
            Effect::GainFlashback(cards) => flashback::instruction(self, cards)?,
            Effect::PermitCast {
                player,
                cards,
                duration,
                terms,
            } => casting::permit_instruction(self, player, cards, duration, terms)?,
            Effect::MayCast { player, card, free } => {
                casting::may_cast_instruction(self, player, card, free)
            }
            Effect::UntapUpTo {
                player,
                count,
                permanents,
            } => tap::untap_up_to_instruction(self, player, count, permanents)?,
            Effect::Delay { ability, carrying } => {
                delayed_triggers::instruction(self, ability, carrying)?
            }
            Effect::GetEmblem { player, emblem } => emblems::instruction(self, player, emblem)?,
            Effect::WinGame(players) => ending::win_instruction(self, players),
            Effect::LoseGame(players) => ending::lose_instruction(self, players),
        })
    }

    /// Carries out the effect an [`Effect::If`], [`Effect::May`] or [`Effect::Unless`] came
    /// to, whose outcome is then its own; one that came to nothing did nothing.
    pub(crate) fn carry_out_or_nothing(&mut self, effect: Option<&'static Effect>) -> Flow {
        let Some(effect) = effect else {
            self.registers.outcome = Outcome::NOTHING;
            return Ok(());
        };
        self.carry_out(effect)
    }

    /// Carries out `effect` for each of `players` in turn order (CR 101.4), each of them
    /// "that player" in it. Each begins from what the instruction before this one did, and
    /// the outcome is every player's together, as is who controlled what it moved.
    fn for_each_player(&mut self, players: Players, effect: &'static Effect) -> Flow {
        let (before, outer) = (self.registers.outcome, self.registers.that_player);
        let mut together = Outcome::NOTHING;
        let mut controlled = self.registers.controlled.clone();
        for player in self.in_turn_order(players) {
            self.registers.that_player = Some(player);
            self.registers.outcome = before;
            self.carry_out(effect)?;
            together = together.with(self.registers.outcome);
            let theirs = &self.registers.controlled;
            controlled = Table::from_fn(|player| controlled[player] | theirs[player]);
        }
        self.registers.that_player = outer;
        self.registers.outcome = together;
        self.registers.controlled = controlled;
        Ok(())
    }

    /// The players `players` names, in the order they act (CR 101.4).
    pub(crate) fn in_turn_order(&self, players: Players) -> impl Iterator<Item = Seat> + use<L> {
        let who = self.scene().players(players);
        self.game.state.players_in_turn_order(who)
    }

    /// The one player who makes a choice the effect offers `players`.
    pub(crate) fn one(&self, players: Players) -> Option<Seat> {
        self.in_turn_order(players).next()
    }

    /// The permanents among the objects `objects` names
    /// ([CR 110.1](https://yawgatog.com/resources/magic-rules/#R1101)): "A permanent is a
    /// card or token on the battlefield."
    pub(crate) fn permanents(&self, objects: Objects) -> Bitboard<L::Slot> {
        let s = &*self.game.state;
        (self.scene().objects(objects)).filter(|slot| s.object(slot).zone() == Zone::Battlefield)
    }

    /// The objects `objects` names that are not in `zone` already.
    pub(crate) fn elsewhere_than(&self, objects: Objects, zone: Zone) -> Bitboard<L::Slot> {
        let s = &*self.game.state;
        (self.scene().objects(objects)).filter(|slot| s.object(slot).zone() != zone)
    }

    /// Notes who controls each of `leaving`, and what is known of the targets among them,
    /// before the effect moves them, for the instructions after (CR 608.2h): "the effect
    /// uses the object's last known information."
    ///
    /// A controller is read through the layers, which an instruction before may have
    /// changed, so they are brought up to date first, as the move itself would.
    pub(crate) fn remember(&mut self, leaving: Bitboard<L::Slot>) -> Result<(), Full> {
        let game = &mut *self.game;
        layers::refresh(game.state, game.table, game.sink)?;
        let s = &*self.game.state;
        self.registers.controlled =
            Table::from_fn(|player| leaving.filter(|slot| s.controller(slot).index() == player));
        for (place, target) in placed(self.resolving.entry.targets.as_slice()) {
            let Some(object) = target.as_object() else {
                continue;
            };
            if self.registers.legal.contains(place)
                && self.game.state.is_current(object)
                && leaving.contains(object.slot)
            {
                self.registers.known[place.index()] = self.scene().known(object.slot);
                self.registers.noted.insert(place);
            }
        }
        Ok(())
    }

    /// The outcome of an instruction that moved `permanents` off the battlefield: those that
    /// left it, as they are now
    /// ([CR 400.7j](https://yawgatog.com/resources/magic-rules/#R4007j)): "If an effect
    /// causes an object to move to a public zone, other parts of that effect can find that
    /// object."
    pub(crate) fn left_the_battlefield(&self, permanents: Bitboard<L::Slot>) -> Outcome<L> {
        let s = &*self.game.state;
        Outcome::acted_on(permanents.filter(|slot| s.object(slot).zone() != Zone::Battlefield))
    }

    /// The first of `permanents`, in the order they entered the battlefield, that is still
    /// one, taken out of the set: how an instruction acts on each of several in an order the
    /// game can see, whatever the ones before did.
    pub(crate) fn next_permanent(
        &self,
        permanents: &mut Bitboard<L::Slot>,
    ) -> Option<ObjectRef<L>> {
        let s = self.state();
        let &slot = (s.battlefield().iter()).find(|&&slot| permanents.contains(slot))?;
        permanents.remove(slot);
        Some(s.object_ref(slot))
    }

    /// The outcome of an instruction that moved `objects` to `zone`: those that are there.
    pub(crate) fn now_in(&self, objects: Bitboard<L::Slot>, zone: Zone) -> Outcome<L> {
        let s = &*self.game.state;
        Outcome::acted_on(objects.filter(|slot| s.object(slot).zone() == zone))
    }
}

/// Stops on an entry the engine has no primitive for: no card can come to depend on one
/// before the engine can do it.
#[cold]
pub(crate) fn no_primitive(entry: &dyn std::fmt::Debug) -> ! {
    unimplemented!("the engine cannot do {entry:?} yet")
}
