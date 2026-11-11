//! The scenarios' moments.

use beryl_cards::names::{
    FRACTURED_IDENTITY, ISLAND, MANA_DRAIN, OUST, PLAINS, REPRIEVE, SINK_INTO_STUPOR, STOCK_UP,
    TIME_SPIRAL,
};
use beryl_engine::DecisionKind;
use beryl_engine::card::{AbilityKind, CardDef, ObjectAbility, Types};
use beryl_scenario::{Action, Assertion, P0, P1, Permanent, Pick, Scenario};

use super::super::slug;
use super::definition::{THEIR_HAND, cause_of, cost_label};
use super::{Candidate, Kind, leak};

/// Whether an action answers one decision, as a spec's scripts play them.
pub(super) fn decides(action: &Action) -> bool {
    matches!(
        action,
        Action::PlayLand { .. }
            | Action::Cast { .. }
            | Action::Activate { .. }
            | Action::TurnFaceUp { .. }
            | Action::Answer { .. }
    )
}

/// A scenario's actions as a spec's scripts play them: its decisions and its turns to advance
/// to. Passing is what the scripts do anyway short of their next action, its assertions are
/// its own, and the session orders simultaneous triggers itself.
pub(in crate::specs) fn playable(scenario: &Scenario) -> Vec<Action> {
    scenario
        .actions
        .iter()
        .filter(|a| playground_action(a))
        .copied()
        .collect()
}

pub(in crate::specs) fn playground_action(action: &Action) -> bool {
    (decides(action) || matches!(action, Action::AdvanceTo { .. }))
        && !matches!(
            action,
            Action::Answer {
                kind: DecisionKind::TriggerOrder | DecisionKind::Priority,
                ..
            }
        )
}

/// The scenario on a table that looks like a game: life off 20, graveyards, and the
/// opponent's lands and hand, where it has none of its own, lands under each library so that
/// the game goes on after its actions, and a twin of the card in the human's hand where it
/// starts there, so that some moment of every card in hand holds two cards just alike (which
/// the UI must keep apart: which one moves, which one is offered). Each part is kept only if
/// the scenario still passes, its assertions and its scripted decisions alike, so nothing
/// added changes it: the table without the twin, then the libraries, are tried alone if the
/// whole fails.
fn dressed(def: &'static CardDef, scenario: &Scenario) -> Scenario {
    let passes = |d: &Scenario| {
        let checked = d.actions(leak(
            d.actions
                .iter()
                .map(|action| deeper(action, scenario, d))
                .collect(),
        ));
        beryl_scenario::run(&beryl_cards::CARDS, &checked).is_ok()
    };
    let deep = with_libraries(scenario);
    let full = with_table(&deep);
    let twinned = with_twin(def, &full);
    if twinned.is_some_and(|t| passes(&t)) {
        twinned.unwrap_or(full)
    } else if passes(&full) {
        full
    } else if passes(&deep) {
        deep
    } else {
        *scenario
    }
}

/// `scenario` with a second copy of `def` in the human's hand, if one starts there.
fn with_twin(def: &'static CardDef, scenario: &Scenario) -> Option<Scenario> {
    let hand = scenario.setup.players[P0.index()].hand;
    hand.contains(&def.name)
        .then(|| scenario.hand(P0, leak([hand, &[def.name]].concat())))
}

/// An assertion counting a whole library, counting the lands [`with_libraries`] adds too.
fn deeper(action: &Action, original: &Scenario, dressed: &Scenario) -> Action {
    match *action {
        Action::Assert {
            assertion:
                Assertion::Count {
                    player,
                    zone: beryl_engine::Zone::Library,
                    card: None,
                    count,
                },
        } => Action::assert(Assertion::Count {
            player,
            zone: beryl_engine::Zone::Library,
            card: None,
            count: count.saturating_add(
                u8::try_from(
                    dressed.setup.players[player.index()].library.len()
                        - original.setup.players[player.index()].library.len(),
                )
                .unwrap_or(u8::MAX),
            ),
        }),
        a => a,
    }
}

/// Cards under each player's library that neither seat can play from a scenario's board: no
/// land drop and no affordable spell, so whatever stops a player after the scenario is its
/// cards'.
const FILLER: &[&str] = &[
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
    TIME_SPIRAL,
    FRACTURED_IDENTITY,
];

