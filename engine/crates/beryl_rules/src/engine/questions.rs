//! The engine's open decision and the legal actions it can offer.
//!
//! Implements: CR 712.9, CR 724.5

use crate::bitboard::Bitboard;
use crate::card::{ChosenKind, FaceIx, Prompt, Purpose, Types};
use crate::casting::land::{enter_from_hand, enter_or_choose};
use crate::decision::{Answers, Candidates, Choice, Decision, DecisionKind, OPTIONS, Opt, Refusal};
use crate::fixed::Full;
use crate::game::{companion, start};
use crate::ids::Seat;
use crate::layout::{Layout, Queue, index};
use crate::observation::Observation;
use crate::replacement::event::Sink;
use crate::scratch::Scratch;
use crate::state::{Pending, State};
use crate::table::GameTable;
use crate::turn::paused_step;
use crate::{
    casting, combat, continuous::arrival_timestamps, interpret::catalog, mana, objects::face_down,
    sba, stack::resolve, stack::trigger, turn, zones::piles,
};
use beryl_kernel::reference::Hand;

use super::{act, collapse};

/// The open decision, a pure function of the state, so `apply` recomputes rather than
/// remembers it. The sink only counts the payment solver's runs. It is built as its own
/// player's to answer: `next` gives it to the player controlling them, if one does
/// ([`controlled`]).
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<Decision<L>, Full> {
    // Each arm returns its result as it is, so a decision is built once, in place.
    match *s.pending() {
        Pending::None => match trigger::next_to_stack(s) {
            Some((player, triggers)) => {
                let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
                for &i in triggers.iter() {
                    list.push(Opt::Trigger(i))?;
                }
                let kind = DecisionKind::TriggerOrder;
                let order = Prompt::new(Purpose::TriggerOrder);
                Ok(Decision::options(player, kind, list, order))
            }
            None => priority_decision(s, t),
        },
        Pending::PlayOrDraw { chooser } => options(
            chooser,
            DecisionKind::PlayOrDraw,
            &[Opt::Play, Opt::Draw],
            Prompt::new(Purpose::PlayOrDraw),
        ),
        Pending::Mulligans => match start::mulligan_question(s) {
            (p, true) => Ok(Decision::objects(
                p,
                DecisionKind::ChooseObject,
                collapse(s, turn::hand(s, p)),
                Prompt::new(Purpose::PutOnBottom).most(s.player(p).to_bottom()),
            )),
            // The question is about the hand to keep.
            (p, false) => Ok(options(
                p,
                DecisionKind::KeepOrMulligan,
                &[Opt::Keep, Opt::Mulligan],
                Prompt::new(Purpose::Mulligan),
            )?
            .about(turn::hand(s, p))),
        },
        Pending::Discard => {
            let p = s.turn().active();
            let count = turn::cleanup_discards(s, t);
            Ok(Decision::objects(
                p,
                DecisionKind::ChooseObject,
                collapse(s, turn::hand(s, p)),
                Prompt::new(Purpose::Cleanup).most(u8::try_from(count).unwrap_or(u8::MAX)),
            ))
        }
        Pending::LegendRule { kept } => {
            Ok(sba::legend_decision(s, t, kept).expect("a legend-rule group without a keeper"))
        }
        Pending::OrderEntered { group } => Ok(arrival_timestamps::decision(s, group)),
        Pending::ArrangePiles { group } => Ok(piles::decision(s, group)),
        // The question is about the land entering, for this much life.
        Pending::EnterPayingLife { slot, life } => Ok(options(
            s.object(slot).owner(),
            DecisionKind::YesNo,
            &[Opt::Yes, Opt::No],
            Prompt::new(Purpose::PayLife).count(life),
        )?
        .about(Bitboard::single(slot))),
        // The question is about the permanent whose mana ability is being activated.
        Pending::ManaColor { slot, .. } => catalog::decision(
            t,
            s.controller(slot),
            ChosenKind::Color,
            Bitboard::single(slot),
        ),
        // The question is about the land entering.
        Pending::EnterChoosing { slot, kind, .. } => {
            catalog::decision(t, s.object(slot).owner(), kind, Bitboard::single(slot))
        }
        Pending::DeclareAttackers { attacker } => combat::attack_decision(s, t, attacker),
        Pending::DeclareBlockers { blocker } => combat::block_decision(s, t, blocker),
        Pending::CombatDamage {
            source, ref lethal, ..
        } => combat::division_decision(s, t, source, lethal.as_slice()),
        Pending::ExactCombatDamage(ref dividing) => combat::exact_decision(s, t, dividing),
        Pending::Casting(c) => casting::decision(s, t, sink, &c),
        Pending::TurningFaceUp {
            slot,
            cost,
            payment,
        } => face_down::decision(s, t, sink, slot, cost, payment),
        Pending::Companion { chooser } => companion::decision(s, t, chooser),
        Pending::CompanionPayment { player, remaining } => {
            companion::payment_decision(s, t, sink, player, remaining)
        }
        Pending::Resolving(ref r) | Pending::StepActions(ref r) | Pending::TurnStart(ref r) => {
            Ok(r.question.clone())
        }
    }
}

