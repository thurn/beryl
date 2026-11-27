//! Builders for the views and scripts that tests and playback tools hand to clients.

use crate::GameUpdate;
use crate::prompt::{PartName, PromptButton, PromptView, Purpose, ShapeBody, Words};
use crate::protocol::{
    AssistsView, CardType, LinkState, ManualOrders, ObjectId, ObjectView, PlayerState, PlayerView,
    PowerToughness, SessionId, Side, StackItem, TablePhase, TableView, ZoneName,
};
use crate::script::{Act, Curve, Reset, Room, Script, Step, StepTiming};

pub use crate::performance_probe::{card, permanent};

/// Builds a `PlayerView` one card at a time. Each card takes the next slot.
#[derive(Clone, Debug)]
pub struct ViewBuilder {
    view: PlayerView,
}

impl Default for ViewBuilder {
    /// A view of a game in its first turn, after both players have kept their hands.
    fn default() -> Self {
        let mut view = PlayerView::neutral(PlayerState::neutral(20), PlayerState::neutral(20));
        view.turn.number = 1;
        view.turn.own_number = 1;
        view.turn.starting = Some(Side::You);
        // The stops a session starts a game with, which the client's default settings hold, so
        // that a test's first decision sends no stops.
        view.stops = crate::StopsSetting::SMART;
        Self { view }
    }
}

impl ViewBuilder {
    /// A view of an empty table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// A view with a few named cards in each zone of each side: a library, a hand, lands, a
    /// creature, an enchantment, a graveyard and an exile pile, and a spell of the viewer's on
    /// the stack.
    #[must_use]
    pub fn sample() -> Self {
        let mut builder = Self::new();
        for side in [Side::You, Side::Opponent] {
            for _ in 0..5 {
                builder.card(ZoneName::Library, side, "Island");
            }
            builder.card(ZoneName::Hand, side, "Counterspell");
            builder.card(ZoneName::Hand, side, "Plains");
            builder.card(ZoneName::Hand, side, "Memory Lapse");
            for name in ["Island", "Island", "Plains"] {
                builder.card(ZoneName::Battlefield, side, name).types = vec![CardType::Land];
            }
            let angel = builder.card(ZoneName::Battlefield, side, "Serra Angel");
            angel.types = vec![CardType::Creature];
            angel.pt = Some(PowerToughness {
                power: 4,
                toughness: 4,
            });
            builder.card(ZoneName::Battlefield, side, "Pacifism").types =
                vec![CardType::Enchantment];
            builder.card(ZoneName::Graveyard, side, "Brainstorm");
            builder.card(ZoneName::Exile, side, "Swords to Plowshares");
        }
        builder.card(ZoneName::Stack, Side::You, "Lightning Bolt");
        builder
    }

    /// Adds a card named `name` to `side`'s `zone`, owned and controlled by that side, and
    /// returns it to be changed further. Library cards and the opponent's hand cards are hidden.
    ///
    /// # Panics
    ///
    /// Panics if all 65,536 object slots are occupied.
    pub fn card(&mut self, zone: ZoneName, side: Side, name: &str) -> &mut ObjectView {
        let slot = ObjectId(u16::try_from(self.view.objects.len()).expect("fewer than 2^16 cards"));
        let player = match side {
            Side::You => &mut self.view.players.you,
            Side::Opponent => &mut self.view.players.opponent,
        };
        match zone {
            ZoneName::Library => player.library.push(slot),
            ZoneName::Graveyard => player.graveyard.push(slot),
            ZoneName::Battlefield => self.view.battlefield.push(slot),
            ZoneName::Stack => self.view.stack.push(StackItem::spell(slot)),
            ZoneName::Hand
            | ZoneName::Exile
            | ZoneName::Nowhere
            | ZoneName::Sideboard
            | ZoneName::Merged => {}
        }
        let hidden =
            zone == ZoneName::Library || (zone == ZoneName::Hand && side == Side::Opponent);
        self.view
            .objects
            .push(object(slot, zone, side, (!hidden).then_some(name)));
        self.view.objects.last_mut().expect("just pushed")
    }

    /// Sets the script the view arrives with.
    pub fn script(&mut self, script: Script) -> &mut Self {
        self.view.script = Some(script);
        self
    }

    #[must_use]
    pub fn build(&self) -> PlayerView {
        self.view.clone()
    }

    /// The view as the session sends it, in session 1.
    #[must_use]
    pub fn update(&self) -> GameUpdate {
        GameUpdate {
            session: SessionId(1),
            view: self.build(),
        }
    }
}

/// Builds a `Script` one beat at a time.
#[derive(Clone, Debug, Default)]
pub struct ScriptBuilder {
    script: Script,
}

impl ScriptBuilder {
    /// A script that starts the table over from the view, with no motion.
    #[must_use]
    pub fn reset() -> Self {
        Self {
            script: Script {
                reset: Some(Reset::default()),
                ..Script::empty()
            },
        }
    }

    /// Adds a beat whose steps are `acts`, each with no cause.
    pub fn beat(&mut self, acts: impl IntoIterator<Item = Act>) -> &mut Self {
        self.script.beats.push(acts.into_iter().map(step).collect());
        self
    }

    #[must_use]
    pub fn build(&self) -> Script {
        self.script.clone()
    }
}

/// A script step with no cause.
#[must_use]
pub const fn step(act: Act) -> Step {
    Step { act, cause: None }
}

/// A plain face-up card with nothing on it.
#[must_use]
pub fn object(slot: ObjectId, zone: ZoneName, side: Side, name: Option<&str>) -> ObjectView {
    ObjectView {
        name: name.map(str::to_owned),
        ..ObjectView::neutral(slot, zone, side)
    }
}

