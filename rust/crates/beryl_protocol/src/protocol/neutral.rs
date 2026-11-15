//! Constructors for views that have no rules engine behind them.

use super::{
    Declaration, FaceName, Facing, ManaPool, ManualAbility, ObjectId, ObjectView, PlayerState,
    PlayerView, Players, Side, StackItem, Status, Step, StepSet, StopsSetting, TurnView, ZoneName,
};

impl PlayerView {
    /// An empty view for a live manual table. The caller supplies the public player states.
    pub fn neutral(you: PlayerState, opponent: PlayerState) -> Self {
        Self {
            status: Status::Free,
            handling: None,
            pending_attackers: None,
            turn: TurnView {
                number: 0,
                own_number: 0,
                active: Side::You,
                step: Step::Untap,
                priority: None,
                starting: None,
                flip_winner: None,
            },
            players: Players { you, opponent },
            battlefield: Vec::new(),
            objects: Vec::new(),
            stack: Vec::new(),
            stops: StopsSetting {
                full_control: false,
                yours: StepSet::default(),
                theirs: StepSet::default(),
            },
            events: Vec::new(),
            undone: false,
            script: None,
            table: None,
        }
    }
}

impl PlayerState {
    /// A player's life total with empty zones and no engine state.
    pub fn neutral(life: i32) -> Self {
        Self {
            life,
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
}

impl ObjectView {
    /// A card whose identity this viewer knows.
    pub fn card(
        slot: ObjectId,
        name: impl Into<String>,
        zone: ZoneName,
        controller: Side,
        owner: Side,
    ) -> Self {
        Self {
            name: Some(name.into()),
            ..Self::hidden(slot, zone, controller, owner)
        }
    }

    /// A card whose identity this viewer does not know.
    pub fn hidden(slot: ObjectId, zone: ZoneName, controller: Side, owner: Side) -> Self {
        Self {
            slot,
            zone,
            owner,
            zone_side: None,
            controller,
            name: None,
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
            custom_colors: None,
            token_printing: None,
            combat: None,
            note: None,
            pinned: None,
            face: FaceName::Front,
            named_counters: Vec::new(),
            doesnt_untap: false,
            hidden_pick: false,
        }
    }
}

/// What the viewer knows of a manual marker's source when it was placed.
pub struct MarkerPreview {
    pub name: String,
    pub face: FaceName,
    pub text: String,
}

impl StackItem {
    /// The stable number assigned by a manual table, if this is one of its markers.
    pub fn marker_id(&self) -> Option<u32> {
        match self {
            Self::Ability { manual, .. } => manual.map(|marker| marker.id),
            Self::Spell { .. } => None,
        }
    }

    /// A spell on the stack with no rules-engine casting choices.
    pub fn spell(slot: ObjectId) -> Self {
        Self::Spell {
            slot,
            targets: Vec::new(),
            x: 0,
            paid: Vec::new(),
        }
    }

    /// A manual marker. The adapter supplies an origin while the source is on the
    /// table and a captured name, face, and text only when the viewer knows them.
    pub fn ability(
        id: u32,
        source: Option<ObjectId>,
        preview: Option<MarkerPreview>,
        controller: Side,
    ) -> Self {
        let (name, face, text) = match preview {
            Some(MarkerPreview { name, face, text }) => (name, Some(face), text),
            None => (String::new(), None, String::new()),
        };
        Self::Ability {
            source: source.filter(|_| face.is_some()),
            ability: 0,
            granted: false,
            name,
            triggered: false,
            taps_to_pay: None,
            nth: 0,
            text: Some(text),
            controller,
            manual: Some(ManualAbility { id, face }),
            targets: Vec::new(),
            x: 0,
        }
    }
}
