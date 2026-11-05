//! What the session answers for the human under the smart stops: their priority while only
//! their own spells and abilities came and went on the stack, and the order of their
//! simultaneous triggers. Exact stops ask both, and replays find every such answer again.

use beryl_cards::CARDS;
use beryl_cards::names::{GET_LOST, ISLAND, MANA_DRAIN, PLAINS, THUNDERTRAP_TRAINER};
use beryl_engine::state::StackEntry;
use beryl_session::protocol::{Candidate, Kind, Side, StackItem, ZoneName};
use beryl_session::replay::Entry;
use beryl_session::{
    CandidateIndex, Game, PlayerView, Seat, SessionId, StartGame, Status, StopsSetting,
};

const SESSION: SessionId = SessionId(1);
const HUMAN: Seat = Seat::P0;
/// Long enough for many casts, short enough to stay quick.
const TURNS: u16 = 20;

fn deck(cards: &[(u8, &str)]) -> Vec<(u8, String)> {
    cards
        .iter()
        .map(|&(n, name)| (n, name.to_owned()))
        .collect()
}

/// Trainers to cast, with offspring once there is mana for it, and Mana Drains to hold.
fn trainers() -> Vec<(u8, String)> {
    deck(&[(24, ISLAND), (10, THUNDERTRAP_TRAINER), (6, MANA_DRAIN)])
}

/// Removal to answer the human's spells with, at instant speed.
fn get_lost() -> Vec<(u8, String)> {
    deck(&[(17, PLAINS), (23, GET_LOST)])
}

/// A deck that never casts anything: no lands, and nothing free.
fn no_plays() -> Vec<(u8, String)> {
    deck(&[(40, "Thieving Skydiver")])
}

fn new_game(seed: u64, opponent: Vec<(u8, String)>, stops: StopsSetting) -> Game {
    let start = StartGame {
        seed,
        decks: [trainers(), opponent],
        human: HUMAN,
        opponent: "random".to_owned(),
        stops,
    };
    Game::new(&CARDS, &start, SESSION).unwrap()
}

/// Whether a decision is asked by a Thundertrap Trainer's look, as it resolves.
fn looks(view: &PlayerView) -> bool {
    matches!(&view.status, Status::Deciding { decision }
        if matches!(&decision.resolving,
            Some(StackItem::Ability { name, .. }) if name == THUNDERTRAP_TRAINER))
}

fn kind(view: &PlayerView) -> Kind {
    let Status::Deciding { decision } = &view.status else {
        unreachable!("a decision's view")
    };
    decision.kind
}

/// The scripted human: plays a land, else casts a Trainer, with offspring if they can;
/// otherwise passes or takes the first candidate. They keep their Mana Drains, which give them
/// something to do whenever a spell is on the stack. Returns what it chose.
fn answer(game: &mut Game, view: &PlayerView) -> Candidate {
    let Status::Deciding { decision } = &view.status else {
        unreachable!("the human answers only their decisions");
    };
    let candidates = &decision.candidates;
    let find = |want: &dyn Fn(&Candidate) -> bool| candidates.iter().position(|c| want(&c.option));
    let index = match decision.kind {
        Kind::Priority if view.stack.is_empty() => {
            find(&|c| matches!(c, Candidate::PlayLand { .. })).or_else(|| {
                candidates
                    .iter()
                    .enumerate()
                    .filter_map(|(i, c)| match &c.option {
                        Candidate::Cast { costs, .. } => Some((costs.len(), i)),
                        _ => None,
                    })
                    .max()
                    .map(|(_, i)| i)
            })
        }
        _ => None,
    }
    .unwrap_or(0);
    let checked = game.answer(decision.id, CandidateIndex(index as u8));
    game.choose(checked.unwrap());
    candidates[index].option.clone()
}

/// One of the human's decisions: the view that asked it, and the stack as it stood then.
struct Asked {
    view: PlayerView,
    stack: Vec<StackEntry>,
    /// The stack once the human's previous answer was applied, if it was in this step.
    after_previous: Option<Vec<StackEntry>>,
    chosen: Candidate,
}

/// Plays the game to its end or into turn [`TURNS`]: every view, and every decision the
/// engine asked the human (the halts aside).
fn play(game: &mut Game) -> (Vec<PlayerView>, Vec<Asked>) {
    let mut views = Vec::new();
    let mut asked = Vec::new();
    let mut previous: Option<((u16, u8), Vec<StackEntry>)> = None;
    while let Some(view) = game.next_view() {
        views.push(view.clone());
        // A replay resumes where a view awaits someone, never as a step begins.
        if view.turn.number > TURNS && view.status != Status::Playing {
            break;
        }
        let Status::Deciding { decision } = &view.status else {
            continue;
        };
        if decision.halt {
            answer(game, &view);
            previous = None;
            continue;
        }
        let step = (view.turn.number, view.turn.step as u8);
        let stack = game.state().stack().to_vec();
        let after_previous = previous
            .take()
            .filter(|(at, _)| *at == step)
            .map(|(_, stack)| stack);
        let chosen = answer(game, &view);
        previous = Some((step, game.state().stack().to_vec()));
        asked.push(Asked {
            view,
            stack,
            after_previous,
            chosen,
        });
    }
    (views, asked)
}

