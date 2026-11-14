//! Replacement effects ([CR 614.1]): "Such effects watch for a particular event that would
//! happen and completely or partially replace that event with a different event."
//!
//! Each replaceable happening is decided here before the state changes: where an object that
//! is about to move goes and how a permanent enters ([`moving`]), and what happens instead of a
//! card drawn ([`draw`]). The caller then performs what is decided, so the
//! event that was replaced never happens and nothing triggers on it ([CR 614.6]): "If an event
//! is replaced, it never happens. A modified event occurs instead, which may in turn trigger
//! abilities." A creature exiled instead of being put into a graveyard did not die.
//!
//! The replacements come from three places: an entry of the Effects store on the object
//! ([`MoveReplacement`]), a static ability of a permanent ([`Replaced`]), and the entering
//! object's own abilities. The card table holds only replacements this module has code for
//! ([`crate::replacement::check`]).
//!
//! [CR 614.1]: https://yawgatog.com/resources/magic-rules/#R6141
//! [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146

use beryl_card::prompt::Purpose;
use beryl_card::{ChosenKind, Draws, Prompt};
use beryl_kernel::chosen::ChosenValue;

use crate::bitboard::Bitboard;
use crate::card::{AbilityIx, AbilityKind, FaceAbility, Replaced, Replacement, Rewrite, Types};
use crate::decision::{Choice, Decision, DecisionKind, Opt};
use crate::event::{Entry, LossCause, Reason, Recipient, ReplacementKind, Sink};
use crate::filter::{self, Evaluate};
use crate::fixed::{Fixed, Full};
use crate::game::Game;
use crate::ids::Seat;
use crate::layers;
use crate::layout::{Layout, Queue};
use crate::life;
use crate::profile::Stops;
use crate::replacement;
use crate::state::{
    AbilityMask, Count, CounterKind, MoveReplacement, ObjectRef, State, TurnStep, Zone, ZoneKey,
};
use crate::table::{Capability, GameTable};
use crate::{actions, chosen, interpret, protection, trigger, zones};

/// A move proposed for one object of a batch, before the state changes: the proposal the
/// replacement effects of a move rewrite ([`moving`]). Where in a library it goes and why it
/// moves are the same for the whole batch, and are kept by the batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Move<L: Layout> {
    pub slot: L::Slot,
    pub from: Zone,
    pub to: Zone,
    /// How it enters, if it is put onto the battlefield.
    pub entry: Entry<L>,
    /// It moves as its spell resolves: a permanent spell becoming a permanent
    /// ([CR 608.3a]): "If the object that's resolving has no targets, it becomes a permanent
    /// and enters the battlefield". Only that resolution moves objects for
    /// [`crate::event::Reason::Resolve`], so a card put onto the battlefield by an effect
    /// "wasn't cast", whichever zone it left. It is not in [`Entry`], which a batch of moves
    /// holds 128 of.
    ///
    /// [CR 608.3a]: https://yawgatog.com/resources/magic-rules/#R6083a
    pub cast: bool,
}

/// Applies the replacement effects of a move to `proposal`: where the object goes
/// ([`destination`]), then, for a permanent that enters, how it enters ([`entering`]). A
/// value the permanent chooses as it enters is asked through `sink`'s record of answers, and
/// kept in `sink` until the permanent has entered ([`Sink::chosen`]).
#[inline]
pub(crate) fn moving<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    mut proposal: Move<L>,
) -> Result<Move<L>, Full> {
    proposal.to = destination(s, t, &proposal)?;
    if proposal.to == Zone::Battlefield {
        proposal.entry = entering(s, t, sink, proposal.slot, proposal.entry)?;
    }
    Ok(proposal)
}

/// Damage proposed before it is dealt: the proposal that prevention and replacement effects
/// of damage rewrite ([`damage`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Damage<L: Layout> {
    pub source: ObjectRef<L>,
    pub to: Recipient<L>,
    pub amount: u8,
    /// It is combat damage ([CR 510.2]).
    ///
    /// [CR 510.2]: https://yawgatog.com/resources/magic-rules/#R5102
    pub combat: bool,
    /// It can be prevented: no effect says it can't ([CR 615.12]), "Some effects state that
    /// damage “can’t be prevented.” If unpreventable damage would be dealt, any applicable
    /// prevention effects are still applied to it. Those effects won’t prevent any damage,
    /// but any additional effects they have will take place."
    ///
    /// [CR 615.12]: https://yawgatog.com/resources/magic-rules/#R61512
    pub preventable: bool,
}