fn options<L: Layout>(
    player: Seat,
    kind: DecisionKind,
    opts: &[Opt<L>],
    prompt: Prompt,
) -> Result<Decision<L>, Full> {
    let mut list: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for &o in opts {
        list.push(o)?;
    }
    Ok(Decision::options(player, kind, list, prompt))
}

/// The priority holder's legal actions, passing first.
///
/// [CR 117.1](https://yawgatog.com/resources/magic-rules/#R1171): "The player with priority
/// may cast spells, activate abilities, and take special actions."
fn priority_decision<L: Layout>(s: &State<L>, t: &GameTable) -> Result<Decision<L>, Full> {
    let player = s.turn().priority();
    // Built in place: the list is most of a decision's size.
    let mut d: Decision<L> = Decision {
        player,
        decider: player,
        kind: DecisionKind::Priority,
        candidates: Candidates::Options(Default::default()),
        prompt: Prompt::new(Purpose::Priority),
        about: Bitboard::EMPTY,
    };
    let Candidates::Options(list) = &mut d.candidates else {
        unreachable!("built with options");
    };
    list.push(Opt::Pass)?;
    if turn::may_play_land(s, player, t.format()) {
        let lands = turn::hand(s, player).filter(|slot| land_face(s, t, slot).is_some());
        Candidates::<L>::extend_options(list, collapse(s, lands), |out, slot| {
            out.push(Opt::PlayLand(slot))
        })?;
    }
    casting::priority_options(s, t, player, list)?;
    mana::instant_options(s, t, player, list)?;
    face_down::face_up_options(s, t, player, list)?;
    companion::options(s, t, player, list)?;
    // Last, as the options a full list leaves out: tapping ahead is never needed, the payment
    // taps for itself.
    if s.profile().floats_for(player) {
        mana::standalone_options(s, t, player, list);
    }
    Ok(d)
}

/// Whether `d` is a priority decision whose only action is passing, but for activating mana
/// abilities on their own: the engine answers it as a lone pass where it would one, as floating
/// mana that nothing else can spend now changes nothing
/// (see [CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "Each player's mana pool
/// empties at the end of each step and phase").
///
/// So is a priority whose other actions are activations that would do nothing, for the player
/// offered them quietly ([`crate::profile::Profile::quiet_pointless`]): paying a cost for no effect is
/// legal (see [CR 602.2](https://yawgatog.com/resources/magic-rules/#R6022)), but no reason to stop.
pub(super) fn only_idle<L: Layout>(s: &State<L>, t: &GameTable, d: &Decision<L>) -> bool {
    let floats = s.profile().floats_for(d.player);
    let quiet = s.profile().quiet_pointless == Some(d.player);
    if d.kind != DecisionKind::Priority || !(floats || quiet) {
        return false;
    }
    match &d.candidates {
        Candidates::Options(list) => list.iter().all(|o| match *o {
            Opt::Pass => true,
            Opt::Mana { .. } => floats,
            Opt::Activate { slot, ability } => quiet && casting::pointless(s, t, slot, ability),
            Opt::PlayLand(_)
            | Opt::Play
            | Opt::Draw
            | Opt::Keep
            | Opt::Mulligan
            | Opt::Trigger(_)
            | Opt::Yes
            | Opt::No
            | Opt::Done
            | Opt::Attack { .. }
            | Opt::Defender(_)
            | Opt::Block(_)
            | Opt::Lethal(_)
            | Opt::Cast { .. }
            | Opt::Pool(_)
            | Opt::Number(_)
            | Opt::Plan(_)
            | Opt::Player(_)
            | Opt::Ability(_)
            | Opt::Object(_)
            | Opt::TurnFaceUp(_)
            | Opt::Companion => false,
        }),
        Candidates::Objects(_) => false,
    }
}