/// [`FILLER`] under each player's library. A scenario about consuming a short
/// library needs its original size in both the spec and the playable example.
pub(in crate::specs) fn with_libraries(scenario: &Scenario) -> Scenario {
    let mut d = *scenario;
    for seat in [P0, P1] {
        let library = d.setup.players[seat.index()].library;
        let needs_short_library = library.len() <= 1
            && scenario.actions.iter().any(|action| {
                matches!(
                    action,
                    Action::Assert {
                        assertion: Assertion::Count {
                            player,
                            zone: beryl_engine::Zone::Library,
                            card: None,
                            count: 0,
                        },
                    } if *player == seat
                )
            });
        if !needs_short_library {
            d = d.library(seat, leak([library, FILLER].concat()));
        }
    }
    d
}

/// Life off 20, graveyards, and the opponent's lands and hand, where it has none of its own.
fn with_table(scenario: &Scenario) -> Scenario {
    const THEIRS: &[Permanent] = &[
        Permanent::new(ISLAND),
        Permanent::new(ISLAND),
        Permanent::new(ISLAND),
        Permanent::new(PLAINS).tapped(),
    ];
    let mut d = *scenario;
    let life_asserted = scenario.actions.iter().any(|a| {
        matches!(
            a,
            Action::Assert {
                assertion: Assertion::Life { .. }
            }
        )
    });
    if !life_asserted && d.setup.players.iter().all(|p| p.life == 20) {
        d = d.life(P0, 17).life(P1, 14);
    }
    let [mine, theirs] = d.setup.players;
    if mine.graveyard.is_empty() {
        d = d.graveyard(P0, &[STOCK_UP, REPRIEVE]);
    }
    if theirs.graveyard.is_empty() {
        d = d.graveyard(P1, &[OUST, MANA_DRAIN]);
    }
    if theirs.battlefield.is_empty() {
        d = d.battlefield(P1, THEIRS);
    }
    if theirs.hand.is_empty() {
        d = d.hand(P1, THEIR_HAND);
    }
    d
}

