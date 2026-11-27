//! Manual payment: paying a cost one chosen mana unit at a time.
//!
//! Implements: none. Supports `pay::pay` and `pay::activate`: the manual choice of mana sources.

use crate::bitboard::Bitboard;
use crate::card::{ManaColor, ManaCost};
use crate::decision::{OPTIONS, Opt};
use crate::fixed::Full;
use crate::ids::Seat;
use crate::layout::{Layout, Queue, Table};
use crate::replacement::event::Sink;
use crate::state::State;
use crate::table::{Capability, GameTable, is_priced};

use super::pay::{activate, activate_consuming, tapped_source};
use super::pool::{helps, pay_unit};
use super::remembered::{self, Paying};
use super::solver::lowest_kind;
use super::sources::{Class, Sources, alternative_ability};
use super::{CLASSES, KINDS, Taps};

/// Manual payment (the `manual_mana` flag): the floating mana and the sources, one per
/// class, whose next unit pays toward `remaining` and leaves the rest payable.
///
/// Floating mana of a kind is one option, whether it is unrestricted or mana that remembers
/// the payment accepts: [`manual_pay`] spends the unrestricted first.
pub(crate) fn manual_options<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
    remaining: ManaCost,
    exclude: Bitboard<L::Slot>,
    paying: Paying<L>,
) -> Result<L::Queue<Opt<L>, OPTIONS>, Full> {
    let sources = Sources::of(s, t, player, exclude, paying)?;
    let pool = s.player(player).mana().amounts_u8();
    let rich =
        t.can_either(Capability::PricedMana, Capability::ManaChanges) && !sources.extras.is_empty();
    let mut out: L::Queue<Opt<L>, OPTIONS> = Default::default();
    for kind in 0..KINDS {
        if !helps(remaining, kind) {
            continue;
        }
        let mut rest = remaining;
        pay_unit(&mut rest, kind);
        let mut after = pool;
        let mut spent = 0;
        if pool[kind] > 0 {
            after[kind] -= 1;
        } else if let Some(i) =
            (sources.remembered().iter()).position(|&(_, k, _)| usize::from(k) == kind)
        {
            spent = 1 << i;
        } else {
            continue;
        }
        let ok = sources
            .units_left(after, &Table::filled(0), spent)
            .pays(rest)
            || (rich
                && super::rich::pays_from_state(
                    after,
                    &sources,
                    &Table::filled(0),
                    spent,
                    &[0; CLASSES],
                    rest,
                ));
        if ok {
            out.push(Opt::Pool(ManaColor::ALL[kind]))?;
        }
    }
    for (i, c) in sources.classes().iter().enumerate() {
        // A class's manual option keeps its existing representative for recorded choices.
        let slot = c.members.min(|slot| slot).expect("a class has a member");
        let mut taps: Taps<L> = Table::filled(0);
        taps[i] = 1;
        for kind in (0..KINDS).filter(|&k| c.kinds & (1 << k) != 0) {
            // A source with a restricted class names it by its kind: [`activate`] taps for
            // the unrestricted ability if it makes the kind.
            if c.is_restricted() && classes_make(&sources, c, kind) {
                continue;
            }
            let pays_rich = |p, cost| {
                rich && super::rich::pays_from_state(p, &sources, &taps, 0, &[0; CLASSES], cost)
            };
            let mut ok = false;
            if helps(remaining, kind) {
                let mut rest = remaining;
                pay_unit(&mut rest, kind);
                let mut after = pool;
                after[kind] = after[kind].checked_add(c.amount - 1).ok_or(Full)?;
                ok = sources.units(after, &taps).pays(rest) || pays_rich(after, rest);
            }
            if !ok && !c.is_restricted() {
                let mut after = pool;
                after[kind] = after[kind].checked_add(c.amount).ok_or(Full)?;
                ok = pays_rich(after, remaining);
            }
            if !ok {
                continue;
            }
            // A source that may also sacrifice itself for mana names that ability: its
            // option for the kind is its other mana ability's.
            if c.is_twin() && c.is_consumed() {
                let ability = alternative_ability(s, t, slot).expect("a twin's ability");
                out.push(Opt::Activate { slot, ability })?;
                break;
            }
            out.push(Opt::Mana {
                slot,
                color: ManaColor::ALL[kind],
            })?;
        }
    }
    if rich {
        super::rich::manual_extra_options(remaining, pool, &sources, &mut out)?;
    }
    Ok(out)
}

