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
//! | | [`retarget`] | A copy's target: the one it has, or a new one. |
//! | | [`pick_or_end`] | One more object, or the end of the picks. |
//! | | [`pick_up_to`] | A whole run: up to a number of objects. |
//! | | [`may_pick`] | A whole run: one object, which may be declined. |
//! | [`Shape::Split`] | [`pick`], [`pick_or_end`] | A card for the destination the prompt's part names; the second way where the first destination's picks offer an end. |
//! | [`Shape::Pay`] | [`pay`] | One step of a payment, or the whole of one the engine worked out. |
//! | [`Shape::Declare`] | [`aim`] | What objects already chosen are aimed at. |
//! | [`Shape::Catalog`] | [`catalog`] | A color, a creature type, a card name or a word. |
//! | | [`catalog_among`] | One of some of a catalog's values. |
//!
//! Casting during a resolution asks the casting machine's questions through the same answer
//! record ([`crate::casting::start::offer`]).
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
//!   ([`State::in_game_order`](crate::state::State::in_game_order), for instance); these
//!   functions order their own candidates.
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
//!
//! Implements: CR 608.2d

use beryl_card::{ChosenKind, Effect, Objects, Place, Zone};
use beryl_kernel::chosen::ChosenValue;

use crate::bitboard::Bitboard;
use crate::card::{Flow, Part, Players, Prompt, Purpose, Shape, Stop};
use crate::decision::{Candidates, Decision, DecisionKind, OPTIONS, Opt};
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
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for o in opts {
        list.push(o)?;
    }
    let d = Decision {
        player,
        decider: player,
        kind,
        candidates: Candidates::Options(list),
        prompt,
        about,
    };
    let c = resolution.answer(game, &d)?;
    Ok(d.candidates.option(c).expect("an answer among the options"))
}

/// Asks `player` for a value of `kind`: a color, a creature type or a card name, one entry of
/// the catalog [`crate::interpret::catalog`] numbers, about the objects `about`. The whole of a
/// [`Shape::Catalog`].
pub fn catalog<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    kind: ChosenKind,
    about: Bitboard<L::Slot>,
) -> Result<ChosenValue<L>, Stop> {
    let d = crate::interpret::catalog::decision(game.table, player, kind, about)?;
    let c = resolution.answer(game, &d)?;
    Ok(crate::interpret::catalog::value(
        kind,
        d.candidates.option(c),
    ))
}

/// Asks `player` for one of the values of `kind` that `keep` takes, by number, as `prompt`
/// says: a [`Shape::Catalog`] narrowed by an answer before it, as a text change's new word
/// is one of the kind of the word it replaces.
pub fn catalog_among<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    kind: ChosenKind,
    prompt: Prompt,
    about: Bitboard<L::Slot>,
    keep: impl Fn(u8) -> bool,
) -> Result<ChosenValue<L>, Stop> {
    debug_assert!(prompt.shape() == Shape::Catalog, "{prompt:?} is no catalog");
    let d =
        crate::interpret::catalog::decision_among(game.table, player, kind, prompt, about, keep)?;
    let c = resolution.answer(game, &d)?;
    Ok(crate::interpret::catalog::value(
        kind,
        d.candidates.option(c),
    ))
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
            Shape::Order
            | Shape::YesNo
            | Shape::Number
            | Shape::Mode
            | Shape::Catalog
            | Shape::Divide
            | Shape::Pay
            | Shape::Declare => false,
        },
        "{prompt:?} is not answered with an object"
    );
    let d = Decision {
        player,
        decider: player,
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
            Shape::Order
            | Shape::YesNo
            | Shape::Number
            | Shape::Mode
            | Shape::Catalog
            | Shape::Divide
            | Shape::Pay
            | Shape::Declare => false,
        },
        "{prompt:?} has no pick that may end"
    );
    let left = crate::engine::collapse(game.state, candidates);
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    list.push(Opt::Done)?;
    Candidates::<L>::extend_options(&mut list, left, |out, slot| out.push(Opt::Object(slot)))?;
    let d = Decision::options(player, DecisionKind::ChooseUpTo, list, prompt);
    let answer = resolution.answer(game, &d)?;
    Ok(match d.candidates.option(answer) {
        Some(Opt::Object(slot)) => Some(slot),
        _ => None,
    })
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