/// A prompt at revision zero for the pick part, with the given buttons in display order.
/// Counts, card context, marks, payment previews and client-managed wording start empty.
#[must_use]
pub fn prompt(
    purpose: Purpose,
    title: &str,
    buttons: Vec<PromptButton>,
    body: ShapeBody,
) -> PromptView {
    PromptView {
        revision: 0,
        purpose,
        part: PartName::Pick,
        title: title.to_owned(),
        subtitle: String::new(),
        words: Words::default(),
        count: None,
        about: Vec::new(),
        from: None,
        marks: Vec::new(),
        previews: Vec::new(),
        buttons,
        body,
    }
}

/// A script with these beats and timing, and no reset unless `reset` is given.
#[must_use]
pub fn script(reset: Option<Reset>, beats: Vec<Vec<Step>>, timing: Vec<StepTiming>) -> Script {
    let mut script = Script::empty();
    script.reset = reset;
    script.beats = beats;
    script.timing = timing;
    script
}

/// The timing of a step that plays for `duration` milliseconds as its turn comes, easing out, and
/// waits for nothing.
#[must_use]
pub fn timing(duration: f64) -> StepTiming {
    StepTiming {
        case: None,
        duration,
        delay: 0.0,
        stagger: 0.0,
        curve: Curve::EaseOut,
        each: None,
        with: false,
        after: Vec::new(),
        hold: None,
        leads: false,
        passes: false,
        alone: false,
        opens: false,
        ahead: false,
        closes: None,
        room: Room::Step,
        phases: Vec::new(),
        effects: Vec::new(),
    }
}

/// What a solo manual table adds to `view` for the viewer at seat 0: every assist off, the
/// first main phase, the order of each side's hand and exile as `view`'s objects stand, and a
/// reference to each object, named for its slot, in the zone it lies in, so that every card
/// can be acted on.
///
/// # Panics
///
/// Never: the view and references are built from values a table view holds.
#[must_use]
pub fn table(view: &PlayerView) -> TableView {
    let seat = |side: Side| u8::from(side == Side::Opponent);
    let mut orders = ManualOrders::default();
    for object in &view.objects {
        let piles = match object.owner {
            Side::You => &mut orders.you,
            Side::Opponent => &mut orders.opponent,
        };
        match object.zone {
            ZoneName::Hand => piles.hand.push(object.slot),
            ZoneName::Exile => piles.exile.push(object.slot),
            _ => {}
        }
    }
    let object_actions = (view.objects.iter())
        .map(|object| {
            let owner = seat(object.owner);
            let zone = match object.zone {
                ZoneName::Library => serde_json::json!({ "library": owner }),
                ZoneName::Hand => serde_json::json!({ "hand": owner }),
                ZoneName::Graveyard => serde_json::json!({ "graveyard": owner }),
                ZoneName::Exile => serde_json::json!({ "exile": owner }),
                ZoneName::Stack => serde_json::json!("stack"),
                _ => serde_json::json!("battlefield"),
            };
            serde_json::from_value(serde_json::json!({
                "subject": { "kind": "object", "slot": object.slot.0 },
                "reference": { "view": "hand-built", "handle": object.slot.0 },
                "zone": zone,
            }))
            .expect("a hand-built object action")
        })
        .collect();
    TableView {
        log: Vec::new(),
        proposals: Vec::new(),
        labels: Vec::new(),
        assists: AssistsView::default(),
        phase: TablePhase::FirstMain,
        seats: 1,
        seat: Some(0),
        orders,
        acting: Vec::new(),
        object_actions,
        card_commands: std::collections::BTreeMap::new(),
        top_revealed: false,
        top_known: false,
        manual_stats: Vec::new(),
        last_draw: None,
        deck_tokens: Vec::new(),
        prompt: None,
        choosing_first: false,
        arrows: Vec::new(),
        link: LinkState::Alone,
        peer_name: None,
        proposal_marks: None,
    }
}

/// A solo table's lobby built by hand, with the 40-card sample deck loaded if `loaded`, and the
/// turn upkeep assist on.
///
/// # Panics
///
/// Never: the lobby is built from values a lobby holds.
#[must_use]
pub fn lobby(loaded: bool) -> crate::ipc::LobbyUpdate {
    let (main, side) = if loaded { (40, 8) } else { (0, 0) };
    serde_json::from_value(serde_json::json!({
        "view": {
            "you": 0,
            "seats": [{
                "seat": 0, "name": "You", "loaded": loaded, "mainCount": main,
                "sideCount": side, "ready": false, "wins": 0,
            }],
            "main": [], "side": [], "unknown": [],
            "assists": ["turn_upkeep"],
            "settings": { "startingLife": 20, "startingHandSize": 7, "first": null },
            "gameNumber": 1, "lastWinner": null, "invite": null, "canStart": loaded,
        }
    }))
    .expect("a hand-built lobby")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_view_has_both_players_zones_and_a_spell() {
        let view = ViewBuilder::sample().build();
        assert_eq!(view.objects.len(), 31);
        assert_eq!(view.battlefield.len(), 10);
        assert_eq!(view.stack, [StackItem::spell(ObjectId(30))]);
        for player in [&view.players.you, &view.players.opponent] {
            assert_eq!(player.library.len(), 5);
            assert_eq!(player.graveyard.len(), 1);
        }
        let opponent_hand: Vec<_> = view
            .objects
            .iter()
            .filter(|object| object.zone == ZoneName::Hand && object.owner == Side::Opponent)
            .collect();
        assert_eq!(opponent_hand.len(), 3);
        assert!(opponent_hand.iter().all(|object| object.name.is_none()));
    }
}
