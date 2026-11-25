//! Copying a spell ([CR 707.10]): "To copy a spell, activated ability, or triggered ability
//! means to put a copy of it onto the stack; a copy of a spell isn't cast and a copy of an
//! activated ability isn't activated."
//!
//! A copy of a spell is an object in a slot of the shared pool, which holds tokens and spell
//! copies, marked as a token: a copy of a permanent spell becomes a token as it resolves
//! (see [CR 608.3f]). A prepare spell uses the same pool representation, and is cast under
//! its own permission (see CR 723.3c). A copy made by this module takes the original's
//! copiable values with the copier's exception, and a stack entry with the original's face,
//! targets, X and casting options paid. No "when you cast" ability triggers and no count
//! of spells cast counts it. When it leaves the stack it goes where a spell's card would, and
//! the state-based check ends it there (see [CR 704.5e]): "If a copy of a spell is in a zone other
//! than the stack, it ceases to exist."
//!
//! [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
//! [CR 608.3f]: https://yawgatog.com/resources/magic-rules/#R6083f
//! [CR 704.5e]: https://yawgatog.com/resources/magic-rules/#R7045e
//!
//! Implements: CR 707.10

use beryl_card::{Amount, ExceptIx, Objects, Words};

use crate::additions::Addition;
use crate::bitboard::Bitboard;
use crate::card::Stop;
use crate::casting::{self, entry_ability};
use crate::decision::Opt;
use crate::fixed::Full;
use crate::game::Game;
use crate::ids::Seat;
use crate::ids::SlotId;
use crate::interpret::choices;
use crate::interpret::{Outcome, Run};
use crate::layout::{Layout, Queue};
use crate::query::Copiable;
use crate::replacement::event::{self, EventKind, Reason};
use crate::resolution::Resolution;
use crate::state::{AbilityTarget, ObjectRef, StackEntry, StackItem, Target, TargetPlace, Zone};

/// [`Effect::CopySpell`](beryl_card::Effect::CopySpell): "Copy [spells] [count] times[, except
/// ...]. You may choose new targets for the copies" (CR 707.10). Each spell still on the stack
/// is copied `count` times, the spells in the order their entries are on the stack, from the
/// bottom. The ability's own spell that has left the stack, as a storm trigger's countered
/// spell has, is copied as it last existed there ([`State::departed_spell`]), since the
/// ability exists independently of it (see [CR 113.7a]) and uses its last known information
/// (see [CR 608.2h]): "if it's no longer in that zone ... the effect uses the object's last known
/// information." The ability's controller controls and owns the copies ([CR 707.10]): "A copy
/// of a spell is owned by the player under whose control it was put on the stack." The
/// outcome is the copies.
///
/// [`State::departed_spell`]: crate::state::State::departed_spell
/// [CR 113.7a]: https://yawgatog.com/resources/magic-rules/#R1137a
/// [CR 608.2h]: https://yawgatog.com/resources/magic-rules/#R6082h
/// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
pub(crate) fn instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    spells: Objects,
    count: Amount,
    except: Option<ExceptIx>,
    new_targets: bool,
) -> Result<Outcome<L>, Stop> {
    let count = run.scene().amount(count);
    let chosen = run.scene().objects(spells);
    let entry = &run.resolution().entry;
    let controller = entry.controller();
    // See CR 707.9b: the exception is one the ability's own face lists, and the copy takes it
    // after those of what it copies. Its color words read through the text changes of the
    // spell or ability making the copy (see CR 612.1), as they are now.
    let addition = except.map(|ix| Addition::new(entry.card(), entry.face(), ix));
    let words = run.state().entry_words(entry);
    let departed = match spells {
        Objects::This => (entry.source())
            .filter(|&source| !run.state().is_current(source))
            .and_then(|source| run.state().departed_spell(source))
            .cloned(),
        Objects::Target(_)
        | Objects::AttachedTo
        | Objects::Those
        | Objects::TriggeredOn
        | Objects::TriggeredOnIn(_)
        | Objects::ExiledWithThis
        | Objects::LibraryTop { .. }
        | Objects::ExceptThose(_)
        | Objects::PutOntoBattlefieldWithThis
        | Objects::Carried
        | Objects::All { .. }
        | Objects::InZoneOf { .. }
        | Objects::CardsIn { .. }
        | Objects::Everywhere(_)
        | Objects::Paid(_)
        | Objects::Other(_) => None,
    };
    let copies = Copies {
        count,
        controller,
        addition,
        words,
        new_targets,
    };
    let mut made = Bitboard::EMPTY;
    // The copies go on above the entries there now, whose places stay as they are.
    for place in 0..run.state().stack().len() {
        let original = run.state().stack()[place].clone();
        let Some(slot) = (original.spell())
            .map(|spell| spell.slot)
            .filter(|&slot| chosen.contains(slot))
        else {
            continue;
        };
        let what = run.state().copiable(slot);
        copies.make(run, &original, what, &mut made)?;
    }
    if let Some(row) = departed {
        copies.make(run, &row.entry, row.copiable, &mut made)?;
    }
    Ok(Outcome::acted_on(made))
}