/// Applies the prevention effects of damage to `proposal`. The one there is yet is
/// protection ([CR 702.16e]): "Any damage that would be dealt by sources that have the stated
/// quality to a permanent or player with protection is prevented." Prevented damage is never
/// dealt ([CR 615.6]): "If damage that would be dealt is prevented, it never happens." Gated on
/// the kind: while no effect gives protection, the proposal is returned as built after one
/// AND.
///
/// [CR 702.16e]: https://yawgatog.com/resources/magic-rules/#R70216e
/// [CR 615.6]: https://yawgatog.com/resources/magic-rules/#R6156
#[inline]
pub(crate) fn damage<L: Layout>(s: &State<L>, t: &GameTable, mut proposal: Damage<L>) -> Damage<L> {
    if !protection::live(s) || !proposal.preventable {
        return proposal;
    }
    if let Recipient::Permanent(r) = proposal.to
        && protection::protected_from_cold(s, t, r.slot, proposal.source.slot)
    {
        proposal.amount = 0;
    }
    proposal
}

/// A replacement effect that applies to a move, by where it comes from: what [CR 614.5] counts
/// as having applied. "A replacement effect doesn't invoke itself repeatedly; it gets only one
/// opportunity to affect an event or any modified events that may replace that event."
///
/// [CR 614.5]: https://yawgatog.com/resources/magic-rules/#R6145
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Source<S> {
    /// An entry of the Effects store on the moving object.
    Effect(MoveReplacement),
    /// A static ability of a permanent.
    Ability(S, AbilityIx),
}

/// The most replacements one move can meet: the scratch lists of [`destination`] hold this
/// many.
const MOST: usize = 16;

/// The zone the object of `proposal`, about to be put into its `to`, goes to after the
/// replacement effects that apply to it, read while the layer results are current.
///
/// Applying them follows [CR 616.1]: "If two or more replacement and/or prevention effects are
/// attempting to modify the way an event affects an object or player, the affected object's
/// controller (or its owner if it has no controller) or the affected player chooses one to
/// apply". The effects that apply and have not applied are collected, one is applied, and
/// they are collected again ([CR 616.1f]): "Once the chosen effect has been applied, this
/// process is repeated (taking into account only replacement or prevention effects that would
/// now be applicable) until there are no more left to apply."
///
/// Effects that would put the object into the same zone are one candidate, applied together:
/// which of two Rest in Peace exiles a card makes no difference. A self-replacement effect
/// ([CR 616.1a]), such as "counter it; if it is countered this way, exile it instead", was
/// applied by the instruction that asked for the move, before it got here. No replacement of a
/// move changes control, copies or turns a face (CR 616.1b to 616.1d), so every candidate here
/// is of [CR 616.1e]: "Any of the applicable replacement and/or prevention effects may be
/// chosen." Where two candidates differ, they are applied in the order they are found, which
/// no card of a game makes happen yet.
///
/// With no replacement of a move on the battlefield or on the object, it is one AND and an
/// empty search of the Effects store.
///
/// [`Full`] past [`MOST`] replacements.
///
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 616.1f]: https://yawgatog.com/resources/magic-rules/#R6161f
/// [CR 616.1a]: https://yawgatog.com/resources/magic-rules/#R6161a
/// [CR 616.1e]: https://yawgatog.com/resources/magic-rules/#R6161e
#[inline]
fn destination<L: Layout>(s: &State<L>, t: &GameTable, proposal: &Move<L>) -> Result<Zone, Full> {
    let statics = s
        .replacing()
        .is_some_and(|r| r.replaces(ReplacementKind::Move));
    if !statics
        && s.move_replacements(s.object_ref(proposal.slot))
            .next()
            .is_none()
    {
        return Ok(proposal.to);
    }
    replaced_destination(s, t, proposal, statics)
}

