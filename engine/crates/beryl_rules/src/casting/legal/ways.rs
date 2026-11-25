//! Casting permissions and other ways a player can cast a card at priority.
//!
//! Implements: CR 702.102a

use beryl_kernel::reference::Graveyard;

use crate::bitboard::Bitboard;
use crate::card::{CardDef, CastingOption, Castings, Face, FaceIx};
use crate::ids::Seat;
use crate::layout::Layout;
use crate::state::ZoneKey;
use crate::state::{State, Zone};
use crate::table::{Capability, GameTable};

use super::super::costs::{From, option_sets, x_range_start};
use super::super::graveyard;
use super::super::start;
use super::copies;

/// How `player` may cast one card now, apart from its face's own options: for free during a
/// resolution, under a flashback permission, under another permission or from their hand,
/// and with escape.
#[derive(Clone, Copy)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "Each flag describes a separate condition checked when offering a cast"
)]
pub(super) struct Ways {
    /// Without escape, and so with or without the face's own alternative cost: from a hand or
    /// under a permission, or during a resolution.
    pub(super) plain: bool,
    /// With an alternative cost no face lists: free, or flashback's. Then the face's own
    /// alternative costs can't be paid (see CR 118.9a).
    pub(super) alternative: bool,
    /// Generic mana the casting permission adds to this face's cost.
    pub(super) increase: u8,
    /// With the escape an effect grants a card in a graveyard ([`Castings::ESCAPE`]).
    pub(super) escape: bool,
    /// Casting it relies on a permanent's permission to cast from a graveyard once each turn
    /// ([`Ways::permitted`]).
    pub(super) graveyard: bool,
    /// The back needs a separate permission lookup when the front's permission came
    /// from resolving an Adventure (see [CR 716.3d]).
    ///
    /// [CR 716.3d]: https://yawgatog.com/resources/magic-rules/#R7163d
    pub(super) on_adventure: bool,
}

impl Ways {
    pub(super) fn of<L: Layout>(
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        slot: L::Slot,
        from: From,
    ) -> Ways {
        match from {
            From::Resolution { free } => Ways {
                plain: true,
                alternative: free,
                increase: 0,
                escape: false,
                graveyard: false,
                on_adventure: false,
            },
            From::Priority => {
                let permission = start::permission(s, player, slot, FaceIx::FRONT);
                // See CR 702.138a: escape "functions while the card with escape is in a player's
                // graveyard", and lets its owner cast it from there.
                let escape = s.reaches_elsewhere()
                    && s.object(slot).owner() == player
                    && has_escape(s, t, slot);
                let graveyard =
                    permission.is_none() && graveyard::relies_on_permission(s, t, player, slot);
                Ways {
                    plain: !escape
                        || graveyard
                        || permission.is_some()
                        || s.object(slot).zone() == Zone::Hand,
                    alternative: permission.is_some_and(|p| p.terms.is_flashback()),
                    increase: permission.map_or(0, |p| p.terms.increase),
                    escape,
                    graveyard,
                    on_adventure: permission.is_some_and(|p| p.terms.on_adventure),
                }
            }
        }
    }

    /// The permission from resolving the Adventure does not allow the Adventure face.
    #[cold]
    #[inline(never)]
    pub(super) fn back<L: Layout>(self, s: &State<L>, player: Seat, slot: L::Slot) -> Self {
        let permission = start::permission(s, player, slot, FaceIx::BACK);
        Self {
            plain: permission.is_some(),
            alternative: permission.is_some_and(|p| p.terms.is_flashback()),
            increase: permission.map_or(0, |p| p.terms.increase),
            ..self
        }
    }

    /// Whether a permanent's permission accepts casting `face` with `costs`, where casting it
    /// relies on one: with the least value of X it may be cast with. The value chosen later
    /// is held to the permission as X is announced.
    pub(super) fn permitted<L: Layout>(
        self,
        s: &State<L>,
        t: &GameTable,
        player: Seat,
        face: &Face,
        costs: Castings,
    ) -> bool {
        !self.graveyard || {
            let x = x_range_start(face, costs, false).unwrap_or(0);
            graveyard::permitter(s, t, player, face, x).is_some()
        }
    }

    /// The sets of casting options of `face` these ways allow: each set the face's own
    /// options allow, and with escape, each of those that pays none of the face's own
    /// alternative costs, with [`Castings::ESCAPE`].
    pub(super) fn option_sets(self, face: &Face) -> impl Iterator<Item = Castings> + '_ {
        let plain = self.plain.then(|| option_sets(face, self.alternative));
        let escape = self
            .escape
            .then(|| option_sets(face, true).map(Castings::with_escape));
        plain
            .into_iter()
            .flatten()
            .chain(escape.into_iter().flatten())
    }
}