/// The first face of a card that is a land, which is the face it is played with
/// ([CR 712.9](https://yawgatog.com/resources/magic-rules/#R7129)): "A player playing a
/// modal double-faced card ... as a land chooses one of its faces that's a land before putting
/// it onto the battlefield." The implemented modal double-faced cards have one land face at most.
pub(crate) fn land_face<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Option<FaceIx> {
    let def = s.card_def(t, slot)?;
    def.face_ixs().find(|&face| {
        def.face(face).types.intersects(Types::LAND)
            && (face == FaceIx::FRONT || def.shape == beryl_card::definition::Shape::Modal)
    })
}

/// Why the viewer may neither cast the card in `slot`, from their hand, nor play it as a land
/// at their priority now, for a client to say why it isn't offered: the first rule the most
/// nearly legal way of playing it breaks. None if one way is legal. It reads what their
/// priority's candidates do, so it tells them nothing they don't know.
///
/// See [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "A player who has priority
/// may play a land card from their hand during a main phase of their turn when the stack is
/// empty." See [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can
/// normally play one land during their turn".
pub fn refusal<L: Layout>(
    obs: Observation<'_, L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<Refusal<L>> {
    let (s, player) = (obs.shown(), obs.viewer());
    let land = match land_face(s, t, slot) {
        None => None,
        Some(_) if turn::may_play_land(s, player, t.format()) => return None,
        Some(_) if casting::sorcery_timing(s, player) => Some(Refusal::LandPlayed),
        Some(_) => Some(Refusal::Timing { by: None }),
    };
    // A full capacity leaves the reason unknown.
    casting::refusal(s, t, player, slot, land).ok().flatten()
}

/// Gives `d`, built for the player it belongs to, to the player controlling them this turn, if
/// one does ([`State::decider`]; [CR 724.5]): "While controlling another player, a player
/// makes all choices and decisions the controlled player is allowed to make or is told to
/// make by the rules or by any objects." Set only on a decision `next` returns, rather than
/// where each is built, so that a game no one controls never moves a decision once more, and
/// a decision answered silently skips it. Out of line and taking the decision by value, so
/// the common path neither calls it nor borrows its decision.
///
/// [CR 724.5]: https://yawgatog.com/resources/magic-rules/#R7245
#[cold]
#[inline(never)]
pub(crate) fn controlled<L: Layout>(s: &State<L>, mut d: Decision<L>) -> Decision<L> {
    d.decider = s.decider(d.player);
    d
}