/// [`destination`] once some replacement of a move may apply: the loop of CR 616.1.
#[inline(never)]
fn replaced_destination<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    proposal: &Move<L>,
    statics: bool,
) -> Result<Zone, Full> {
    let (slot, mut to) = (proposal.slot, proposal.to);
    let object = s.object_ref(slot);
    let mut applied: Fixed<Option<Source<L::Slot>>, MOST> = Fixed::new();
    loop {
        let mut found: Fixed<Option<(Source<L::Slot>, Zone)>, MOST> = Fixed::new();
        let mut add = |source, instead| {
            if applied.as_slice().contains(&Some(source)) {
                Ok(())
            } else {
                found.push(Some((source, instead)))
            }
        };
        for r in s.move_replacements(object) {
            if let Some(instead) = effect_destination(r, to) {
                add(Source::Effect(r), instead)?;
            }
        }
        if statics {
            for (permanent, ability, instead) in static_destinations(s, t, slot, to) {
                add(Source::Ability(permanent, ability), instead)?;
            }
            if to == Zone::Battlefield {
                entering_destinations(s, t, proposal, &mut add)?;
            }
        }
        let Some(&Some((_, instead))) = found.as_slice().first() else {
            return Ok(to);
        };
        debug_assert!(
            found.iter().flatten().all(|&(_, zone)| zone == instead),
            "the affected player would choose between replacements putting it into different zones"
        );
        for &(source, zone) in found.iter().flatten() {
            if zone == instead {
                applied.push(Some(source))?;
            }
        }
        to = instead;
    }
}

/// Where an entry of the Effects store puts an object that would be put into `to`, if it
/// applies: "exile it instead" of a graveyard, or a flashback cost's, which exiles the spell
/// wherever else it would go ([CR 702.34a]): "If the flashback cost was paid, exile this card
/// instead of putting it anywhere else any time it would leave the stack."
///
/// [CR 702.34a]: https://yawgatog.com/resources/magic-rules/#R70234a
fn effect_destination(r: MoveReplacement, to: Zone) -> Option<Zone> {
    let applies = match r {
        MoveReplacement::ExileInsteadOfGraveyard => to == Zone::Graveyard,
        MoveReplacement::Flashback => to != Zone::Exile,
    };
    applies.then_some(Zone::Exile)
}

/// The static abilities of permanents that replace the object in `slot` being put into `to`,
/// each with the zone it goes to instead, in the order of the battlefield: "If a card or token
/// would be put into a graveyard from anywhere, exile it instead." The filter is read for the
/// permanent's controller.
fn static_destinations<'a, L: Layout>(
    s: &'a State<L>,
    t: &'a GameTable,
    slot: L::Slot,
    to: Zone,
) -> impl Iterator<Item = (L::Slot, AbilityIx, Zone)> + 'a {
    s.battlefield()
        .iter()
        .filter(move |&&p| s.copiable(p).may_replace(t).replaces(ReplacementKind::Move))
        .flat_map(move |&p| {
            let controller = s.controller(p);
            (0u8..)
                .zip(trigger::face(s, t, p).abilities)
                .filter_map(move |(index, a)| {
                    let Some(&Replacement {
                        replaces: Replaced::PutInto(objects, zone),
                        rewrite: Rewrite::ToZone(instead),
                    }) = a.replacement()
                    else {
                        return None;
                    };
                    let ability = AbilityIx::new(index);
                    let applies = a.kind() == AbilityKind::Replacement
                        && zone == to
                        && s.has_ability(p, ability)
                        && interpret::static_condition_holds(s, t, p, ability, a)
                        && objects.matches(s, t, slot, controller);
                    if !applies {
                        return None;
                    }
                    crate::coverage::hit(a);
                    Some((p, ability, instead))
                })
        })
}