pub(super) fn flipped(action: &Action) -> Option<(Action, &'static str)> {
    match *action {
        Action::Answer {
            player,
            kind: DecisionKind::YesNo,
            pick,
        } => match pick {
            Pick::Yes => Some((Action::answer(player, DecisionKind::YesNo, Pick::No), "No")),
            Pick::No => Some((
                Action::answer(player, DecisionKind::YesNo, Pick::Yes),
                "Yes",
            )),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn from_scenario(def: &'static CardDef, scenario: &Scenario, out: &mut Vec<Candidate>) {
    let board = dressed(def, scenario);
    let group = heading(def, scenario);
    let actions = playable(scenario);
    let card = slug(def.name);
    let full = slug(scenario.name);
    let short = full
        .strip_prefix(&card)
        .map_or(full.as_str(), |s| s.trim_start_matches('-'))
        .to_owned();
    let at = |name: String, title: String, setup: &[Action], play: Vec<Action>, kind| Candidate {
        name,
        title,
        board: board.actions(leak(setup.to_vec())),
        cause: cause_of(setup)
            .filter(|_| play.is_empty())
            .or(cause_of(&play)),
        play,
        kind,
        group,
    };
    out.push(at(
        short.clone(),
        "Start".to_owned(),
        &[],
        actions.clone(),
        Kind::Whole,
    ));
    for (i, action) in actions.iter().enumerate() {
        let setup = &actions[..i];
        let mine = label_of(def, action);
        if matches!(*action,
            Action::Cast { player: P0, card, .. }
            | Action::PlayLand { player: P0, card }
            | Action::Activate { player: P0, card, .. } if card == def.name)
            && ways_to_play(def)
        {
            out.push(at(
                format!("{short}-ways-{i}"),
                "Ways to play".to_owned(),
                setup,
                Vec::new(),
                Kind::Ways,
            ));
        }
        if let Some(what) = mine.clone() {
            out.push(at(
                format!("{short}-{i}"),
                what,
                setup,
                vec![*action],
                Kind::Play,
            ));
        }
        if let Action::Activate {
            player: P0,
            card,
            ability: ObjectAbility::Own(_),
        } = *action
            && card == def.name
            && !def.front().types.intersects(Types::LAND)
        {
            let play = bounce(def, &actions, i);
            out.push(Candidate {
                board: with_a_bounce(&board).actions(leak(setup.to_vec())),
                ..at(
                    format!("{short}-bounced-{i}"),
                    format!("{}, bounced", mine.clone().unwrap_or_default()),
                    setup,
                    play,
                    Kind::Bounced,
                )
            });
        }
        if let Action::Answer { kind, .. } = *action
            && kind != DecisionKind::Priority
        {
            let ask = out.len();
            out.push(at(
                format!("{short}-ask-{i}"),
                String::new(),
                setup,
                Vec::new(),
                Kind::Ask,
            ));
            if let Some((flip, answer)) = flipped(action) {
                let mut c = at(
                    format!("{short}-{}-{i}", answer.to_lowercase()),
                    format!("Answer {answer}"),
                    setup,
                    vec![flip],
                    Kind::Flip(ask),
                );
                c.cause = cause_of(setup);
                out.push(c);
            }
        }
    }
}

/// Whether a card in hand offers more than one way to play it: an alternative or additional
/// cost ([CR 118.9](https://yawgatog.com/resources/magic-rules/#R1189): "Some spells have
/// alternative costs ... An alternative cost is a cost listed in a spell's text, or applied to
/// it from another effect, that its controller may pay rather than paying the spell's mana
/// cost"), an ability activated from the hand by discarding it (cycling, CR 702.29a), or a
/// second castable face.
fn ways_to_play(def: &'static CardDef) -> bool {
    let front = def.front();
    !front.casting.is_empty()
        || front
            .abilities
            .iter()
            .any(|a| a.kind == AbilityKind::Activated && a.cost.discard_self)
        || def.faces.len() > 1 && !def.faces.iter().skip(1).all(|f| f.transformed)
}

/// A scenario's heading: its name, less the card's name it starts with ("Exiles its own
/// loot's discard for a Rogue", "Ability returns a card after the Converter left").
pub(in crate::specs) fn heading(def: &'static CardDef, scenario: &Scenario) -> &'static str {
    let name = scenario.name;
    let rest = name.strip_prefix(def.name).map_or(name, |r| {
        r.strip_prefix("'s ")
            .or_else(|| r.strip_prefix(": "))
            .or_else(|| r.strip_prefix(' '))
            .unwrap_or(name)
    });
    let mut chars = rest.chars();
    let heading = chars
        .next()
        .map(|c| c.to_uppercase().chain(chars).collect::<String>())
        .unwrap_or_default();
    Box::leak(heading.into_boxed_str())
}

/// What a scenario's action of the card is, as its moment's label: "Cast", "{T}", "−3".
pub(in crate::specs) fn label_of(def: &'static CardDef, action: &Action) -> Option<String> {
    match *action {
        Action::Cast { card, .. } if card == def.name => Some("Cast".to_owned()),
        Action::PlayLand { card, .. } if card == def.name => Some("Play".to_owned()),
        Action::TurnFaceUp { card, .. } if card == def.name => Some("Turn up".to_owned()),
        Action::Activate {
            card,
            ability: ObjectAbility::Own(k),
            ..
        } if card == def.name => Some(
            def.front()
                .abilities
                .get(k.index())
                .map_or_else(|| format!("Ability {k}"), |a| cost_label(a, k.get())),
        ),
        _ => None,
    }
}

/// The activation at `i` with its own answers (its targets, its payment), then the opponent
/// bouncing the card with Sink into Stupor in response.
fn bounce(def: &'static CardDef, actions: &[Action], i: usize) -> Vec<Action> {
    let own = actions[i + 1..]
        .iter()
        .take_while(|a| {
            matches!(a, Action::Answer { player: P0, kind, .. } if *kind != DecisionKind::Priority)
        })
        .copied();
    std::iter::once(actions[i])
        .chain(own)
        .chain([
            Action::cast(P1, SINK_INTO_STUPOR),
            Action::choose(P1, DecisionKind::Target, def.name),
        ])
        .collect()
}

/// The board with Sink into Stupor in the opponent's hand and three Islands to cast it.
fn with_a_bounce(board: &Scenario) -> Scenario {
    let theirs = board.setup.players[P1.index()];
    let hand = leak([&[SINK_INTO_STUPOR][..], theirs.hand].concat());
    let lands = leak(
        [
            theirs.battlefield,
            &[
                Permanent::new(ISLAND),
                Permanent::new(ISLAND),
                Permanent::new(ISLAND),
            ][..],
        ]
        .concat(),
    );
    board.hand(P1, hand).battlefield(P1, lands)
}
