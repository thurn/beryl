//! The definition's moments.

use std::fmt::Write as _;

use beryl_cards::names::{
    COUNTERSPELL, ISLAND, MANA_DRAIN, OUST, PLAINS, REPRIEVE, SINK_INTO_STUPOR, STOCK_UP, SWAMP,
    THIEVING_SKYDIVER,
};
use beryl_engine::card::{AbilityKind, CardDef, CostAction, Face, Measure, Needs, Timing, Types};
use beryl_engine::state::CounterKind;
use beryl_engine::{DecisionKind, Seat, TurnStep};
use beryl_scenario::{Action, P0, P1, Permanent, Scenario};

use super::{Candidate, Kind, leak};

/// Twelve lands for each library: enough to draw from for the turns a moment plays.
const LANDS: &[&str] = &[
    PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND, PLAINS, ISLAND,
];

/// The opponent's lands: the {1}{U}{U} for Sink into Stupor, and two more.
const THEIR_LANDS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
];

/// A land in the human's hand: it keeps them stopping in their main phase after what a moment
/// plays, so the board shows what it left before the turn moves on.
const LAND_IN_HAND: &[&str] = &[PLAINS];

/// The opponent's lands and a tapped creature, for an ability that targets one.
const TARGETS: &[Permanent] = &[
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(ISLAND),
    Permanent::new(PLAINS),
    Permanent::new(PLAINS),
    Permanent::new(THIEVING_SKYDIVER).tapped(),
];

/// The opponent's hand: Sink into Stupor answers any spell or ability with the card on the
/// battlefield, so from their seat the game stops on each one, as an opponent holding a
/// response does.
pub(super) const THEIR_HAND: &[&str] = &[SINK_INTO_STUPOR, COUNTERSPELL, PLAINS];

/// A mid-game table around the card: life off 20, graveyards, the opponent's lands and hand.
/// The human's hand stays empty: a land in it would add a stop of its own.
pub(super) fn table(name: &'static str, turn: u16, active: Seat, step: TurnStep) -> Scenario {
    Scenario::new(name)
        .at(turn, active, step)
        .life(P0, 17)
        .life(P1, 14)
        .graveyard(P0, &[STOCK_UP, REPRIEVE])
        .battlefield(P1, THEIR_LANDS)
        .hand(P1, THEIR_HAND)
        .graveyard(P1, &[OUST, MANA_DRAIN])
        .library(P0, LANDS)
        .library(P1, LANDS)
}

/// The abilities the definition's moments cover: activated abilities other than mana
/// abilities, of the front face, by index among its abilities.
pub(super) fn activated(face: &Face) -> Vec<(u8, &'static beryl_engine::card::Ability)> {
    face.abilities
        .iter()
        .enumerate()
        .filter(|(_, a)| a.kind == AbilityKind::Activated)
        .filter(|(_, a)| !a.cost.does(CostAction::Discard))
        .filter(|(_, a)| !matches!(a.cost.measure(), Some(Measure::TotalPower(_))))
        .filter_map(|(i, a)| Some((u8::try_from(i).ok()?, a)))
        .collect()
}

/// The card's activated abilities that `snapshot` names by index, each with its label: its
/// cost as the card prints it, for a reader of `specs-describe`.
pub fn ability_labels(def: &'static CardDef) -> Vec<(u8, String)> {
    activated(def.front())
        .into_iter()
        .map(|(k, a)| (k, cost_label(a, k)))
        .collect()
}

/// An ability's label: its cost, as its card prints it ("{2}{T}", "+2", "−1"), or its index
/// for a cost that is none of those.
pub(super) fn cost_label(a: &beryl_engine::card::Ability, k: u8) -> String {
    if let Some(l) = a.cost.loyalty_change() {
        return match l {
            0 => "0".to_owned(),
            l if l > 0 => format!("+{l}"),
            l => format!("\u{2212}{}", l.unsigned_abs()),
        };
    }
    let mana = a.cost.mana;
    let mut out = String::new();
    if mana.generic > 0 {
        let _ = write!(out, "{{{}}}", mana.generic);
    }
    for (symbol, &n) in ["W", "U", "B", "R", "G", "C"].iter().zip(&mana.symbols) {
        for _ in 0..n {
            let _ = write!(out, "{{{symbol}}}");
        }
    }
    if a.cost.does(CostAction::Tap) {
        out += "{T}";
    }
    if a.cost.does(CostAction::Sacrifice) {
        out += if out.is_empty() {
            "Sacrifice"
        } else {
            ", sacrifice"
        };
    }
    if out.is_empty() {
        format!("Ability {k}")
    } else {
        out
    }
}