/// Offers to `add` each static ability of a permanent that replaces the object of `proposal`
/// entering the battlefield as its `entry` says, with the zone it goes to instead, in the order
/// of the battlefield: "If a nontoken creature would enter and it wasn't cast, exile it
/// instead." The filter is read for the permanent's controller, of the object as it would
/// exist on the battlefield ([CR 614.12]): "To determine which replacement effects apply and
/// how they apply, check the characteristics of the permanent as it would exist on the
/// battlefield". An enchantment that another effect would make a creature there is a
/// creature. A permanent entering is not yet on the battlefield, so its own such ability does
/// not replace its entering, nor that of anything entering with it.
///
/// The state as it would be with the object entered is made only for an ability left to ask
/// ([`layers::entered`]); [`Full`] if making it, or `add`, ran out of room.
///
/// [CR 614.12]: https://yawgatog.com/resources/magic-rules/#R61412
fn entering_destinations<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    proposal: &Move<L>,
    mut add: impl FnMut(Source<L::Slot>, Zone) -> Result<(), Full>,
) -> Result<(), Full> {
    let Move {
        slot, entry, cast, ..
    } = *proposal;
    for &p in s.battlefield() {
        if !s.copiable(p).may_replace(t).replaces(ReplacementKind::Move) {
            continue;
        }
        let controller = s.controller(p);
        for (index, a) in (0u8..).zip(trigger::face(s, t, p).abilities) {
            let Some(&Replacement {
                replaces: Replaced::Enters { objects, not_cast },
                rewrite: Rewrite::ToZone(instead),
            }) = a.replacement()
            else {
                continue;
            };
            let ability = AbilityIx::new(index);
            let applies = a.kind() == AbilityKind::Replacement
                && !(not_cast && cast)
                && s.has_ability(p, ability)
                && interpret::static_condition_holds(s, t, p, ability, a);
            if !applies {
                continue;
            }
            let passes = layers::entered(s, t, slot, entry, |as_it_would_exist| {
                objects.matches(as_it_would_exist, t, slot, controller)
            })?;
            if !passes {
                continue;
            }
            crate::coverage::hit(a);
            add(Source::Ability(p, ability), instead)?;
        }
    }
    Ok(())
}

/// A replacement of draws by a static ability of a permanent ([`Replaced::Draw`]) that applies
/// to a draw: the permanent, the index of the ability on its face, the ability and the
/// permanent's controller, for whom the players it names are read (CR 109.5).
#[derive(Clone, Copy, Debug)]
pub(crate) struct DrawReplacement<L: Layout> {
    pub permanent: L::Slot,
    pub index: AbilityIx,
    pub ability: &'static crate::card::Ability,
    pub controller: Seat,
}

impl<L: Layout> DrawReplacement<L> {
    /// Whether applying `other` instead would do the same: the same ability of the same
    /// card, controlled by the same player. Which of two Chains of the same player's applies
    /// first makes no difference, since the other then applies to the draw that replaced it.
    fn same_as(&self, other: &Self) -> bool {
        std::ptr::eq(self.ability, other.ability) && self.controller == other.controller
    }

    /// What the rules record as having applied ([`Sink::applied`]).
    pub fn applied(&self, s: &State<L>) -> (ObjectRef<L>, u8) {
        (s.object_ref(self.permanent), self.index.index() as u8)
    }

    /// Who caused what happens instead, should it end the game.
    pub fn cause(&self, s: &State<L>, t: &GameTable) -> LossCause<L> {
        let (card, face) = s.copiable(self.permanent).record(t);
        LossCause::Effect {
            source: Some(s.object_ref(self.permanent)),
            card,
            face,
            ability: Some(FaceAbility::Own(self.index)),
        }
    }
}