/// Performs a manual payment option, returning what remains to pay and the kind of the mana
/// it paid with, or `None` when the option only added mana to the pool (such as activating a
/// priced mana source or floating mana for its price).
pub(crate) fn manual_pay<L: Layout>(
    s: &mut State<L>,
    t: &GameTable,
    sink: &mut Sink<L>,
    player: Seat,
    mut remaining: ManaCost,
    option: Opt<L>,
    paying: Paying<L>,
) -> Result<(ManaCost, Option<ManaColor>), Full> {
    let made = s.player(player).remembered_mana().len();
    let rich = t.can_either(Capability::PricedMana, Capability::ManaChanges);
    let color = match option {
        Opt::Pool(color) => color,
        Opt::Mana { slot, color } => {
            sink.together();
            let activated = activate(s, t, sink, tapped_source(s, slot), color);
            sink.apart();
            activated?;
            if rich && !super::rich::manual_can_spend_after(s, t, player, remaining, color, paying)?
            {
                return Ok((remaining, None));
            }
            color
        }
        // Either a priced/multi-term mana ability (adds mana to the pool after paying its price
        // from the pool) or a source's twin sacrifice mana ability.
        Opt::Activate { slot, ability } => {
            let a = s
                .abilities(t, slot)
                .find(|&(id, _)| id == ability)
                .map(|(_, a)| a)
                .expect("an ability offered");
            if rich && (is_priced(a) || a.produces() == crate::card::ManaSet::NONE) {
                return super::rich::manual_pay_extra(
                    s, t, sink, player, remaining, slot, ability, paying,
                );
            }
            let kinds = a.produces().colors;
            let kind = (0..KINDS)
                .find(|&k| kinds & (1 << k) != 0 && helps(remaining, k))
                .unwrap_or_else(|| lowest_kind(kinds));
            let color = ManaColor::ALL[kind];
            sink.together();
            let activated = activate_consuming(s, t, sink, tapped_source(s, slot), color, true);
            sink.apart();
            activated?;
            if rich && !super::rich::manual_can_spend_after(s, t, player, remaining, color, paying)?
            {
                return Ok((remaining, None));
            }
            color
        }
        Opt::Pass
        | Opt::PlayLand(_)
        | Opt::Play
        | Opt::Draw
        | Opt::Keep
        | Opt::Mulligan
        | Opt::FreeMulligan
        | Opt::Trigger(_)
        | Opt::Yes
        | Opt::No
        | Opt::Done
        | Opt::Attack { .. }
        | Opt::AttackAll(_)
        | Opt::Defender(_)
        | Opt::Block(_)
        | Opt::Lethal(_)
        | Opt::Cast { .. }
        | Opt::Number(_)
        | Opt::Plan(_)
        | Opt::Player(_)
        | Opt::Ability(_)
        | Opt::Object(_)
        | Opt::TurnFaceUp(_)
        | Opt::Companion => unreachable!("a manual payment option"),
    };
    let pool = s.player(player).remembered_mana().len();
    if pool > made {
        // A restricted ability made it: the mana that remembers, last in the pool.
        remembered::spend(s, t, player, pool - 1, paying)?;
    } else if !beryl_kernel::Number::is_zero(s.player(player).mana()[color]) {
        s.spend_mana(player, color, 1);
    } else {
        let accepted = remembered::accepted_entries(s, t, player, paying);
        let &(at, ..) = (accepted.iter())
            .find(|&&(_, kind, _)| usize::from(kind) == color.index())
            .expect("mana of the kind an option names");
        remembered::spend(s, t, player, usize::from(at), paying)?;
    }
    pay_unit(&mut remaining, color.index());
    Ok((remaining, Some(color)))
}

/// Whether the class with the same member as the restricted class `c` makes `kind`: then that
/// class's option names the permanent tapped for it.
fn classes_make<L: Layout>(sources: &Sources<L>, c: &Class<L>, kind: usize) -> bool {
    c.twin()
        .is_some_and(|twin| sources.classes()[usize::from(twin)].kinds & (1 << kind) != 0)
}
