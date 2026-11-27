//! One template per [`Effect`] instruction: what it says, as the clauses of a spell or an
//! ability ([CR 608.2c](https://yawgatog.com/resources/magic-rules/#R6082c)): "The controller
//! of the spell or ability follows its instructions in the order written."
//!
//! [`Ctx::effect`] names every instruction once and hands it to the template for its kind:
//! instructions that hold others (here), moves by what happens to an object (`moves.rs`) and
//! by where it goes (`zones.rs`), what a player gets or loses (`resources.rs`), permissions
//! and rule changes (`permissions.rs`), copies (`copies.rs`), continuous effects
//! (`modify.rs`), damage and its prevention (`prevention.rs`), and turns (`phases.rs`).

use super::Ctx;
use super::clauses::prefix;
use crate::ability::Behavior;
use crate::continuous::Duration;
use crate::cost::Cost;
use crate::effect::{Effect, Objects, Players, Recipients};

impl Ctx {
    /// The clauses an effect says, each a sentence once capitalized and closed: "draw three
    /// cards", "put two cards from your hand on top of your library in any order".
    #[allow(clippy::too_many_lines)] // One exhaustive match, a line or two per instruction.
    pub(super) fn effect(&mut self, effect: &Effect) -> Vec<String> {
        match *effect {
            // Instructions that hold others.
            Effect::Sequence(effects) => self.sequence(effects),
            Effect::If {
                condition,
                then,
                otherwise,
            } => self.conditional(condition, then, otherwise),
            Effect::May { player, effect } => self.may(player, effect),
            Effect::Unless {
                effect,
                player,
                cost,
            } => self.unless_clauses(effect, player, cost),
            Effect::ForEachPlayer { players, effect } => self.for_each_player(players, effect),
            Effect::Choose {
                player,
                among,
                optional,
                ..
            } => self.choose(player, among, optional),
            Effect::ChooseValue { player, kind } => {
                let what = super::replacements::chosen_kind(kind);
                vec![self.clause(player, ("choose", "chooses"), |_| what.to_owned())]
            }
            Effect::ChooseAtRandom(among) => self.choose_at_random(among),
            // Spells on the stack.
            Effect::Counter(spells) => vec![self.acts(("counter", "counters"), spells, "")],
            Effect::CounterInto { spells, zone } => self.counter_into(spells, zone),
            Effect::CopySpell {
                spells,
                count,
                except,
                new_targets,
            } => self.copy_spell(spells, count, except, new_targets),
            // Permanents, where they are.
            Effect::Regenerate(objects) => {
                vec![self.acts(("regenerate", "regenerates"), objects, "")]
            }
            Effect::Tap(objects) => vec![format!("tap {}", self.objects(objects))],
            Effect::Untap(objects) => vec![self.acts(("untap", "untaps"), objects, "")],
            Effect::PhaseOut(objects) => vec![self.phase_out(objects)],
            Effect::SetPrepared { objects, prepared } => {
                vec![self.set_prepared(objects, prepared)]
            }
            Effect::Attach { objects, to } => {
                let host = self.objects(to);
                vec![self.acts(("attach", "attaches"), objects, &format!(" to {host}"))]
            }
            Effect::ExchangeControl(first, second) => {
                let (first, second) = (self.objects(first), self.objects(second));
                vec![format!("exchange control of {first} and {second}")]
            }
            Effect::Transform(objects) => vec![self.acts(("transform", "transforms"), objects, "")],
            // "Them" are this object and its partner, which the ability's condition named.
            Effect::Meld(_) => vec![format!(
                "exile them, then meld them into {}",
                self.meld.map_or("[its meld result]", |(_, into)| into)
            )],
            Effect::Flip(objects) => vec![self.acts(("flip", "flips"), objects, "")],
            Effect::Explore(objects) => {
                let verb = if self.is_plural(objects) {
                    "explore"
                } else {
                    "explores"
                };
                vec![format!("{} {verb}", self.objects(objects))]
            }
            // The reminder text of mutate, which no card prints as a line of its own.
            Effect::Mutate(objects) => vec![format!(
                "put this spell over or under {}. They mutate into the creature on top plus all \
                 abilities from under it",
                self.objects(objects)
            )],
            // Moves by what happens to an object.
            Effect::Destroy {
                objects,
                regeneration,
            } => self.destroy(objects, regeneration),
            Effect::Exile(objects) | Effect::ExileLinked(objects) => self.exile_clauses(objects),
            Effect::ExileUntilThisLeaves(objects) => {
                let until = format!(" until {} leaves the battlefield", self.this);
                vec![self.acts(("exile", "exiles"), objects, &until)]
            }
            // "That creature's controller sacrifices it": a delayed ability's permanent may be
            // another player's, who sacrifices it. CR 701.21a: "To sacrifice a permanent, its
            // controller moves it from the battlefield directly to its owner's graveyard."
            Effect::Sacrifice(Objects::Carried) => {
                vec![format!(
                    "that {}'s controller sacrifices it",
                    self.carried_noun()
                )]
            }
            Effect::Sacrifice(objects) => {
                vec![self.acts(("sacrifice", "sacrifices"), objects, "")]
            }
            Effect::ReturnToHand(objects) => self.return_to_hand(objects),
            Effect::PutIntoGraveyard(objects) => self.put_into_graveyard(objects),
            Effect::ReturnToBattlefield(objects) => self.return_to_battlefield(objects, false),
            Effect::ReturnTransformed(objects) => self.return_to_battlefield(objects, true),
            Effect::Search {
                player,
                filter,
                count,
                found,
            } => self.search(player, filter, count, found),
            Effect::Wish { filter } => self.wish(filter),
            // Moves by where an object goes, and looks at hidden cards.
            Effect::PutOntoBattlefield(onto) => vec![self.put_onto(onto)],
            Effect::PutIntoLibrary { objects, position } => {
                self.put_into_library(objects, position)
            }
            Effect::PutBack { player, count } => self.put_from_hand(player, Some(count)),
            Effect::PutAnyOnBottom { player } => self.put_from_hand(player, None),
            Effect::ShuffleInto { player, cards } => {
                vec![self.clause(player, ("shuffle", "shuffles"), |c| {
                    let cards = c.objects(cards);
                    format!("{cards} into {} library", c.possessive(player))
                })]
            }
            Effect::ExileUntil {
                player,
                matching,
                when_found,
            } => self.exile_until_clauses(player, matching, when_found),
            Effect::Dig {
                player,
                look,
                take,
                top,
                rest,
            } => self.dig(player, look, take, top, rest),
            Effect::Fateseal {
                player,
                library,
                count,
            } => self.fateseal(player, library, count),
            Effect::LookAt { player, cards } => {
                vec![self.clause(player, ("look", "looks"), |c| {
                    format!("at {}", c.objects(cards))
                })]
            }
            Effect::Reveal(objects) => vec![self.acts(("reveal", "reveals"), objects, "")],
            Effect::ManifestDread(player) => {
                vec![
                    self.clause(player, ("manifest dread", "manifests dread"), |_| {
                        String::new()
                    }),
                ]
            }
            // What a player gets, loses or counts out.
            Effect::Draw { player, count } => vec![self.cards(player, ("draw", "draws"), count)],
            Effect::Discard { player, count } => self.discard(player, count),
            Effect::Mill { player, count } => {
                let clause = self.cards(player, ("mill", "mills"), count);
                self.done_this_way = Some("milled");
                vec![clause]
            }
            Effect::Scry { player, count } => {
                vec![self.clause(player, ("scry", "scries"), |c| c.amount(count))]
            }
            Effect::FlipCoin(player) => {
                vec![self.clause(player, ("flip", "flips"), |_| "a coin".to_owned())]
            }
            Effect::GainLife { player, amount } => {
                vec![self.stated(player, ("gain", "gains"), |c| c.quantity(amount, "life"))]
            }
            Effect::LoseLife { player, amount } => {
                vec![self.stated(player, ("lose", "loses"), |c| c.quantity(amount, "life"))]
            }
            Effect::CreateTokens {
                player,
                token,
                count,
            } => vec![self.clause(player, ("create", "creates"), |c| c.tokens(token, count))],
            Effect::CreateAttackingTokens {
                player,
                token,
                count,
                attacking,
            } => self.create_attacking_tokens(player, token, count, attacking),
            Effect::AddCounters {
                objects,
                kind,
                count,
            } => self.counters(objects, kind, count, true),
            Effect::RemoveCounters {
                objects,
                kind,
                count,
            } => self.counters(objects, kind, count, false),
            Effect::RemoveAllCounters(objects) => self.remove_all_counters(objects),
            Effect::DoubleCounters { objects, kind } => self.double_counters(objects, kind),
            Effect::AddMana { player, mana } => self.add_mana(player, mana),
            Effect::AddProduct { player } => self.add_product(player),
            Effect::AddManaOfKind {
                player,
                kind,
                amount,
            } => self.add_mana_of_kind(player, kind, amount),
            Effect::UntapUpTo {
                player,
                count,
                permanents,
            } => self.untap_up_to(player, count, permanents),
            // Damage and its prevention.
            Effect::DealDamage { to, amount } => self.deal_damage(to, amount),
            Effect::PreventDamage {
                to,
                prevention,
                from,
            } => vec![self.prevention(to, prevention, from)],
            // Copies.
            Effect::CreateCopies {
                player,
                of,
                count,
                except,
            } => vec![self.create_copies(player, of, count, except)],
            Effect::BecomeCopy {
                objects,
                of,
                except,
            } => vec![self.become_copy(objects, of, except)],
            // Continuous effects, permissions and rule changes.
            Effect::Modify {
                objects,
                modifications,
                duration,
            } => {
                let text = self.modify(objects, modifications);
                vec![format!("{text}{}", duration_suffix(duration))]
            }
            Effect::ChangeText {
                objects,
                words,
                duration,
            } => vec![format!(
                "change the text of {} by replacing all instances of {}{}",
                self.objects(objects),
                words.phrase(),
                duration_suffix(duration)
            )],
            Effect::ChangeRule { rule, duration } => self.change_rule(rule, duration),
            Effect::ChangeMana { change, duration } => self.change_mana(change, duration),
            Effect::GainFlashback(cards) => self.gain_flashback(cards),
            Effect::PermitCast {
                player,
                cards,
                duration,
                terms,
            } => self.permit_cast(player, cards, duration, terms),
            Effect::MayCast {
                player,
                card,
                free,
                free_below,
                copy,
                face,
                within_x,
            } => self.may_cast(player, card, free, free_below, copy, face, within_x),
            // Turns, later events and the game.
            Effect::Delay { ability, .. } => self.delay(ability),
            Effect::GetEmblem { player, emblem } => self.get_emblem(player, emblem),
            Effect::WinGame(player) => {
                vec![self.stated(player, ("win", "wins"), |_| "the game".to_owned())]
            }
            Effect::LoseGame(player) => {
                vec![self.stated(player, ("lose", "loses"), |_| "the game".to_owned())]
            }
            Effect::AdditionalStep(step) => super::phases::additional_step(step),
            Effect::AdditionalPhases { after, phases } => {
                super::phases::additional_phases(after, phases)
            }
            Effect::EndTurn | Effect::RestartGame { .. } => self.end_or_restart(effect),
            Effect::ExtraTurn(player, ability) => self.extra_turn(player, ability),
            Effect::ControlPlayer(player) => self.control_player(player),
            Effect::Subgame => vec![
                "players play a Magic subgame, using their libraries as their decks".to_owned(),
            ],
        }
    }