/// The replacement of `player`'s next draw by a permanent that applies now, if any. It
/// replaces a draw from an empty library too ([CR 614.11]): "Some effects replace card draws.
/// These effects are applied even if no cards could be drawn because there are no cards in the
/// affected player's library."
///
/// A replacement that has applied to this draw, or to a draw it replaced, is not offered again
/// ([`Sink::applied`], CR 614.5). Of those left, where two would do different things, the
/// drawing player chooses which applies first ([CR 616.1]): "the affected object's controller
/// (or its owner if it has no controller) or the affected player chooses one to apply". The
/// others may then apply to whatever replaced the draw (CR 616.1f). With every candidate the
/// same, the first in the order of the battlefield applies.
///
/// [`Full`] past [`MOST`] replacements, or where the choice stops the run ([`answer`]).
///
/// [CR 614.11]: https://yawgatog.com/resources/magic-rules/#R61411
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
pub(crate) fn draw<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
) -> Result<Option<DrawReplacement<L>>, Full> {
    let (s, t) = (&*game.state, game.table);
    let mut found: Fixed<Option<DrawReplacement<L>>, MOST> = Fixed::new();
    for &permanent in s.battlefield() {
        if !s
            .copiable(permanent)
            .may_replace(t)
            .replaces(ReplacementKind::Draw)
        {
            continue;
        }
        let controller = s.controller(permanent);
        for (index, ability) in (0u8..).zip(trigger::face(s, t, permanent).abilities) {
            let Some(&Replacement {
                replaces: Replaced::Draw(players, draws),
                ..
            }) = ability.replacement()
            else {
                continue;
            };
            let index = AbilityIx::new(index);
            let applies = replacement::replaces_draws(ability)
                && filter::is_for(&players, player, controller)
                && replaces_this_draw(s, player, draws)
                && s.has_ability(permanent, index)
                && interpret::static_condition_holds(s, t, permanent, index, ability)
                && !game
                    .sink
                    .applied
                    .as_slice()
                    .contains(&(s.object_ref(permanent), index.index() as u8));
            if applies {
                found.push(Some(DrawReplacement {
                    permanent,
                    index,
                    ability,
                    controller,
                }))?;
            }
        }
    }
    let Some(&Some(first)) = found.as_slice().first() else {
        return Ok(None);
    };
    let chosen = if found.iter().flatten().all(|r| r.same_as(&first)) {
        first
    } else {
        choose_draw_replacement(game, player, &found)?
    };
    crate::coverage::hit(chosen.ability);
    Ok(Some(chosen))
}

/// The drawing player's choice among the replacements of their draw in `found`, which do not
/// all do the same ([`draw`]): one candidate for each that differs from those before it.
#[cold]
fn choose_draw_replacement<L: Layout>(
    game: &mut Game<'_, L>,
    player: Seat,
    found: &Fixed<Option<DrawReplacement<L>>, MOST>,
) -> Result<DrawReplacement<L>, Full> {
    let mut candidates = <L::Queue<Opt<L>, 64>>::default();
    let mut firsts: Fixed<Option<DrawReplacement<L>>, MOST> = Fixed::new();
    for r in found.iter().flatten() {
        if !firsts.iter().flatten().any(|f| f.same_as(r)) {
            firsts.push(Some(*r))?;
            candidates.push(Opt::Object(r.permanent))?;
        }
    }
    let d = Decision::options(
        player,
        DecisionKind::ChooseObject,
        candidates,
        Prompt::new(Purpose::Replace),
    );
    let chosen = usize::from(answer(game.state, game.sink, &d)?.0);
    Ok(firsts.as_slice()[chosen].expect("a candidate"))
}

/// Whether a replacement of `player`'s draws that replaces those `draws` replaces the one they
/// would make now. Every draw but the first one they draw in their own draw step
/// ([CR 504.1]): "First, the active player draws a card." That is the count of cards they
/// have drawn in their draw step this turn, which counts only cards drawn: a draw that was
/// replaced never happened ([CR 614.6]), so the next one is still the first.
///
/// [CR 504.1]: https://yawgatog.com/resources/magic-rules/#R5041
/// [CR 614.6]: https://yawgatog.com/resources/magic-rules/#R6146
fn replaces_this_draw<L: Layout>(s: &State<L>, player: Seat, draws: Draws) -> bool {
    match draws {
        Draws::Each => true,
        Draws::ExceptFirstInDrawStep => {
            let turn = s.turn();
            let first = turn.step() == TurnStep::Draw
                && turn.active() == player
                && s.counted(Count::DrewInDrawStep, player) == 0;
            !first
        }
    }
}