/// How an [`instruction`] makes the copies of each spell it copies.
struct Copies<L: Layout> {
    count: u8,
    controller: Seat,
    addition: Option<Addition<L::Card>>,
    /// The text changes of the spell or ability making the copies, which the exception's
    /// words read through.
    words: Words,
    new_targets: bool,
}

impl<L: Layout> Copies<L> {
    /// Puts `count` copies of the spell whose entry is `original` and whose copiable values
    /// are `what` on the stack, noting each in `made`.
    fn make(
        &self,
        run: &mut Run<'_, '_, L>,
        original: &StackEntry<L>,
        mut what: Copiable<L>,
        made: &mut Bitboard<L::Slot>,
    ) -> Result<(), Stop> {
        if let Some(addition) = self.addition {
            what = what.with_addition_read(addition, run.game().table, &self.words)?;
        }
        for _ in 0..self.count {
            let (game, resolution) = run.resolving();
            let mut copy = original.clone();
            if self.new_targets {
                choose_new_targets(game, resolution, self.controller, &mut copy)?;
            }
            made.insert(put_copy(game, self.controller, what, copy)?);
        }
        Ok(())
    }
}

/// Asks `controller` for each target of `copy` in the order they were chosen whether to keep it
/// or choose a new one, and sets each as it is answered
/// ([CR 707.10c](https://yawgatog.com/resources/magic-rules/#R70710c)): "The player may leave
/// any number of the targets unchanged, even if those targets would be illegal. If the player
/// chooses to change some or all of the targets, the new targets must be legal."
fn choose_new_targets<L: Layout>(
    game: &mut Game<'_, L>,
    resolution: &mut Resolution<'_, L>,
    controller: Seat,
    copy: &mut StackEntry<L>,
) -> Result<(), Stop> {
    copy.item = StackItem::Spell {
        object: ObjectRef::default(),
        controller,
        card: copy.card(),
        face: copy.face(),
        costs: copy.costs(),
        copied: true,
    };
    let Some(ability) = entry_ability(game.table, copy) else {
        return Ok(());
    };
    for i in 0..copy.targets.len() {
        let place =
            TargetPlace::new(u8::try_from(i).expect("a stack entry holds at most TARGETS targets"));
        let target = copy.targets.as_slice()[i];
        let others = casting::new_targets(game.state, game.table, copy, ability, place);
        let kept = option_of(target);
        let chosen = choices::retarget(game, resolution, controller, kept, others)?;
        let spec = target.spec();
        copy.targets.as_mut_slice()[i] = match chosen {
            Opt::Player(player) => Target::player(spec, player),
            Opt::Ability(place) => Target::ability(spec, place.into()),
            Opt::Object(slot) if chosen != kept => {
                Target::object(spec, game.state.object_ref(slot))
            }
            Opt::Object(_)
            | Opt::Pass
            | Opt::PlayLand(_)
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
            | Opt::Activate { .. }
            | Opt::Mana { .. }
            | Opt::Pool(_)
            | Opt::Number(_)
            | Opt::Plan(_)
            | Opt::TurnFaceUp(_) => target,
        };
    }
    Ok(())
}

