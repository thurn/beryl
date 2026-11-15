//! Builders for the views and scripts that tests and tools hand the client core, so that a test
//! names only what it is about. Only a driver of the client needs them (the `driver` feature).

use beryl_session::GameUpdate;
use beryl_session::protocol::{
    CardType, Declaration, FaceName, Facing, ManaPool, ObjectId, ObjectView, PlayerState,
    PlayerView, SessionId, Side, StackItem, ZoneName,
};
use beryl_session::script::{Act, Curve, Reset, Script, Step, StepTiming};

/// Builds a `PlayerView` one card at a time. Each card takes the next slot.
#[derive(Clone, Debug)]
pub struct ViewBuilder {
    view: PlayerView,
}

impl Default for ViewBuilder {
    fn default() -> Self {
        Self {
            view: PlayerView::neutral(player(), player()),
        }
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
            builder
                .card(ZoneName::Battlefield, side, "Serra Angel")
                .types = vec![CardType::Creature];
            builder.card(ZoneName::Battlefield, side, "Pacifism").types =
                vec![CardType::Enchantment];
            builder.card(ZoneName::Graveyard, side, "Brainstorm");
            builder.card(ZoneName::Exile, side, "Swords to Plowshares");
        }
        builder.card(ZoneName::Stack, Side::You, "Lightning Bolt");
        builder
    }

    /// Adds a card named `name` to `side`'s `zone`, owned and controlled by that side, and
    /// returns it to be changed further. A library card's name is hidden.
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
            ZoneName::Stack => self.view.stack.push(StackItem::Spell {
                slot,
                targets: Vec::new(),
                x: 0,
                paid: Vec::new(),
            }),
            ZoneName::Hand | ZoneName::Exile | ZoneName::Nowhere | ZoneName::Sideboard => {}
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
                beats: Vec::new(),
            },
        }
    }

    /// Adds a beat whose steps are `acts`, each with no cause.
    pub fn beat(&mut self, acts: impl IntoIterator<Item = Act>) -> &mut Self {
        self.script.beats.push(
            acts.into_iter()
                .map(|act| Step { act, cause: None })
                .collect(),
        );
        self
    }

    #[must_use]
    pub fn build(&self) -> Script {
        self.script.clone()
    }
}

/// A player at 20 life with nothing in their zones.
fn player() -> PlayerState {
    PlayerState {
        life: 20,
        mana: ManaPool::default(),
        lands_played: 0,
        mulligans: 0,
        mulligan: Declaration::Kept,
        library: Vec::new(),
        graveyard: Vec::new(),
        emblems: Vec::new(),
        named_counters: Vec::new(),
    }
}

/// A plain face-up card with nothing on it.
#[must_use]
pub fn object(slot: ObjectId, zone: ZoneName, side: Side, name: Option<&str>) -> ObjectView {
    ObjectView {
        slot,
        zone,
        owner: side,
        zone_side: None,
        controller: side,
        name: name.map(str::to_owned),
        facing: Facing::Up,
        token: false,
        tapped: false,
        summoning_sick: false,
        damage: 0,
        counters: Vec::new(),
        types: Vec::new(),
        pt: None,
        keywords: Vec::new(),
        attached_to: None,
        exiled_by: None,
        exiled_with: None,
        chosen: None,
        text: None,
        type_line: None,
        combat: None,
        note: None,
        pinned: None,
        face: FaceName::Front,
        named_counters: Vec::new(),
        doesnt_untap: false,
    }
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
        effects: Vec::new(),
    }
}