/// Applies the permanent's own replacement effects that modify how it enters (CR 614.12):
/// they "check the characteristics of the permanent as it would exist on the battlefield".
/// One it would not have there does not apply ([`layers::lost_entering`]): under an effect
/// that makes nonbasic lands Mountains, a land that would enter tapped unless its controller
/// paid life enters untapped, and no life is paid. Each changes only how it enters and none
/// conflicts with another, so each applies in turn. A choice one requires is made here, before
/// the permanent enters ([CR 614.12a]): "If a replacement effect that modifies how a permanent
/// enters the battlefield requires a choice, that choice is made before the permanent enters
/// the battlefield." Then the planeswalker's intrinsic one
/// ([CR 306.5b]): "A planeswalker has the intrinsic ability 'This permanent enters with a
/// number of loyalty counters on it equal to its printed loyalty number.'"
///
/// [CR 306.5b]: https://yawgatog.com/resources/magic-rules/#R3065b
/// [CR 614.12a]: https://yawgatog.com/resources/magic-rules/#R61412a
fn entering<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    mut entry: Entry<L>,
) -> Result<Entry<L>, Full> {
    let o = s.object(slot);
    let face = s.copiable(slot).turned(entry.face).face(t);
    let controller = entry.controller.unwrap_or(o.owner());
    // Found at the first replacement of entering: most permanents have none.
    let mut lost = None;
    let mut pays_life = false;
    for (n, ability) in face.abilities.iter().enumerate() {
        let Some(replacement) = ability.replacement() else {
            continue;
        };
        if replacement.replaces != Replaced::ThisEnters {
            continue;
        }
        let lost = match lost {
            Some(lost) => lost,
            None => *lost.insert(layers::lost_entering(s, t, slot, entry, face)?),
        };
        if lost.contains(AbilityIx::new(n as u8)) {
            continue;
        }
        match replacement.rewrite {
            Rewrite::EntersTapped => entry.tapped = true,
            Rewrite::EntersTappedUnlessFewLands(n) => {
                // Not yet on the battlefield, so every land counted is another land.
                let lands = s
                    .battlefield()
                    .iter()
                    .filter(|&&p| {
                        let d = s.derived(p);
                        d.controller == controller && d.types.contains(Types::LAND)
                    })
                    .count();
                if lands > usize::from(n) {
                    entry.tapped = true;
                }
            }
            Rewrite::EntersTappedUnlessPayLife(_) => {
                pays_life = entry.pay_life;
                if !entry.pay_life {
                    entry.tapped = true;
                }
            }
            Rewrite::Choose(kind) => {
                let value = choose_value(s, t, sink, slot, controller, kind)?;
                sink.chosen.push((slot, value));
            }
            // The card table holds no other rewrite of entering.
            Rewrite::None | Rewrite::ToZone(_) | Rewrite::Instead | Rewrite::Dredge(_) => continue,
        }
        crate::coverage::hit(ability);
    }
    // Life is paid only to an ability that applied.
    entry.pay_life = pays_life;
    if let Some(loyalty) = face.loyalty {
        entry.counters = Some((CounterKind::Loyalty, loyalty));
    }
    Ok(entry)
}

/// The life an "unless you pay N life" replacement of the entering object asks for, if it has
/// one ([`Rewrite::EntersTappedUnlessPayLife`]), with the index of its ability.
fn life_asked(face: &crate::card::Face) -> Option<(AbilityIx, u8)> {
    (0u8..)
        .zip(face.abilities)
        .find_map(|(n, a)| match a.replacement()?.rewrite {
            Rewrite::EntersTappedUnlessPayLife(life) => Some((AbilityIx::new(n), life)),
            _ => None,
        })
}

/// Pays the life an "unless you pay N life" entering replacement asks for
/// ([CR 119.4]): "If a player pays life, the payment is subtracted from their life total; in
/// other words, the player loses that much life."
///
/// [CR 119.4]: https://yawgatog.com/resources/magic-rules/#R1194
pub(crate) fn pay_life_to_enter<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<(), Full> {
    let o = s.object(slot);
    let face = s.copiable(slot).turned(entry.face).face(t);
    let player = entry.controller.unwrap_or(o.owner());
    if let Some((ability, n)) = life_asked(face)
        && s.has_ability(slot, ability)
    {
        // The life is lost to the permanent's own replacement effect ([CR 614.1c]: "Effects
        // that read ... "As [this permanent] enters . . . ," ... are replacement effects."),
        // not to whatever puts the permanent onto the battlefield.
        //
        // [CR 614.1c]: https://yawgatog.com/resources/magic-rules/#R6141c
        sink.enter_source(|| s.object_ref(slot));
        let lost = life::lose(&mut Game::new(s, t, sink), player, n);
        sink.leave();
        lost?;
    }
    Ok(())
}

/// The life an entering replacement of the object in `slot` asks for as it enters as
/// `entry` says, if any: none when the object would not have that ability on the
/// battlefield ([`layers::lost_entering`], CR 614.12).
///
/// [`Full`] if finding what it would have ran out of room.
pub(crate) fn life_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Option<u8>, Full> {
    let face = s.copiable(slot).turned(entry.face).face(t);
    let Some((ability, life)) = life_asked(face) else {
        return Ok(None);
    };
    let lost = layers::lost_entering(s, t, slot, entry, face)?;
    Ok((!lost.contains(ability)).then_some(life))
}

