//! Card scenarios played for both seats' scripts: small scripted positions
//! played through the engine, each seat's views fed to its screen as the session feeds them.

use beryl_cards::CARDS;
use beryl_engine::event::Tagged;
use beryl_engine::{Observation, Seat};
use beryl_session::StopsSetting;
use beryl_session::protocol::{Moment, PlayerView, heard, player_view};
use beryl_session::script::{Heard, Names, Screen};

/// Each seat's views as `card`'s scenario named `name` plays, the first player's and then the
/// second's, each with its script: the first is the reset its screen starts from, and view
/// `n` comes with what action `n` of the scenario did, with the forced answers after it.
/// Each view is the one a player waiting on the scenario's next answer has: it says which
/// attackers await what they attack and which cards the opponent is handling, and no more of
/// the decision. `tweak` may change view `n` of seat 0 or 1 before its screen sees it.
pub fn views(
    card: &str,
    name: &str,
    tweak: impl FnMut(usize, u8, &mut PlayerView),
) -> Vec<[PlayerView; 2]> {
    let heard = views_heard(card, name, tweak);
    heard
        .into_iter()
        .map(|seats| seats.map(|(view, _)| view))
        .collect()
}

/// [`views`], each with what the engine said of its events and its stack: what its screen
/// built its script from.
pub fn views_heard(
    card: &str,
    name: &str,
    mut tweak: impl FnMut(usize, u8, &mut PlayerView),
) -> Vec<[(PlayerView, Heard); 2]> {
    let (_, scenarios) = beryl_cards::scenarios()
        .iter()
        .find(|&&(c, _)| c == card)
        .expect("the card's scenarios");
    let scenario = scenarios
        .iter()
        .find(|s| s.name == name)
        .expect("the scenario");
    let (table, _) = beryl_scenario::position(&CARDS, scenario).unwrap_or_else(|e| panic!("{e}"));
    // The decision open in each state an action answers.
    let mut open = Vec::new();
    beryl_scenario::visit(&CARDS, scenario, |_, state, decision| {
        open.push((*state, *decision));
    })
    .unwrap_or_else(|e| panic!("{e:?}"));
    let mut screens: [Screen; 2] = Default::default();
    // The stack by the names both seats' scripts call its entries, as a session follows it.
    let mut names = Names::default();
    let mut views = Vec::new();
    beryl_scenario::watch_tagged(&CARDS, scenario, |state, events, tags| {
        let decision = open
            .iter()
            .find(|(asked, _)| asked == state)
            .map(|(_, d)| d);
        let n = views.len();
        let tagged: Vec<Tagged> = (events.iter().zip(tags))
            .map(|(event, &tag)| Tagged { event: *event, tag })
            .collect();
        let said = names.follow(&tagged);
        names.agree(state.stack());
        views.push([0, 1].map(|seat| {
            let obs = Observation::new(state, Seat::new(seat));
            let mut view = player_view(
                &table,
                obs,
                Moment::Thinking {
                    resolving: None,
                    decision,
                },
                StopsSetting::SMART,
                events,
            );
            tweak(n, seat, &mut view);
            let heard = heard(&table, obs, events, &said, names.stack().to_vec());
            view.script = Some(screens[usize::from(seat)].script(&view, &heard));
            (view, heard)
        }));
    })
    .unwrap_or_else(|e| panic!("{e:?}"));
    views
}

/// Each seat's scripts of a scenario, as text ([`views`]).
pub fn scripts(card: &str, name: &str) -> Vec<[String; 2]> {
    let text = |view: PlayerView| view.script.expect("a view's script").to_string();
    let views = views(card, name, |_, _, _| {});
    views.into_iter().map(|seats| seats.map(text)).collect()
}

/// Script `n` of `card`'s scenario `name`, for the player who goes first and for the other.
pub fn script(card: &str, name: &str, n: usize) -> [String; 2] {
    scripts(card, name).swap_remove(n)
}

/// A script's text from its lines.
pub fn text<S: AsRef<str>>(lines: &[S]) -> String {
    let mut text = String::new();
    for line in lines {
        text.push_str(line.as_ref());
        text.push('\n');
    }
    text
}
