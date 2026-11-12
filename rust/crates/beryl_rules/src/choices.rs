//! Choices made while a spell or ability resolves
//! ([CR 608.2d](https://yawgatog.com/resources/magic-rules/#R6082d)): "If an effect of a
//! spell or ability offers any choices other than choices already made as part of casting the
//! spell, activating the ability, or otherwise putting the spell or ability on the stack, the
//! player announces these while applying the effect."
//!
//! A choice is put to its player as a prompt ([`beryl_card::prompt`]): a [`Prompt`] says what
//! it is for and, by its purpose, the [`Shape`] it is answered in. This module has the
//! functions that ask, by shape, and it is the one place in the rules where a resolution's
//! question becomes a [`Decision`]. A library function or a card's body builds the prompt and
//! calls one of these; it names no kind of decision.
//!
//! | Shape | Function | What one call asks |
//! | --- | --- | --- |
//! | [`Shape::YesNo`] | [`yes_no`] | The yes or no. Also the one that opens another shape's run ([`Part::Whether`], [`Part::Instead`]). |
//! | [`Shape::Pick`] | [`pick`] | One object, where one must be taken. |
//! | | [`pick_or_end`] | One more object, or the end of the picks. |
//! | | [`pick_up_to`] | A whole run: up to a number of objects. |
//! | | [`may_pick`] | A whole run: one object, which may be declined. |
//! | [`Shape::Split`] | [`pick`], [`pick_or_end`] | A card for the destination the prompt's part names; the second way where the first destination's picks offer an end. |
//! | [`Shape::Pay`] | [`pay`] | One step of a payment, or the whole of one the engine worked out. |
//! | [`Shape::Declare`] | [`aim`] | What objects already chosen are aimed at. |
//!
//! No resolution asks in the other shapes yet (an order, a number, a mode, a catalog entry, a
//! division): the first that does adds its function here. The cast a resolution offers is a
//! mode, but the engine asks it once the resolution has ended ([`crate::casting::offer`]).
//!
//! In debug builds each function checks that the prompt it is given is one its question can
//! answer, so a purpose asked through the wrong function fails where it is written.
//!
//! # An effect that asks more than once
//!
//! A function here returns its answer, or stops the resolution with [`Stop::Pause`] when the
//! player has not given one. The caller passes that on with `?` and does nothing else about
//! it. Everything the resolution did is then thrown away, and once the player has answered
//! it runs again from its first instruction, on the game as it was, with every answer so far
//! (architecture §7.3). The record is the answers alone, in the order they were asked: the
//! third question of a run is answered by the third answer, whatever it is about. So an
//! effect that needs several answers follows one pattern:
//!
//! - **Ask in an order the game fixes.** Which question comes next, who is asked and among
//!   what may depend on the game and on the answers before, and on nothing else. Go through
//!   players in turn order and objects in an order the game keeps
//!   ([`State::in_candidate_order`](crate::state::State::in_candidate_order), the order
//!   permanents entered); these functions order their own candidates.
//! - **Do, then ask.** Carry out what an answer decides before asking the next question: a
//!   card picked for the bottom of a library goes there, and then the next is picked. The
//!   player answers looking at the game as it stands at their question, and the run after
//!   comes to the same question by doing the same things.
//! - **One prompt for one run.** The questions of a run are the picks of one prompt: build
//!   it once, with its purpose, count and places, and give each pick its part and what is
//!   still to take ([`Prompt::most`]). A dig is a [`Shape::Split`]: a [`pick`] at
//!   [`Part::First`] for each card kept, then one at [`Part::Second`] for each of the rest in
//!   the order they go. A put-back is a split's first part alone, and a search is a
//!   [`pick_up_to`] ([`crate::actions::search`]).
//! - **Leave the single answers in.** A question with one answer is not put to the player:
//!   it is answered for them, and takes no place in the record. Ask it all the same, so
//!   the profile that stops at every question still puts it to them.
//! - **Keep nothing between runs.** No count of runs and no memory of an earlier one: the
//!   record is all that carries over, and only these functions read it.
//!
//! The ability must say that it asks: effect data does by what it holds
//! ([`Effect::asks`]), a Rust body by its registration ([`crate::CustomBody::SAYS`]).
//!
//! How many questions an effect asks, in what order and with which candidates is part of
//! every save and every rollout, which replay the answers by their place. A change to any of
//! the three is a change of the save format.

use beryl_card::{Effect, Objects, Place, Zone};

use crate::bitboard::Bitboard;
use crate::card::{Flow, Part, Players, Prompt, Purpose, Shape, Stop};
use crate::decision::{Candidates, Decision, DecisionKind, Opt};
use crate::game::Game;
use crate::ids::Seat;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::resolution::Resolution;