/// Whether `object`, about to move to `to`, is exiled instead by an entry of the Effects store
/// on it ([`effect_destination`]), for an instruction that takes a spell off the stack and
/// must know where its card goes before it moves.
pub(crate) fn exiled_instead<L: Layout>(s: &State<L>, object: ObjectRef<L>, to: Zone) -> bool {
    s.move_replacements(object)
        .any(|r| effect_destination(r, to).is_some())
}

/// Whether a replacement effect's choice can come up ([CR 616.1]): an optional replacement
/// may apply during the next resolution or step, or a replacement of draws whose instructions
/// may ask a question. The engine runs each resolution and step that may meet such a choice on
/// a copy of the state, so the choice can pause it ([`crate::asking`]). A run that does not run
/// on the copy cannot pause, and takes the first candidate of any choice it meets ([`answer`]),
/// which declines an optional replacement.
///
/// The one optional replacement is dredge, so this holds in every game with a card that has
/// dredge, wherever that card is. Dredge works from the graveyard as the draw happens
/// ([CR 702.52a]), and the same run can put a dredge card there before it draws, as an
/// ability that mills two cards and then draws one does. A replacement of draws by
/// instructions ([`ReplacementKind::DrawInstructions`]) may ask too ("discards a card"), and
/// the drawing player chooses the order of it and another replacement of the draw ([`draw`]),
/// so this holds in every game with a card that has one: such a card may enter the battlefield
/// during the run. A game whose cards have neither pays two ANDs. Two replacements of a move
/// that would put an object into different zones would be a choice too, but no card yet puts
/// one anywhere but exile, so [`destination`] has nothing to choose between. Two replacements
/// of a draw that each end the game, of permanents of different players, would differ too: no
/// card of the pool makes that happen, and the choice then takes its first candidate.
///
/// [CR 616.1]: https://yawgatog.com/resources/magic-rules/#R6161
/// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
#[inline]
pub(crate) fn may_ask(t: &GameTable) -> bool {
    t.can(Capability::Dredge) || t.replaces_any().replaces(ReplacementKind::DrawInstructions)
}

/// Whether a resolution may meet a choice it did not declare it asks: a replacement effect's
/// ([`may_ask`]), or a value a permanent chooses as it enters, which any resolution may put
/// onto the battlefield in a game with a card that chooses one
/// ([`Capability::ChoosesAsEnters`]). A game with neither pays the one AND [`may_ask`] does.
#[inline]
pub(crate) fn resolution_may_ask(t: &GameTable) -> bool {
    t.can_either(Capability::Dredge, Capability::ChoosesAsEnters)
}

/// N of the card in `slot`'s dredge, with its ability, if it has dredge.
fn dredge_of<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
) -> Option<(&'static crate::card::Ability, u8)> {
    s.copiable(slot)
        .face(t)
        .abilities
        .iter()
        .find_map(|a| match a.replacement() {
            Some(&Replacement {
                rewrite: Rewrite::Dredge(n),
                ..
            }) => Some((a, n)),
            _ => None,
        })
}

/// The cards in `player`'s graveyard whose dredge could replace their next draw
/// ([CR 702.52a]): "As long as you have at least N cards in your library, if you would draw a
/// card, you may instead mill N cards and return this card from your graveyard to your hand."
/// Dredge works only there, and only for the card's owner, whose graveyard it is.
///
/// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
pub(crate) fn dredges<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Bitboard<L::Slot> {
    let library = s.library(player).len();
    s.in_candidate_order(s.zone(ZoneKey::Of(player, Zone::Graveyard)))
        .filter(|&slot| dredge_of(s, t, slot).is_some_and(|(_, n)| library >= usize::from(n)))
        .collect()
}