/// Asks `player` for a target of a copy of a spell as it is made: `kept`, the target it has,
/// listed first, or one of `others`, the new targets it may have instead
/// (see [CR 707.10c](https://yawgatog.com/resources/magic-rules/#R70710c)): "The player may leave
/// any number of the targets unchanged, even if those targets would be illegal." A target spec
/// that can choose players lists them by seat, then any abilities on the stack, the top first,
/// then any objects in the candidate order, as the casting machine lists a target's options.
/// Returns the one chosen.
pub(crate) fn retarget<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    kept: Opt<L>,
    others: crate::casting::TargetSet<L>,
) -> Result<Opt<L>, Stop> {
    let s = &*game.state;
    let mut opts: L::Queue<Opt<L>, OPTIONS> = Default::default();
    opts.push(kept)?;
    for p in others.players.in_seat_order() {
        opts.push(Opt::Player(p))?;
    }
    let mut unlisted = others.abilities;
    while unlisted != 0 {
        let place = usize::try_from(unlisted.ilog2()).expect("a bit in a u64");
        unlisted &= !(1 << place);
        opts.push(Opt::Ability(crate::layout::index::<L>(place)))?;
    }
    Candidates::<L>::extend_options(
        &mut opts,
        crate::engine::collapse(s, others.objects),
        |out, slot| out.push(Opt::Object(slot)),
    )?;
    let opts = opts.as_slice().iter().copied().enumerate();
    // The target kept is never listed twice.
    let opts = opts
        .filter(|&(i, opt)| i == 0 || opt != kept)
        .map(|(_, opt)| opt);
    let prompt = Prompt::new(Purpose::Target).most(1);
    let none = Bitboard::EMPTY;
    option(
        game,
        resolution,
        player,
        DecisionKind::Target,
        opts,
        prompt,
        none,
    )
}

/// Asks `player` what the objects `aimed`, chosen already, are aimed at, one of `targets`:
/// the aim of a [`Shape::Declare`] with nothing left to declare. A resolution aims only
/// attackers (see [CR 508.4](https://yawgatog.com/resources/magic-rules/#R5084)): "If a creature
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
/// (see [CR 608.2g](https://yawgatog.com/resources/magic-rules/#R6082g)): "If an effect gives a
/// player the option to pay mana, they may activate mana abilities before taking that
/// action." Returns the one chosen ([`crate::casting::payment`] pays with it).
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
        Part::Pick
        | Part::Whether
        | Part::Instead
        | Part::Count
        | Part::First
        | Part::Second
        | Part::Aim => DecisionKind::ManaPayment,
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
        Effect::Sequence(_)
        | Effect::If { .. }
        | Effect::May { .. }
        | Effect::Unless { .. }
        | Effect::ForEachPlayer { .. }
        | Effect::Choose { .. }
        | Effect::LookAt { .. }
        | Effect::Counter(_)
        | Effect::CounterInto { .. }
        | Effect::CopySpell { .. }
        | Effect::Destroy { .. }
        | Effect::Regenerate(_)
        | Effect::Tap(_)
        | Effect::PhaseOut(_)
        | Effect::ExileUntilThisLeaves(_)
        | Effect::Sacrifice(_)
        | Effect::ReturnToHand(_)
        | Effect::PutIntoGraveyard(_)
        | Effect::ReturnToBattlefield(_)
        | Effect::PutOntoBattlefield(_)
        | Effect::ReturnTransformed(_)
        | Effect::PutIntoLibrary { .. }
        | Effect::PutBack { .. }
        | Effect::ShuffleInto { .. }
        | Effect::Dig { .. }
        | Effect::ExileUntil { .. }
        | Effect::Fateseal { .. }
        | Effect::Draw { .. }
        | Effect::Discard { .. }
        | Effect::Mill { .. }
        | Effect::Scry { .. }
        | Effect::Explore(_)
        | Effect::ManifestDread(_)
        | Effect::FlipCoin(_)
        | Effect::DealDamage { .. }
        | Effect::PreventDamage { .. }
        | Effect::GainLife { .. }
        | Effect::LoseLife { .. }
        | Effect::CreateTokens { .. }
        | Effect::CreateAttackingTokens { .. }
        | Effect::CreateCopies { .. }
        | Effect::BecomeCopy { .. }
        | Effect::Mutate(_)
        | Effect::AddCounters { .. }
        | Effect::RemoveCounters { .. }
        | Effect::AddMana { .. }
        | Effect::AddManaOfKind { .. }
        | Effect::Modify { .. }
        | Effect::ChangeText { .. }
        | Effect::ExchangeControl(..)
        | Effect::Attach { .. }
        | Effect::ChangeRule { .. }
        | Effect::GainFlashback(_)
        | Effect::PermitCast { .. }
        | Effect::MayCast { .. }
        | Effect::UntapUpTo { .. }
        | Effect::Delay { .. }
        | Effect::GetEmblem { .. }
        | Effect::WinGame(_)
        | Effect::LoseGame(_)
        | Effect::ControlPlayer(_)
        | Effect::EndTurn
        | Effect::ExtraTurn(_)
        | Effect::Wish { .. } => (MAY, Bitboard::EMPTY),
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