/// Asks `player` to take one of `opts`, as a question of `kind`.
fn option<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    kind: DecisionKind,
    opts: impl IntoIterator<Item = Opt<L>>,
    prompt: Prompt,
    about: Bitboard<L::Slot>,
) -> Result<Opt<L>, Stop> {
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
    for o in opts {
        list.push(o)?;
    }
    let d = Decision {
        player,
        kind,
        candidates: Candidates::Options(list),
        prompt,
        about,
    };
    let c = resolution.answer(game, &d)?;
    Ok(d.candidates.option(c).expect("an answer among the options"))
}

/// Asks `player` yes or no, as `prompt` says, about the objects `about`: the cards the
/// question concerns, or what a yes goes on to pick among ([`beryl_card::prompt`], "The
/// object set"). The whole of a [`Shape::YesNo`], and the yes or no that opens another
/// shape's run.
pub fn yes_no<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    prompt: Prompt,
    about: Bitboard<L::Slot>,
) -> Result<bool, Stop> {
    debug_assert!(
        prompt.shape() == Shape::YesNo || matches!(prompt.part, Part::Whether | Part::Instead),
        "{prompt:?} is not answered yes or no"
    );
    let (kind, yes_no) = (DecisionKind::YesNo, [Opt::Yes, Opt::No]);
    Ok(option(game, resolution, player, kind, yes_no, prompt, about)? == Opt::Yes)
}

/// Asks `player` for one of `candidates`, which must not be empty: an object of a
/// [`Shape::Pick`] where one must be taken, or a card for either destination of a
/// [`Shape::Split`] where the pick offers no end. Interchangeable candidates are one answer
/// (§7.2).
pub fn pick<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    candidates: Bitboard<L::Slot>,
    prompt: Prompt,
) -> Result<L::Slot, Stop> {
    pick_about(
        game,
        resolution,
        player,
        candidates,
        prompt,
        Bitboard::EMPTY,
    )
}

/// [`pick`], of a prompt whose object set is `about`.
fn pick_about<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    candidates: Bitboard<L::Slot>,
    prompt: Prompt,
    about: Bitboard<L::Slot>,
) -> Result<L::Slot, Stop> {
    debug_assert!(
        match prompt.shape() {
            Shape::Pick => prompt.part == Part::Pick,
            Shape::Split => matches!(prompt.part, Part::First | Part::Second),
            _ => false,
        },
        "{prompt:?} is not answered with an object"
    );
    let d = Decision {
        player,
        kind: DecisionKind::ChooseObject,
        candidates: Candidates::Objects(crate::engine::collapse(game.state, candidates)),
        prompt,
        about,
    };
    let c = resolution.answer(game, &d)?;
    Ok(d.candidates.object(c).expect("an answer among the objects"))
}

/// Asks `player` for one more of `candidates` or for the end of the picks, `None`: a pick of
/// a [`Shape::Pick`] that takes up to a number, or a card for the first destination of a
/// [`Shape::Split`] that takes as many as the player gives it. The end is offered first, then
/// the objects in candidate order, interchangeable ones as one answer (§7.2).
pub fn pick_or_end<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    candidates: Bitboard<L::Slot>,
    prompt: Prompt,
) -> Result<Option<L::Slot>, Stop> {
    debug_assert!(
        match prompt.shape() {
            Shape::Pick => prompt.part == Part::Pick,
            Shape::Split => prompt.part == Part::First,
            _ => false,
        },
        "{prompt:?} has no pick that may end"
    );
    let left = crate::engine::collapse(game.state, candidates);
    // Candidates are listed in the candidate order, which a `Choice` counts in.
    let left = game.state.in_candidate_order(left).map(Opt::Object);
    let opts = core::iter::once(Opt::Done).chain(left);
    let (kind, none) = (DecisionKind::ChooseUpTo, Bitboard::EMPTY);
    Ok(
        match option(game, resolution, player, kind, opts, prompt, none)? {
            Opt::Object(slot) => Some(slot),
            _ => None,
        },
    )
}

/// Asks `player` to choose up to `n` of `candidates`, one [`pick_or_end`] at a time, each a
/// pick of `prompt` as it is given: the whole run of a [`Shape::Pick`] with an end. Returns
/// those chosen.
pub fn pick_up_to<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    candidates: Bitboard<L::Slot>,
    n: u8,
    prompt: Prompt,
) -> Result<Bitboard<L::Slot>, Stop> {
    let mut chosen = Bitboard::EMPTY;
    while chosen.count() < u32::from(n) && !(candidates & !chosen).is_empty() {
        match pick_or_end(game, resolution, player, candidates & !chosen, prompt)? {
            Some(slot) => chosen.insert(slot),
            None => break,
        }
    }
    Ok(chosen)
}

/// Asks `player` to choose one of `candidates`, or none when they decline: the whole run of
/// a [`Shape::Pick`] that opens with a yes or no ([`Part::Whether`]) and then takes its
/// object ([`Part::Pick`]). Both carry the candidates as the prompt's object set: the yes or
/// no does not offer them, and the pick offers one of each group of interchangeable ones
/// (§7.2). With no candidates there is nothing to ask.
pub fn may_pick<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    candidates: Bitboard<L::Slot>,
    prompt: Prompt,
) -> Result<Option<L::Slot>, Stop> {
    let whether = prompt.part(Part::Whether);
    if candidates.is_empty() || !yes_no(game, resolution, player, whether, candidates)? {
        return Ok(None);
    }
    let pick = prompt.part(Part::Pick);
    pick_about(game, resolution, player, candidates, pick, candidates).map(Some)
}

