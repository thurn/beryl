//! How a save names an object ([`Obj`]) and its zone ([`ZoneName`]).

use beryl_engine::card::{FaceIx, Types};
use beryl_engine::state::Flag;
use beryl_engine::{GameTable, Layout, State, Zone};
use serde::{Deserialize, Serialize};

use super::{is_false, is_zero};
use crate::checkpoint::{Counter, Exception};

/// An object, by what the rules see of it: its name, zone, controller and owner, whether it
/// is tapped, and for a permanent, since version 6, whether it is a token, summoning sick,
/// its counters, the damage marked on it and its place in combat. Copies of a card alike in
/// all of these are alike to the rules for one answer; [`Described::nth`] tells them apart
/// for the answers after it.
#[allow(
    clippy::struct_excessive_bools,
    reason = "an object's statuses, each yes or no in the rules"
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Obj {
    pub name: String,
    pub zone: ZoneName,
    pub controller: u8,
    /// Its owner, where that is not its controller, since version 5. Two copies of a card one
    /// player controls, one of them another player's, part ways where the rules send a card
    /// to its owner's zone ([CR 108.3](https://yawgatog.com/resources/magic-rules/#R1083)):
    /// "The owner of a card in the game is the player who started the game with it in their
    /// deck."
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<u8>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub tapped: bool,
    /// A card in its owner's hand or library that the other player knows, since version 5.
    /// Playing a copy they have seen tells them nothing; playing one they haven't leaves them
    /// still not knowing the other, in a hidden zone
    /// ([CR 400.2](https://yawgatog.com/resources/magic-rules/#R4002)).
    #[serde(default, skip_serializing_if = "is_false")]
    pub shown: bool,
    /// Named by a version 4 save or older, which names neither the owner nor whether the other
    /// player knows it: replay takes an object either way, as version 4 did.
    #[serde(default, rename = "beforeV5", skip_serializing_if = "is_false")]
    pub before_v5: bool,
    /// A token, since version 6: a token copy of a card is a permanent apart from the card
    /// ([CR 111.1](https://yawgatog.com/resources/magic-rules/#R1111)). Since version 13 also
    /// a copy of a spell on the stack, which is a token ([CR 707.10]): the copy and its spell
    /// differ to what counters one.
    ///
    /// [CR 707.10]: https://yawgatog.com/resources/magic-rules/#R70710
    #[serde(default, skip_serializing_if = "is_false")]
    pub token: bool,
    /// The card it really is, for a card that has another object's values under a copy
    /// effect, since version 13: a Phantasmal Image that entered as a copy of Grizzly Bears is
    /// named "Grizzly Bears" ([CR 707.2]) but is not a Grizzly Bears to the rules that send a
    /// card somewhere, so the two part ways where it leaves the battlefield.
    ///
    /// [CR 707.2]: https://yawgatog.com/resources/magic-rules/#R7072
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<String>,
    /// The copy exceptions its copiable values carry, in the order they were made, since
    /// version 13 ([CR 707.9b]): "The final set of values for that characteristic becomes part
    /// of the copiable values of the copy." A Phantasmal Image copying Grizzly Bears is an
    /// Illusion with a sacrifice trigger; one copying that Image has the trigger twice.
    ///
    /// [CR 707.9b]: https://yawgatog.com/resources/magic-rules/#R7079b
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excepts: Vec<Exception>,
    /// A creature that cannot yet attack or {T} ([CR 302.6](https://yawgatog.com/resources/magic-rules/#R3026)):
    /// "A creature's activated ability with the tap symbol or the untap symbol in its
    /// activation cost can't be activated unless the creature has been under its controller's
    /// control continuously since their most recent turn began." Since version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub sick: bool,
    /// A permanent's counters, since version 6.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub counters: Vec<Counter>,
    /// The damage marked on a permanent, since version 6.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub damage: u8,
    /// An attacking creature ([CR 506.4](https://yawgatog.com/resources/magic-rules/#R5064)),
    /// since version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub attacking: bool,
    /// A blocking creature, since version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub blocking: bool,
    /// An attacking creature that was blocked, which stays blocked though its blockers leave
    /// combat ([CR 509.1h](https://yawgatog.com/resources/magic-rules/#R5091h)), since
    /// version 6.
    #[serde(default, skip_serializing_if = "is_false")]
    pub blocked: bool,
    /// The planeswalker an attacking creature attacks, or the creature a blocking one blocks,
    /// by name, since version 6: two attackers alike but for whom they attack differ to the
    /// defender ([CR 506.3](https://yawgatog.com/resources/magic-rules/#R5063)).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub against: Option<String>,
    /// Named by a version 5 save or older, which names none of a permanent's token, summoning
    /// sickness, counters, damage and place in combat: replay takes an object whatever they
    /// are.
    #[serde(default, rename = "beforeV6", skip_serializing_if = "is_false")]
    pub before_v6: bool,
    /// Named by a version 12 save or older, which names neither the card a copy really is nor
    /// its copy exceptions, and no spell copy as a token: replay takes an object whatever they
    /// are.
    #[serde(default, rename = "beforeV13", skip_serializing_if = "is_false")]
    pub before_v13: bool,
}