/// Asks `player` for one of `candidates`, cards of a hidden zone of theirs, or for none
/// (`Opt::Done`, offered first), without revealing which
/// ([CR 101.4a](https://yawgatog.com/resources/magic-rules/#R1014a)): "If an effect has each
/// player choose a card in a hidden zone, such as their hand or library, those cards may
/// remain face down as they're chosen." It is asked though no card passes, and never
/// answered silently: the other players do not know the candidates, so whether and how it is
/// asked must not depend on them. Interchangeable cards are one answer (§7.2). The answer is
/// recorded as the slot picked ([`Resolution::answer_hidden`]).
pub fn hidden_pick<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    player: Seat,
    candidates: Bitboard<L::Slot>,
    prompt: Prompt,
) -> Result<Option<L::Slot>, Stop> {
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    list.push(Opt::Done)?;
    let candidates = crate::engine::collapse(game.state, candidates);
    Candidates::<L>::extend_options(&mut list, candidates, |out, slot| {
        out.push(Opt::Object(slot))
    })?;
    let d = Decision {
        player,
        decider: player,
        kind: DecisionKind::ChooseUpTo,
        candidates: Candidates::Options(list),
        prompt,
        about: Bitboard::EMPTY,
    };
    resolution.answer_hidden(game, &d)
}

/// [`Effect::Choose`]: "[Player] chooses one of [objects]", or with `optional` may choose
/// none (CR 608.2d): a [`pick`], or a [`may_pick`]. With no such player or no object there
/// is nothing to ask. A `hidden` pick is a [`hidden_pick`], asked of the player whatever
/// their candidates. The outcome is the object chosen.
pub(crate) fn choose_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    player: Players,
    among: Objects,
    prompt: Prompt,
    (optional, hidden): (bool, bool),
) -> Result<Outcome<L>, Stop> {
    let candidates = run.scene().objects(among);
    let Some(player) = run.one(player) else {
        return Ok(Outcome::NOTHING);
    };
    if hidden {
        let (game, resolution) = run.resolving();
        let chosen = hidden_pick(game, resolution, player, candidates, prompt)?;
        return Ok(chosen.map_or(Outcome::NOTHING, |slot| {
            Outcome::acted_on(Bitboard::single(slot))
        }));
    }
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