/// Lands that pay for every one of `abilities`, one at a time: each color's most, and
/// Islands for the rest. None if a cost needs mana no implemented basic land makes.
pub(super) fn lands(abilities: &[(u8, &beryl_engine::card::Ability)]) -> Option<Vec<&'static str>> {
    // W, U, B, R, G, C, by `ManaColor` index.
    const BASICS: [Option<&str>; 6] = [Some(PLAINS), Some(ISLAND), Some(SWAMP), None, None, None];
    let mut most = [0u8; 6];
    let mut total = 0u8;
    for (_, a) in abilities {
        let cost = a.cost.mana;
        for (m, n) in most.iter_mut().zip(cost.symbols) {
            *m = (*m).max(n);
        }
        total = total.max(cost.generic + cost.symbols.iter().sum::<u8>());
    }
    let mut out = Vec::new();
    for (color, &n) in most.iter().enumerate() {
        if n > 0 {
            let land = BASICS[color]?;
            out.extend(std::iter::repeat_n(land, n.into()));
        }
    }
    while out.len() < total.into() {
        out.push(ISLAND);
    }
    Some(out)
}

/// The card as a permanent: with counters for an ability that removes them, and the loyalty
/// `ability` costs where its printed loyalty is less.
pub(super) fn permanent(
    def: &'static CardDef,
    face: &Face,
    ability: Option<&beryl_engine::card::Ability>,
) -> Permanent {
    let p = Permanent::new(def.name);
    let cost = ability
        .and_then(|a| a.cost.loyalty_change())
        .map_or(0, i8::unsigned_abs);
    if face.loyalty.is_some_and(|l| l < cost) {
        return p.counters(CounterKind::Loyalty, cost);
    }
    match face.abilities.iter().find_map(|a| a.cost.removes()) {
        Some(kind) => p.counters(kind, 2),
        None => p,
    }
}

fn probe(name: &str, title: &str, board: Scenario, play: Vec<Action>) -> Candidate {
    let cause = cause_of(&play);
    Candidate {
        name: name.to_owned(),
        title: title.to_owned(),
        board,
        play,
        kind: Kind::Probe,
        group: "",
        cause,
    }
}

/// The card the last cast or activation among `actions` was of.
pub(super) fn cause_of(actions: &[Action]) -> Option<&'static str> {
    actions.iter().rev().find_map(|a| match *a {
        Action::Cast { card, .. } | Action::Activate { card, .. } => Some(card),
        _ => None,
    })
}