fn priority_on_a_stack(a: &Asked) -> bool {
    kind(&a.view) == Kind::Priority && !a.stack.is_empty()
}

/// Whether the human's own spell or ability is on top, and only their own entries came onto
/// the stack or left it since their previous answer in this step.
fn only_own_doings(a: &Asked) -> bool {
    let Some(before) = &a.after_previous else {
        return false;
    };
    let own = |e: &StackEntry| e.controller == HUMAN;
    let kept = before
        .iter()
        .zip(&a.stack)
        .take_while(|(x, y)| x == y)
        .count();
    a.stack.last().is_some_and(own)
        && before[kept..].iter().all(own)
        && a.stack[kept..].iter().all(own)
}

/// The replay reproduces the game: cut at the end, it shows the last view.
fn assert_replays(game: &Game, views: &[PlayerView]) {
    let (_, again) = Game::from_replay(&CARDS, game.replay(), SESSION).unwrap();
    assert_eq!(&again, views.last().unwrap());
}

#[test]
fn a_humans_own_spell_resolves_without_their_priority_under_smart_stops_only() {
    let mut resolved = 0;
    let mut own_priorities = [0, 0];
    for seed in 0..3 {
        for (i, stops) in [StopsSetting::Smart, StopsSetting::Exact]
            .into_iter()
            .enumerate()
        {
            let mut game = new_game(seed, no_plays(), stops);
            let (views, asked) = play(&mut game);
            // The opponent never casts anything: every stack is the human's own.
            own_priorities[i] += asked.iter().filter(|a| priority_on_a_stack(a)).count();
            resolved += asked.iter().filter(|a| looks(&a.view)).count();
            assert_replays(&game, &views);
        }
    }
    assert!(resolved > 6, "only {resolved} Trainers resolved");
    assert_eq!(own_priorities[0], 0, "smart stops asked to pass to oneself");
    assert!(
        own_priorities[1] > 0,
        "exact stops never offered a response"
    );
}

#[test]
fn an_opponents_response_gives_the_human_priority() {
    let (mut responses, mut after_responses) = (0, 0);
    for seed in 0..8 {
        let mut game = new_game(seed, get_lost(), StopsSetting::Smart);
        let (views, asked) = play(&mut game);
        let theirs = |e: &StackEntry| e.controller != HUMAN;
        for a in asked.iter().filter(|a| priority_on_a_stack(a)) {
            assert!(
                !only_own_doings(a),
                "seed {seed}: asked to pass to their own spell at {:?}",
                a.view.turn
            );
            if a.stack.last().is_some_and(theirs) {
                responses += 1;
            } else if a
                .after_previous
                .as_ref()
                .is_some_and(|before| before.iter().any(|e| theirs(e) && !a.stack.contains(e)))
            {
                // The opponent's response left the stack, from above the human's spell.
                after_responses += 1;
            }
        }
        assert_replays(&game, &views);
    }
    assert!(responses > 0, "the opponent never responded");
    assert!(
        after_responses > 0,
        "no response left the stack above the human's spell"
    );
}

#[test]
fn smart_stops_order_the_humans_triggers_and_exact_stops_ask() {
    let mut orders_asked = [0, 0];
    let (mut offspring, mut autos) = (0, 0);
    for seed in 0..4 {
        for (i, stops) in [StopsSetting::Smart, StopsSetting::Exact]
            .into_iter()
            .enumerate()
        {
            let mut game = new_game(seed, no_plays(), stops);
            let (views, asked) = play(&mut game);
            orders_asked[i] += asked
                .iter()
                .filter(|a| kind(&a.view) == Kind::TriggerOrder)
                .count();
            if stops == StopsSetting::Smart {
                offspring += offspring_first(&asked);
                autos += game
                    .replay()
                    .log
                    .iter()
                    .filter(|e| matches!(e, Entry::Auto(_)))
                    .count();
            }
            assert_replays(&game, &views);
        }
    }
    assert_eq!(orders_asked[0], 0, "smart stops asked a trigger order");
    assert!(
        orders_asked[1] > 0,
        "exact stops never asked a trigger order"
    );
    assert!(offspring > 0, "no Trainer was cast with offspring");
    assert!(autos > offspring, "the session answered only {autos} times");
}

/// How many Trainers cast with offspring made their token before the first look that
/// followed: the heuristic resolves offspring's plain effect before a trigger that asks.
fn offspring_first(asked: &[Asked]) -> usize {
    let tokens = |v: &PlayerView| {
        v.objects
            .iter()
            .filter(|o| o.token && o.zone == ZoneName::Battlefield && o.controller == Side::You)
            .count()
    };
    let mut checked = 0;
    let mut cast: Option<usize> = None;
    for a in asked {
        if let Some(had) = cast
            && looks(&a.view)
        {
            assert_eq!(
                tokens(&a.view),
                had + 1,
                "a look came before offspring's token"
            );
            checked += 1;
            cast = None;
        }
        if matches!(&a.chosen, Candidate::Cast { costs, .. } if !costs.is_empty()) {
            cast = Some(tokens(&a.view));
        }
    }
    checked
}