/// Offers `player` the dredge abilities that could replace their next draw, if any
/// ([`dredges`]), and carries out the one they choose: they mill N cards, then return the
/// card from their graveyard to their hand ([CR 702.52a]). Returns whether the draw was
/// replaced. One draw is replaced by at most one dredge (CR 616.1f: once one has applied there
/// is no draw left to replace).
///
/// The choice is among the dredge cards and the draw itself ([`Purpose::Replace`]). A
/// replacement of draws by a permanent is not optional, so declining every dredge leaves it to
/// apply ([`draw`]), which is what applying it first would do.
///
/// [CR 702.52a]: https://yawgatog.com/resources/magic-rules/#R70252a
pub(crate) fn dredge<L: Layout>(game: &mut Game<'_, L>, player: Seat) -> Result<bool, Full> {
    let cards = dredges(game.state, game.table, player);
    if cards.is_empty() {
        return Ok(false);
    }
    let mut options = <L::Queue<Opt<L>, 64>>::default();
    options.push(Opt::Done)?;
    for slot in game.state.in_candidate_order(cards) {
        options.push(Opt::Object(slot))?;
    }
    let d = Decision::options(
        player,
        DecisionKind::ChooseUpTo,
        options,
        Prompt::new(Purpose::Replace),
    );
    let Some(Opt::Object(card)) = d.candidates.option(answer(game.state, game.sink, &d)?) else {
        return Ok(false);
    };
    let (ability, n) = dredge_of(game.state, game.table, card).expect("a dredge card");
    crate::coverage::hit(ability);
    let s = &*game.state;
    game.sink.enter_source(|| s.object_ref(card));
    let done = actions::mill::mill(game, player, n)
        .and_then(|_| zones::move_all(game, Bitboard::single(card), Zone::Hand, Reason::Effect));
    game.sink.leave();
    done.map(|()| true)
}

/// The value of `kind` that the permanent in `slot` chooses as it enters under `player`'s
/// control ([`Rewrite::Choose`]), asked through the record of answers ([`answer`]).
fn choose_value<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    slot: L::Slot,
    player: Seat,
    kind: ChosenKind,
) -> Result<ChosenValue<L>, Full> {
    let d = chosen::decision(t, player, kind, Bitboard::single(slot))?;
    let c = answer(s, sink, &d)?;
    Ok(chosen::value(kind, d.candidates.option(c)))
}

/// The kind of value the face `face` chooses as it enters, if it chooses one
/// ([`Rewrite::Choose`]), with the index of its ability.
fn chooses(face: &crate::card::Face) -> Option<(AbilityIx, ChosenKind)> {
    (0u8..)
        .zip(face.abilities)
        .find_map(|(n, a)| match a.replacement()?.rewrite {
            Rewrite::Choose(kind) => Some((AbilityIx::new(n), kind)),
            _ => None,
        })
}

/// The kind of value the object in `slot` chooses as it enters as `entry` says, if any: none
/// when it would not have that ability on the battlefield ([`layers::lost_entering`],
/// CR 614.12).
///
/// [`Full`] if finding what it would have ran out of room.
pub(crate) fn chosen_to_enter<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    slot: L::Slot,
    entry: Entry<L>,
) -> Result<Option<ChosenKind>, Full> {
    let face = s.copiable(slot).turned(entry.face).face(t);
    let Some((ability, kind)) = chooses(face) else {
        return Ok(None);
    };
    let lost = layers::lost_entering(s, t, slot, entry, face)?;
    Ok((!lost.contains(ability)).then_some(kind))
}

/// The answer to a choice `d` asked below code that has no resolution to ask through: a
/// replacement effect's choice (CR 616.1), or a value a permanent chooses as it enters. One
/// answer is taken silently, except under [`Stops::Forced`]. Otherwise the run's record
/// answers it, or the run stops at it ([`crate::asking`]), returning [`Full`] for whoever
/// opened the run to tell apart. A run that cannot stop, which a choice no state could
/// foresee meets, takes the first candidate: declining, for an optional replacement.
fn answer<L: Layout>(s: &State<L>, sink: &mut Sink<L>, d: &Decision<L>) -> Result<Choice, Full> {
    debug_assert!(!d.is_empty(), "a choice with no candidate");
    if d.len() == 1 && s.profile().stops != Stops::Forced {
        return Ok(Choice(0));
    }
    let asking = &mut sink.asking;
    if let Some(c) = asking.recorded() {
        return Ok(c);
    }
    if asking.is_open() {
        return Err(asking.stop(d.clone()));
    }
    Ok(Choice(0))
}
