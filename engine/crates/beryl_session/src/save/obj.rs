//! How a save names an object ([`Obj`]) and its zone ([`ZoneName`]).

use beryl_engine::card::{FaceIx, Types};
use beryl_engine::state::{Flag, Memo};
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
    /// The face up, by its index in the card's definition, where it is not the front but
    /// has the front's name, since version 14: a prototyped Arcane Proxy keeps the card's
    /// name but is a 2/1 with mana value 3 ([CR 719.3b]). Any other face up that is not the
    /// front, as a transformed permanent's or an Adventure's, has a name of its own, which
    /// [`Obj::name`] already gives.
    ///
    /// [CR 719.3b]: https://yawgatog.com/resources/magic-rules/#R7193b
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub face: Option<u8>,
    /// A permanent's designations that change what it does, since version 14: a Class's
    /// level above 1, solved and prepared.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub designations: Vec<Designated>,
    /// Named by a version 13 save or older, which names neither the face up nor the
    /// designations: replay takes an object whatever they are.
    #[serde(default, rename = "beforeV14", skip_serializing_if = "is_false")]
    pub before_v14: bool,
}

/// A designation of a permanent, as a save names it ([`Obj::designations`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Designated {
    /// A Class's level, from 2 up ([CR 717.2d]): without a level it "is treated as though
    /// its level is 1."
    ///
    /// [CR 717.2d]: https://yawgatog.com/resources/magic-rules/#R7172d
    Level(u8),
    /// A solved Case ([CR 720.3b](https://yawgatog.com/resources/magic-rules/#R7203b)).
    Solved,
    /// A prepared creature, whose prepare spell's copy its controller may cast.
    Prepared,
}

impl Designated {
    /// The designations of the permanent in `slot`, in the order of [`Designated`].
    fn on<L: Layout>(s: &State<L>, slot: L::Slot) -> Vec<Designated> {
        let memos = s.memos(slot);
        let level = memos.iter().find_map(|memo| match memo {
            Memo::ClassLevel(level) => Some(Designated::Level(level)),
            _ => None,
        });
        let solved = memos
            .iter()
            .any(|memo| memo == Memo::Solved)
            .then_some(Designated::Solved);
        let prepared = memos
            .iter()
            .any(|memo| matches!(memo, Memo::Prepared(_)))
            .then_some(Designated::Prepared);
        [level, solved, prepared].into_iter().flatten().collect()
    }
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
            damage: if permanent {
                beryl_engine::UnsignedNumber::to_u8_saturating(o.damage())
            } else {
                0
            },
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
            face: (!hidden
                && o.face() != FaceIx::FRONT
                && s.card_def(t, slot)
                    .is_some_and(|def| def.face_name(o.face()) == def.face_name(FaceIx::FRONT)))
            .then(|| o.face().get()),
            designations: if permanent {
                Designated::on(s, slot)
            } else {
                Vec::new()
            },
            before_v14: false,
        }
    }

    /// This object as a version before 5, if `before_v5`, before 6, if `before_v6`, before
    /// 13, if `before_v13`, or before 14, if `before_v14`, named it.
    pub(super) fn name_like(&mut self, before: Before) {
        let Before {
            v5: before_v5,
            v6: before_v6,
            v13: before_v13,
            v14: before_v14,
        } = before;
        if before_v14 {
            self.face = None;
            self.designations.clear();
            self.before_v14 = true;
        }
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

/// Which versions before the one this build writes named an object ([`Obj::name_like`]).
#[allow(
    clippy::struct_excessive_bools,
    reason = "one yes or no for each version that named less"
)]
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct Before {
    pub v5: bool,
    pub v6: bool,
    pub v13: bool,
    pub v14: bool,
}

impl Before {
    /// The versions `o` was named before.
    pub(super) fn of(o: &Obj) -> Before {
        Before {
            v5: o.before_v5,
            v6: o.before_v6,
            v13: o.before_v13,
            v14: o.before_v14,
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
