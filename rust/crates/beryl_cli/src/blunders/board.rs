//! Rebuilding a game's board from a [`Board`] record, where the incident's replay no longer
//! reproduces its game.

use beryl_engine::card::FaceIx;
use beryl_engine::state::CounterKind;
use beryl_engine::{
    CardIx, GameTable, Profile, Seat, Slot, State, TurnStep, Zone, two_player::TwoPlayerSeat as _,
};
use beryl_session::with_stops;

use super::entries::Board;

pub(super) fn seat(p: u8) -> Result<Seat, String> {
    Seat::try_new(p).ok_or_else(|| format!("no seat {p}"))
}

fn step_named(name: &str) -> Result<TurnStep, String> {
    use TurnStep::{
        BeginningOfCombat, CombatDamage, DeclareAttackers, DeclareBlockers, Draw, End, EndOfCombat,
        FirstStrikeDamage, PostcombatMain, PrecombatMain, Upkeep,
    };
    [
        Upkeep,
        Draw,
        PrecombatMain,
        BeginningOfCombat,
        DeclareAttackers,
        DeclareBlockers,
        FirstStrikeDamage,
        CombatDamage,
        EndOfCombat,
        PostcombatMain,
        End,
    ]
    .into_iter()
    .find(|s| format!("{s:?}") == name)
    .ok_or_else(|| format!("no step {name:?} with priority"))
}

fn counter_named(name: &str) -> Result<CounterKind, String> {
    use CounterKind::{Chorus, Loyalty, MinusOne, PlusOne, Story, Time};
    [PlusOne, MinusOne, Loyalty, Time, Story, Chorus]
        .into_iter()
        .find(|k| format!("{k:?}") == name)
        .ok_or_else(|| format!("no counter {name:?}"))
}

/// The state `b` describes, in the game of `t`, as `beryl_scenario` arranges its setups:
/// every card starts in its owner's library and moves to its zone.
pub fn arrange(t: &GameTable, b: &Board) -> Result<State, String> {
    let profile = with_stops(Profile::SMART, b.stops, seat(b.human)?);
    let mut s = State::new(t, b.seed, profile).map_err(|e| e.to_string())?;
    s.board()
        .begin_at(b.turn, seat(b.active)?, step_named(&b.step)?);
    let full = |_| "a zone is full".to_owned();
    let from_library = |s: &State, p: Seat, name: &str| {
        s.library(p)
            .iter()
            .copied()
            .find(|&slot| s.name(t, slot) == name)
            .ok_or_else(|| format!("player {}'s deck has no {name} left", p.get()))
    };
    for (p, side) in Seat::BOTH.into_iter().zip(&b.players) {
        s.board().set_life(p, side.life);
        for h in &side.hand {
            let slot = from_library(&s, p, &h.card)?;
            s.board().place(slot, Zone::Hand).map_err(full)?;
            s.board().set_known(p.opponent(), slot, h.shown);
        }
    }
    let mut slots: Vec<Slot> = Vec::with_capacity(b.battlefield.len());
    for m in &b.battlefield {
        let owner = seat(m.owner)?;
        let face = match m.face {
            0 => FaceIx::FRONT,
            1 => FaceIx::BACK,
            255 => FaceIx::DOWN,
            f => return Err(format!("no face {f}")),
        };
        let slot = if m.token {
            let card: CardIx = t
                .index_named(&m.card)
                .ok_or_else(|| format!("no card makes a {}", m.card))?;
            let token = s.board().place_token(owner, card, face, m.one_one);
            token.map_err(full)?
        } else {
            let slot = from_library(&s, owner, &m.card)?;
            s.board().place(slot, Zone::Battlefield).map_err(full)?;
            s.board().set_face(slot, face);
            slot
        };
        s.board().set_controller(slot, seat(m.controller)?);
        s.board().set_tapped(slot, m.tapped);
        s.board().set_sick(slot, m.sick);
        s.board().set_damage(slot, m.damage);
        for (kind, n) in &m.counters {
            s.board().set_counters(slot, counter_named(kind)?, *n);
        }
        slots.push(slot);
    }
    let permanent = |i: u16| {
        slots
            .get(usize::from(i))
            .copied()
            .ok_or_else(|| format!("no permanent {i}"))
    };
    for (m, &slot) in b.battlefield.iter().zip(&slots) {
        if let Some(i) = m.attached_to {
            s.board().attach(slot, permanent(i)?);
        }
    }
    for (p, side) in Seat::BOTH.into_iter().zip(&b.players) {
        // Top first: the last placed is on top.
        for name in side.graveyard.iter().rev() {
            let slot = from_library(&s, p, name)?;
            s.board().place(slot, Zone::Graveyard).map_err(full)?;
        }
        for x in &side.exile {
            let slot = from_library(&s, p, &x.card)?;
            s.board().place(slot, Zone::Exile).map_err(full)?;
            if let Some(i) = x.with {
                s.board().set_exiled_with(slot, permanent(i)?);
            }
            if let Some(i) = x.until {
                s.board().set_exiled_until(slot, permanent(i)?);
            }
        }
        let mut order: Vec<Slot> = Vec::with_capacity(side.library.len());
        for name in &side.library {
            let slot = s
                .library(p)
                .iter()
                .copied()
                .find(|&slot| s.name(t, slot) == *name && !order.contains(&slot))
                .ok_or_else(|| format!("player {}'s library holds no more {name}", p.get()))?;
            order.push(slot);
        }
        if order.len() != s.library(p).len() {
            return Err(format!(
                "player {}'s library lists {} cards of {}",
                p.get(),
                order.len(),
                s.library(p).len()
            ));
        }
        s.board().order_library(p, &order);
    }
    Ok(s)
}