/// Asks `player` what the objects `aimed`, chosen already, are aimed at, one of `targets`:
/// the aim of a [`Shape::Declare`] with nothing left to declare. A resolution aims only
/// attackers ([CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): "If a creature
/// is put onto the battlefield attacking, its controller chooses which defending player,
/// planeswalker a defending player controls, or battle a defending player protects it's
/// attacking as it enters the battlefield". Returns the one chosen.
pub fn aim<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    aimed: Bitboard<L::Slot>,
    targets: &[Opt<L>],
    prompt: Prompt,
) -> Result<Opt<L>, Stop> {
    debug_assert!(
        prompt.purpose == Purpose::Attack && prompt.part == Part::Aim,
        "{prompt:?} does not aim an attacker"
    );
    let (kind, targets) = (DecisionKind::AttackTarget, targets.iter().copied());
    option(game, resolution, player, kind, targets, prompt, aimed)
}

/// Asks `player` for one answer of a [`Shape::Pay`], among `options`: at [`Part::Plan`] a
/// whole payment the engine worked out, and otherwise the next step of one made by hand
/// ([CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)): "If an effect gives a
/// player the option to pay mana, they may activate mana abilities before taking that
/// action." Returns the one chosen ([`crate::costs`] pays with it).
pub fn pay<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    options: &[Opt<L>],
    prompt: Prompt,
) -> Result<Opt<L>, Stop> {
    debug_assert!(
        prompt.shape() == Shape::Pay && matches!(prompt.part, Part::Pick | Part::Plan),
        "{prompt:?} is not a payment"
    );
    let kind = match prompt.part {
        Part::Plan => DecisionKind::PaymentPlan,
        _ => DecisionKind::ManaPayment,
    };
    let (options, none) = (options.iter().copied(), Bitboard::EMPTY);
    option(game, resolution, player, kind, options, prompt, none)
}

/// The yes or no of an [`Effect::May`], about nothing but the effect.
const MAY: Prompt = Prompt::new(Purpose::May);

/// The yes or no of an [`Effect::May`] whose whole effect is to exile cards: whether to move
/// them there.
const MAY_EXILE: Prompt = Prompt::new(Purpose::Move).places(Place::Exile, Place::None);

/// The yes or no of an [`Effect::May`] whose effect is a search: "you may search your
/// library".
const MAY_SEARCH: Prompt = Prompt::new(Purpose::MaySearch);

/// [`Effect::May`]: "[Player] may [effect]" (CR 608.2d). The player is asked as the effect
/// is applied, and with no such player no one is. Where the effect is to exile cards the
/// question is about them ([`Purpose::Move`]), and with none to exile there is nothing to
/// ask; where it is a search, the question is whether to search ([`Purpose::MaySearch`]).
/// The outcome is the effect's, or nothing when it was declined.
pub(crate) fn may_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    player: Players,
    effect: &'static Effect,
) -> Flow {
    let (prompt, about) = match *effect {
        Effect::Exile(cards) | Effect::ExileLinked(cards) => {
            let cards = run.elsewhere_than(cards, Zone::Exile);
            if cards.is_empty() {
                return run.carry_out_or_nothing(None);
            }
            (MAY_EXILE, cards)
        }
        Effect::Search { .. } => (MAY_SEARCH, Bitboard::EMPTY),
        _ => (MAY, Bitboard::EMPTY),
    };
    let agreed = match run.one(player) {
        Some(player) => {
            let (game, resolution) = run.resolving();
            yes_no(game, resolution, player, prompt, about)?
        }
        None => false,
    };
    run.carry_out_or_nothing(agreed.then_some(effect))
}

/// [`Effect::Choose`]: "[Player] chooses one of [objects]", or with `optional` may choose
/// none (CR 608.2d): a [`pick`], or a [`may_pick`]. With no such player or no object there
/// is nothing to ask. The outcome is the object chosen.
pub(crate) fn choose_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    player: Players,
    among: Objects,
    prompt: Prompt,
    optional: bool,
) -> Result<Outcome<L>, Stop> {
    let candidates = run.scene().objects(among);
    let Some(player) = run.one(player) else {
        return Ok(Outcome::NOTHING);
    };
    if candidates.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    let (game, resolution) = run.resolving();
    let chosen = if optional {
        may_pick(game, resolution, player, candidates, prompt)?
    } else {
        Some(pick(game, resolution, player, candidates, prompt)?)
    };
    Ok(chosen.map_or(Outcome::NOTHING, |slot| {
        Outcome::acted_on(Bitboard::single(slot))
    }))
}