/// Answers a question of the casting machine, a paused resolution, if one is open, or one of the rules' own questions that a general kind asks: the
/// pending state says which question it is, where the kind says only the form of its answer.
pub(super) fn perform_machine<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    x: &mut Scratch<L>,
    a: &mut dyn Answers<L>,
    d: &Decision<L>,
    c: Choice,
) -> Option<Result<(), Full>> {
    Some(match *s.pending() {
        Pending::Resolving(ref r) => {
            let answer = resolve::recorded(d, index::<L>(usize::from(c.0)), r.asks_hidden());
            s.record_answer(answer)
                .and_then(|()| resolve::resolve(s, t, x, a))
        }
        Pending::StepActions(_) | Pending::TurnStart(_) => s
            .record_answer(index::<L>(usize::from(c.0)))
            .and_then(|()| paused_step::step_actions(s, t, x, a)),
        Pending::Casting(casting) => {
            let (option, object) = (d.candidates.option(c), d.candidates.object(c));
            act(s, &mut x.events, d.player, option);
            let done = casting::perform(s, t, &mut x.events, casting, option, object);
            x.events.leave();
            done
        }
        Pending::TurningFaceUp {
            slot,
            cost,
            payment,
        } => {
            let option = d
                .candidates
                .option(c)
                .or_else(|| d.candidates.object(c).map(Opt::Object))
                .expect("a payment");
            act(s, &mut x.events, d.player, None);
            let done = face_down::perform(s, t, &mut x.events, slot, cost, payment, option);
            x.events.leave();
            done
        }
        Pending::Companion { chooser } => {
            let option = d.candidates.option(c);
            act(s, &mut x.events, d.player, None);
            let done = companion::answer(s, t, &mut x.events, chooser, option);
            x.events.leave();
            done
        }
        Pending::CompanionPayment { player, remaining } => {
            let option = d.candidates.option(c).expect("a payment");
            act(s, &mut x.events, d.player, None);
            let done = companion::pay(s, t, &mut x.events, player, remaining, option);
            x.events.leave();
            done
        }
        Pending::EnterPayingLife { slot, .. } => {
            s.settle_question();
            let pay = d.candidates.option(c) == Some(Opt::Yes);
            act(s, &mut x.events, d.player, None);
            let land = s.card_in(s.object_ref(slot), Hand).expect("a land in hand");
            let done = enter_or_choose(s, t, &mut x.events, land, pay);
            x.events.leave();
            done
        }
        Pending::ManaColor { slot, .. } => {
            s.settle_question();
            act(s, &mut x.events, d.player, None);
            let done = mana::activate_chosen(s, t, &mut x.events, slot, d.candidates.option(c));
            x.events.leave();
            done
        }
        Pending::EnterChoosing { slot, pay_life, .. } => {
            s.settle_question();
            act(s, &mut x.events, d.player, None);
            // The land's entering asks the same question again, of the same candidates, and
            // the answer is recorded for it.
            let done = x.events.asking.begin(&[index::<L>(c.0.into())], false);
            let land = s.card_in(s.object_ref(slot), Hand).expect("a land in hand");
            let done = done.and_then(|()| enter_from_hand(s, t, &mut x.events, land, pay_life));
            x.events.asking.end();
            x.events.leave();
            done
        }
        // The other question of a mulligan, keep or mulligan, has a kind of its own.
        Pending::Mulligans if d.kind == DecisionKind::ChooseObject => {
            let card = d.candidates.object(c).expect("an object choice");
            let card = s.card_in(s.object_ref(card), Hand).expect("a card in hand");
            start::put_on_bottom(s, t, x, a, d.player, card)
        }
        Pending::Discard => {
            let card = d.candidates.object(c).expect("an object choice");
            turn::cleanup_discard(s, t, &mut x.events, card)
        }
        Pending::LegendRule { mut kept } => {
            kept.insert(d.candidates.object(c).expect("an object choice"));
            if sba::unresolved_legends(s, t, kept).is_some() {
                s.ask(Pending::LegendRule { kept });
                Ok(())
            } else {
                sba::apply_legend_rule(s, t, &mut x.events, kept)
            }
        }
        Pending::OrderEntered { group } => {
            arrival_timestamps::pick(s, group, d.candidates.object(c).expect("an object choice"))
        }
        Pending::ArrangePiles { group } => {
            piles::pick(s, group, d.candidates.object(c).expect("an object choice"))
        }
        Pending::None
        | Pending::PlayOrDraw { .. }
        | Pending::Mulligans
        | Pending::DeclareAttackers { .. }
        | Pending::DeclareBlockers { .. }
        | Pending::CombatDamage { .. }
        | Pending::ExactCombatDamage(_) => return None,
    })
}
