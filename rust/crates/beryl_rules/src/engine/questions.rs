//! The engine's open decision and the legal actions it can offer.

use crate::bitboard::Bitboard;
use crate::card::{FaceIx, Prompt, Purpose, Types};
use crate::decision::{Candidates, Decision, DecisionKind, Opt, Refusal};
use crate::event::Sink;
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue};
use crate::observation::Observation;
use crate::state::{Pending, State};
use crate::table::GameTable;
use crate::{cast, combat, face_down, mana, sba, start, trigger, turn};

use super::collapse;

/// The open decision, a pure function of the state, so `apply` recomputes rather than
/// remembers it. The sink only counts the payment solver's runs.
pub(crate) fn decision<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
) -> Result<Decision<L>, Full> {
    // Each arm returns its result as it is, so a decision is built once, in place.
    match *s.pending() {
        Pending::None => match trigger::next_to_stack(s) {
            Some((player, triggers)) => {
                let mut list: L::Queue<Opt<L>, 64> = Default::default();
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
            let count = s.hand_size(p).saturating_sub(turn::MAX_HAND_SIZE);
            Ok(Decision::objects(
                p,
                DecisionKind::ChooseObject,
                collapse(s, turn::hand(s, p)),
                Prompt::new(Purpose::Cleanup).most(u8::try_from(count).unwrap_or(u8::MAX)),
            ))
        }
        Pending::LegendRule { kept } => {
            let (p, group) =
                sba::unresolved_legends(s, t, kept).expect("a legend-rule group without a keeper");
            let keep = Prompt::new(Purpose::LegendRule).most(1);
            let kind = DecisionKind::ChooseObject;
            Ok(Decision::objects(p, kind, collapse(s, group), keep))
        }
        // The question is about the land entering, for this much life.
        Pending::EnterPayingLife { slot, life } => Ok(options(
            s.object(slot).owner(),
            DecisionKind::YesNo,
            &[Opt::Yes, Opt::No],
            Prompt::new(Purpose::PayLife).count(life),
        )?
        .about(Bitboard::single(slot))),
        Pending::DeclareAttackers { attacker } => combat::attack_decision(s, attacker),
        Pending::DeclareBlockers { blocker } => combat::block_decision(s, blocker),
        Pending::CombatDamage { done, lethal } => combat::division_decision(s, done, lethal),
        Pending::Casting(c) => cast::decision(s, t, sink, &c),
        Pending::TurningFaceUp { slot, remaining } => {
            face_down::decision(s, t, sink, slot, remaining)
        }
        Pending::Resolving(ref r) => Ok(r.question.clone()),
        Pending::CastOffer {
            object,
            player,
            free,
        } => Ok(Decision::options(
            player,
            DecisionKind::CastOffer,
            cast::offer_options(s, t, object, player, free)?,
            Prompt::new(Purpose::CastOffer),
        )
        // The question is about the card offered.
        .about(Bitboard::single(object.slot))),
    }
}

fn options<L: Layout>(
    player: Seat,
    kind: DecisionKind,
    opts: &[Opt<L>],
    prompt: Prompt,
) -> Result<Decision<L>, Full> {
    let mut list: L::Queue<Opt<L>, 64> = Default::default();
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
        kind: DecisionKind::Priority,
        candidates: Candidates::Options(Default::default()),
        prompt: Prompt::new(Purpose::Priority),
        about: Bitboard::EMPTY,
    };
    let Candidates::Options(list) = &mut d.candidates else {
        unreachable!("built with options");
    };
    list.push(Opt::Pass)?;
    if turn::may_play_land(s, player) {
        let lands = turn::hand(s, player).filter(|slot| land_face(s, t, slot).is_some());
        // Candidates are listed in the candidate order, which a `Choice` counts in.
        for slot in s.in_candidate_order(collapse(s, lands)) {
            list.push(Opt::PlayLand(slot))?;
        }
    }
    cast::priority_options(s, t, player, list)?;
    face_down::face_up_options(s, t, player, list)?;
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
/// ([CR 106.4](https://yawgatog.com/resources/magic-rules/#R1064): "Each player's mana pool
/// empties at the end of each step and phase").
///
/// So is a priority whose other actions are activations that would do nothing, for the player
/// offered them quietly ([`crate::Profile::quiet_pointless`]): paying a cost for no effect is
/// legal ([CR 602.2](https://yawgatog.com/resources/magic-rules/#R6022)), but no reason to stop.
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
            Opt::Activate { slot, ability } => quiet && cast::pointless(s, t, slot, ability),
            _ => false,
        }),
        Candidates::Objects(_) => false,
    }
}

/// The first face of a card that is a land, which is the face it is played with
/// ([CR 712.9](https://yawgatog.com/resources/magic-rules/#R7129)): "A player playing a
/// modal double-faced card ... as a land chooses one of its faces that's a land before putting
/// it onto the battlefield." The cube's modal double-faced cards have one land face at most.
pub(super) fn land_face<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> Option<FaceIx> {
    let def = s.card_def(t, slot)?;
    def.face_ixs()
        .find(|&f| def.face(f).types.intersects(Types::LAND))
}

/// Why the viewer may neither cast the card in `slot`, from their hand, nor play it as a land
/// at their priority now, for a client to say why it isn't offered: the first rule the most
/// nearly legal way of playing it breaks. None if one way is legal. It reads what their
/// priority's candidates do, so it tells them nothing they don't know.
///
/// [CR 305.1](https://yawgatog.com/resources/magic-rules/#R3051): "A player who has priority
/// may play a land card from their hand during a main phase of their turn when the stack is
/// empty." [CR 305.2](https://yawgatog.com/resources/magic-rules/#R3052): "A player can
/// normally play one land during their turn".
pub fn refusal<L: Layout>(
    obs: Observation<'_, L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<Refusal<L>> {
    let (s, player) = (obs.shown(), obs.viewer());
    let land = match land_face(s, t, slot) {
        None => None,
        Some(_) if turn::may_play_land(s, player) => return None,
        Some(_) if cast::sorcery_timing(s, player) => Some(Refusal::LandPlayed),
        Some(_) => Some(Refusal::Timing { by: None }),
    };
    // A full capacity leaves the reason unknown.
    cast::refusal(s, t, player, slot, land).ok().flatten()
}