/// A target as the option that names it.
fn option_of<L: Layout>(target: Target<L>) -> Opt<L> {
    if let Some(r) = target.as_object() {
        return Opt::Object(r.slot);
    }
    if let Some(player) = target.as_player() {
        return Opt::Player(player);
    }
    match target.as_ability() {
        Some(AbilityTarget::At(place)) => Opt::Ability(crate::layout::index::<L>(place)),
        // An ability that has left the stack: an option no other target is.
        _ => Opt::Done,
    }
}

/// Puts a copy with the copiable values `what` and the entry `copy` (the original's, its
/// targets chosen) on the stack under `controller`'s control, in a free slot of the shared
/// pool. Returns its slot. [`Full`] if the pool or the stack has no room: the game then
/// moves to the large layout, which has more of both.
fn put_copy<L: Layout>(
    game: &mut Game<'_, L>,
    controller: Seat,
    what: Copiable<L>,
    mut copy: StackEntry<L>,
) -> Result<L::Slot, Full> {
    let (s, t) = (&mut *game.state, game.table);
    let slot = L::Slot::pool()
        .find(|&slot| s.object(slot).zone() == Zone::Nowhere)
        .ok_or(Full)?;
    let face = s.make_token(t, slot, controller, what)?;
    // It is put on the stack, not cast (CR 707.10): no Cast event follows.
    event::move_one(s, t, game.sink, slot, Zone::Stack, Reason::Effect)?;
    let spell = s
        .spell(s.object_ref(slot))
        .expect("a spell copy just put on the stack");
    s.become_spell(spell, face, controller);
    let (card, _) = s.definition(slot).record(t);
    copy.item = StackItem::Spell {
        object: s.object_ref(slot),
        controller,
        card,
        face,
        costs: copy.costs(),
        copied: true,
    };
    s.put_on_stack(game.sink, copy)?;
    // See CR 707.10c: "the copy is put onto the stack with those targets."
    if t.listens_anywhere().hears(EventKind::Targeted) {
        casting::targeted(s, t)?;
    }
    Ok(slot)
}

/// [`Effect::CopyCard`](beryl_card::Effect::CopyCard): creates a copy of each card object
/// in `cards` in the same zone ([CR 707.12]) so an immediate cast instruction can cast it.
/// The outcome is the created copies.
///
/// [CR 707.12]: https://yawgatog.com/resources/magic-rules/#R70712
pub(crate) fn copy_card_instruction<L: Layout>(
    run: &mut Run<'_, '_, L>,
    cards: Objects,
) -> Result<Outcome<L>, Full> {
    let cards = run.scene().objects(cards);
    if cards.is_empty() {
        return Ok(Outcome::NOTHING);
    }
    let controller = run.resolution().entry.controller();
    let mut made = Bitboard::EMPTY;
    for &card_slot in run.in_game_order(cards)?.iter() {
        let game = run.game();
        let (s, t) = (&mut *game.state, game.table);
        let zone = s.object(card_slot).zone();
        if !matches!(
            zone,
            Zone::Exile | Zone::Graveyard | Zone::Hand | Zone::Library
        ) {
            continue;
        }
        let copy = L::Slot::pool()
            .find(|&slot| s.object(slot).zone() == Zone::Nowhere)
            .ok_or(Full)?;
        let what = s.copiable(card_slot).recorded(t);
        s.make_token(t, copy, controller, what)?;
        s.move_object(game.sink, copy, zone, crate::zone::Place::Top)?;
        made.insert(copy);
    }
    Ok(Outcome::acted_on(made))
}