    /// "You may draw a card", or "you may have target player draw a card" where the
    /// instruction's own clause did not take the "may".
    fn may(&mut self, player: Players, effect: &Effect) -> Vec<String> {
        self.may = true;
        let mut clauses = self.effect(effect);
        if std::mem::take(&mut self.may) {
            let who = self.player(player);
            prefix(&mut clauses, &format!("{who} may have "));
        }
        clauses
    }

    /// "Counter target spell unless its controller pays {3}".
    fn unless_clauses(&mut self, effect: &Effect, player: Players, cost: Cost) -> Vec<String> {
        let mut clauses = self.effect(effect);
        let unless = self.unless(player, cost);
        match clauses.first_mut() {
            Some(first) => {
                first.push_str(" unless ");
                first.push_str(&unless);
            }
            None => clauses.push(format!("unless {unless}")),
        }
        clauses
    }

    /// "Choose a creature you control", which later instructions name as "it".
    fn choose(&mut self, player: Players, among: Objects, optional: bool) -> Vec<String> {
        self.may |= optional;
        let clause = self.clause(player, ("choose", "chooses"), |c| c.one_of(among));
        self.plural = false;
        vec![clause]
    }

    /// "Choose a creature card in your graveyard at random", which later instructions name as
    /// "it".
    fn choose_at_random(&mut self, among: Objects) -> Vec<String> {
        let clause = self.clause(Players::You, ("choose", "chooses"), |c| {
            format!("{} at random", c.one_of(among))
        });
        self.plural = false;
        vec![clause]
    }

    /// "This creature deals 3 damage to any target", or "deals 2 damage divided as you choose
    /// among one or two targets".
    fn deal_damage(&mut self, to: Recipients, amount: crate::Amount) -> Vec<String> {
        let damage = self.quantity(amount, "damage");
        if matches!(to, Recipients::DividedTwo(_)) {
            return vec![format!(
                "{} deals {damage} divided as you choose among one or two targets",
                self.this_word()
            )];
        }
        let to = self.recipients(to);
        vec![format!("{} deals {damage} to {to}", self.this_word())]
    }
}

/// How long a continuous effect from a resolving spell lasts, after what it does
/// ([CR 611.2a](https://yawgatog.com/resources/magic-rules/#R6112a)): "until end of turn".
pub(super) fn duration_suffix(duration: Duration) -> &'static str {
    match duration {
        Duration::EndOfTurn => " until end of turn",
        Duration::UntilYourNextTurn => " until your next turn",
        Duration::Indefinite => "",
    }
}

/// Whether a behavior is effect data that says nothing: an Aura's spell, which only
/// targets.
pub(super) fn says_nothing(body: Behavior) -> bool {
    matches!(body, Behavior::Data(Effect::Sequence([])))
}