impl Obj {
    pub(super) fn of<L: Layout>(t: &GameTable, s: &State<L>, slot: L::Slot, legacy: bool) -> Obj {
        let o = s.object(slot);
        let controller = s.controller(slot);
        let permanent = o.zone() == Zone::Battlefield;
        let public_name = |slot| {
            if !legacy && s.object(slot).face() == FaceIx::DOWN {
                "Face-down card"
            } else {
                s.name(t, slot)
            }
        };
        let hidden = !legacy && o.face() == FaceIx::DOWN;
        Obj {
            name: public_name(slot).to_owned(),
            zone: o.zone().into(),
            controller: controller.get(),
            owner: (o.owner() != controller).then_some(o.owner().get()),
            tapped: o.is_tapped(),
            shown: matches!(o.zone(), Zone::Hand | Zone::Library)
                && s.opponents_of(o.owner()).any(|p| s.knows(p, slot)),
            before_v5: false,
            token: (permanent || o.zone() == Zone::Stack) && o.is_token(),
            card: (!hidden && !o.is_token() && o.flags().has(Flag::Copy))
                .then(|| s.definition_name(t, slot).to_owned()),
            excepts: if !hidden && (permanent || o.zone() == Zone::Stack) {
                Exception::of(t, s, slot)
            } else {
                Vec::new()
            },
            sick: permanent
                && o.is_summoning_sick()
                && s.derived(slot).types.contains(Types::CREATURE),
            counters: if permanent {
                Counter::on(s, slot)
            } else {
                Vec::new()
            },
            damage: if permanent { o.damage() } else { 0 },
            attacking: permanent && o.is_attacking(),
            blocking: permanent && o.is_blocking(),
            blocked: permanent && o.is_blocked(),
            against: o
                .attacked_planeswalker()
                .or(o.blocked_attackers().first())
                .filter(|&other| permanent && s.object(other).zone() == Zone::Battlefield)
                .map(|other| public_name(other).to_owned()),
            before_v6: false,
            before_v13: false,
        }
    }

    /// This object as a version before 5, if `before_v5`, before 6, if `before_v6`, or before
    /// 13, if `before_v13`, named it.
    pub(super) fn name_like(&mut self, before_v5: bool, before_v6: bool, before_v13: bool) {
        if before_v13 {
            self.card = None;
            self.excepts.clear();
            if self.zone == ZoneName::Stack {
                self.token = false;
            }
            self.before_v13 = true;
        }
        if before_v5 {
            self.owner = None;
            self.shown = false;
            self.before_v5 = true;
        }
        if before_v6 {
            self.token = false;
            self.sick = false;
            self.counters.clear();
            self.damage = 0;
            self.attacking = false;
            self.blocking = false;
            self.blocked = false;
            self.against = None;
            self.before_v6 = true;
        }
    }
}

/// A [`Zone`], under names a save keeps.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ZoneName {
    Library,
    Hand,
    Battlefield,
    Graveyard,
    Exile,
    Stack,
    Nowhere,
    /// Part of a merged permanent (CR 731.2).
    Merged,
}

impl From<Zone> for ZoneName {
    fn from(zone: Zone) -> ZoneName {
        match zone {
            Zone::Library => ZoneName::Library,
            Zone::Hand => ZoneName::Hand,
            Zone::Battlefield => ZoneName::Battlefield,
            Zone::Graveyard => ZoneName::Graveyard,
            Zone::Exile => ZoneName::Exile,
            Zone::Stack => ZoneName::Stack,
            Zone::Nowhere => ZoneName::Nowhere,
            Zone::Merged => ZoneName::Merged,
        }
    }
}