/// Whether the card in `slot` is a card in a graveyard that an effect gives escape.
pub(super) fn has_escape<L: Layout>(s: &State<L>, t: &GameTable, slot: L::Slot) -> bool {
    s.card_in(s.object_ref(slot), Graveyard)
        .is_some_and(|card| s.escape_from(t, card).is_some())
}

/// A player announces fuse during casting. [CR 702.102a]: "If a player casts a split card
/// with fuse from their hand, the player may choose to cast both halves of that split card
/// rather than choose one half."
/// The source zone, rather than the way a cast is paid for, determines whether fuse applies.
///
/// [CR 702.102a]: https://yawgatog.com/resources/magic-rules/#R702102a
pub(super) fn can_fuse<L: Layout>(
    s: &State<L>,
    def: &CardDef,
    player: Seat,
    slot: L::Slot,
) -> bool {
    def.shape.has_fuse() && {
        let object = s.object(slot);
        object.zone() == Zone::Hand
            && object.owner() == player
            && matches!(def.face(FaceIx::FUSED).casting, [CastingOption::Fuse])
    }
}

/// The cards `player` can cast at priority: their hand, the cards a permission names, the
/// cards in their graveyard that have escape, and while they control a permanent that lets
/// them cast from their graveyard, the cards there. Their graveyard is looked at only while an
/// effect reaches cards off the battlefield, which a game without one tests once.
#[inline]
pub(super) fn cast_sources<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Bitboard<L::Slot> {
    let mut cards = s.zone(ZoneKey::Of(player, Zone::Hand));
    for p in s.cast_permissions() {
        if p.terms.player == player {
            cards.insert(p.card.slot);
        }
    }
    if s.reaches_elsewhere() {
        cards |= escaping(s, t, player);
    }
    if t.can(Capability::CastsFromGraveyard) {
        cards |= graveyard::sources(s, t, player);
    }
    cards
}

/// The cards in `player`'s graveyard that have escape. Out of line: only while an effect
/// reaches cards off the battlefield.
#[cold]
#[inline(never)]
fn escaping<L: Layout>(s: &State<L>, t: &GameTable, player: Seat) -> Bitboard<L::Slot> {
    s.zone(ZoneKey::Of(player, Zone::Graveyard))
        .filter(|slot| has_escape(s, t, slot))
}

/// The cards in `player`'s graveyard that they may cast from there, whatever the step and
/// whether they could pay: each card a permission names, such as flashback an effect granted;
/// each card with escape; and each card a permanent's unused permission accepts a face of.
/// The table shows these cards in their player's hand while they may be cast. Only the
/// player's own permissions count: a card in a graveyard is cast only where a rule or an
/// effect allows it (see [CR 601.3]): "A player can begin to cast a spell only if a rule or
/// effect allows that player to cast it". Flashback lets its owner cast the card "from your
/// graveyard" (see [CR 702.34a]), and escape "functions while the card with escape is in a
/// player's graveyard" (see [CR 702.138a]).
///
/// [CR 601.3]: https://yawgatog.com/resources/magic-rules/#R6013
/// [CR 702.34a]: https://yawgatog.com/resources/magic-rules/#R70234a
/// [CR 702.138a]: https://yawgatog.com/resources/magic-rules/#R702138a
pub fn castable_from_graveyard<L: Layout>(
    s: &State<L>,
    t: &GameTable,
    player: Seat,
) -> Bitboard<L::Slot> {
    let graveyard = s.zone(ZoneKey::Of(player, Zone::Graveyard));
    let mut cards = Bitboard::EMPTY;
    for p in s.cast_permissions() {
        if p.terms.player == player && graveyard.contains(p.card.slot) {
            cards.insert(p.card.slot);
        }
    }
    if s.reaches_elsewhere() {
        cards |= escaping(s, t, player);
    }
    if t.can(Capability::CastsFromGraveyard) {
        cards |= graveyard::sources(s, t, player).filter(|slot| {
            copies::castable_def(s, t, slot, From::Priority).is_some_and(|def| {
                def.face_ixs().any(|f| {
                    let face = def.face(f);
                    face.playable() && graveyard::permitter(s, t, player, face, 0).is_some()
                })
            })
        });
    }
    cards
}