pub(super) type Activated = [(u8, &'static beryl_engine::card::Ability)];

pub(super) fn from_definition(def: &'static CardDef) -> Vec<Candidate> {
    let face = def.front();
    if face.types.intersects(Types::INSTANT) || face.types.intersects(Types::SORCERY) {
        return Vec::new();
    }
    let abilities = activated(face);
    if abilities.is_empty() {
        return Vec::new();
    }
    let Some(lands) = lands(&abilities) else {
        return Vec::new();
    };
    let board = |ability: Option<&beryl_engine::card::Ability>, tapped: bool| {
        let lands = lands.iter().map(|&l| {
            let land = Permanent::new(l);
            if tapped { land.tapped() } else { land }
        });
        leak(
            std::iter::once(permanent(def, face, ability))
                .chain(lands)
                .collect(),
        )
    };
    let mut out = stops(&abilities, board(None, false), board(None, true));
    activations(def, &abilities, &|a| board(Some(a), false), &mut out);
    out
}

/// The moments of when the card stops its controller, and when it must not.
pub(super) fn stops(
    abilities: &Activated,
    open: &'static [Permanent],
    tapped: &'static [Permanent],
) -> Vec<Candidate> {
    let mut out = Vec::new();
    // Loyalty abilities are activated only as a sorcery could be cast (CR 606.3), so only
    // the others can stop a player outside their own main phase.
    if abilities
        .iter()
        .any(|(_, a)| a.timing == Timing::Instant && a.cost.loyalty_change().is_none())
    {
        let main = table("stops-main", 5, P0, TurnStep::PostcombatMain);
        out.push(probe(
            "stops-main",
            "Stops: main 2",
            main.battlefield(P0, open),
            Vec::new(),
        ));
        let end = table("stops-their-end", 6, P1, TurnStep::PrecombatMain);
        out.push(probe(
            "stops-their-end",
            "Stops: their end",
            end.battlefield(P0, open),
            Vec::new(),
        ));
    }
    if abilities
        .iter()
        .any(|(_, a)| a.cost.mana.generic > 0 || a.cost.mana.symbols.iter().any(|&n| n > 0))
    {
        // A land in hand stops the human here, to see what stays offered unpaid.
        let board = table("lands-tapped", 5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, tapped)
            .hand(P0, LAND_IN_HAND);
        out.push(probe("lands-tapped", "Lands tapped", board, Vec::new()));
    }
    if abilities.iter().any(|(_, a)| a.needs != Needs::Nothing) {
        // Nothing paid for and nothing for the ability that needs something: no stop.
        let board = table("nothing-to-do", 5, P0, TurnStep::PostcombatMain).battlefield(P0, tapped);
        out.push(probe("nothing-to-do", "Nothing to do", board, Vec::new()));
    }
    // The same board with what the ability needs: it stops the human here.
    for need in abilities
        .iter()
        .map(|(_, a)| a.needs)
        .filter(|&n| n != Needs::Nothing)
    {
        let (name, title, board) = match need {
            Needs::ExiledWithSource => {
                let mut met = tapped.to_vec();
                met[0] = met[0].exiled_with(&[OUST]);
                ("card-exiled", "Stops: card exiled", leak(met))
            }
            Needs::Nothing => continue,
        };
        if out.iter().any(|c| c.name == name) {
            continue;
        }
        let board = table(name, 5, P0, TurnStep::PostcombatMain).battlefield(P0, board);
        out.push(probe(name, title, board, Vec::new()));
    }
    out
}

/// Each ability activated, and the card bounced in response to one.
fn activations(
    def: &'static CardDef,
    abilities: &Activated,
    open: &dyn Fn(&beryl_engine::card::Ability) -> &'static [Permanent],
    out: &mut Vec<Candidate>,
) {
    for &(k, a) in abilities {
        let board = table("ability", 5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, open(a))
            .battlefield(P1, TARGETS)
            .hand(P0, LAND_IN_HAND);
        let play = vec![Action::activate(P0, def.name, k)];
        out.push(probe(
            &format!("ability-{k}"),
            &cost_label(a, k),
            board,
            play,
        ));
    }
    let bounceable = !def.front().types.intersects(Types::LAND);
    if let Some(&(k, a)) = abilities
        .iter()
        .find(|(_, a)| a.timing == Timing::Instant && !a.cost.does(CostAction::Sacrifice))
        .filter(|_| bounceable)
    {
        let board = table("bounced", 5, P0, TurnStep::PrecombatMain)
            .battlefield(P0, with_decoy(open(a)))
            .hand(P0, LAND_IN_HAND);
        let play = vec![
            Action::activate(P0, def.name, k),
            Action::cast(P1, SINK_INTO_STUPOR),
            Action::choose(P1, DecisionKind::Target, def.name),
        ];
        let title = format!("{}, bounced", cost_label(a, k));
        out.push(probe("bounced", &title, board, play));
    }
}

/// The board with a tapped Thieving Skydiver too: a second permanent for Sink into Stupor to
/// target, so that its target is asked rather than answered silently.
fn with_decoy(board: &'static [Permanent]) -> &'static [Permanent] {
    leak(
        board
            .iter()
            .copied()
            .chain(std::iter::once(Permanent::new(THIEVING_SKYDIVER).tapped()))
            .collect(),
    )
}
