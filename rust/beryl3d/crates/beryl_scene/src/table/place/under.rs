//! The cards that lie under a permanent in its row, as the web interface tucks them
//! (`battlefieldPlacements` in `ui/src/game/battlefield.ts`):
//!
//! - the other cards of a merged permanent. A merge puts a card under a permanent or over it
//!   ([CR 731.2]): "To merge an object with a permanent, place that object on top of or under
//!   that permanent."
//! - the cards a permanent holds in exile: one exiled until the permanent leaves ([CR 610.3]),
//!   or one the permanent's abilities find "exiled with" it ([CR 607.2a]) (`held` in
//!   `ui/src/game/zoneViews.ts`).
//! - the Auras and Equipment attached to it ([CR 301.5]), whoever controls them, unless it is a
//!   land or is attached to something itself (`attachments` in `ui/src/game/zoneViews.ts`).
//!
//! A card under a permanent is in that permanent's row and on its side of the table, and is
//! in no group of its own.
//!
//! [CR 731.2]: https://yawgatog.com/resources/magic-rules/#R7312
//! [CR 610.3]: https://yawgatog.com/resources/magic-rules/#R6103
//! [CR 607.2a]: https://yawgatog.com/resources/magic-rules/#R6072a
//! [CR 301.5]: https://yawgatog.com/resources/magic-rules/#R3015

use beryl_protocol::protocol::{CardType, ObjectId, ObjectView, ZoneName};

use super::{Group, Zones};

/// The slot of the permanent `object` lies under, if it lies under one, finding objects by
/// slot with `find`. It is the first of the permanents `object` could lie under that is on the
/// battlefield and attached to nothing: for a card in the merged zone, its merged permanent;
/// for a card in exile, the permanent that exiled it until it leaves, and then the one whose
/// abilities find it "exiled with" it; for a permanent, the one it is attached to, if that is
/// not a land.
pub(crate) fn host_slot<'a>(
    object: &ObjectView,
    find: impl Fn(ObjectId) -> Option<&'a ObjectView>,
) -> Option<ObjectId> {
    let holders = match object.zone {
        ZoneName::Merged => [object.part_of, None],
        ZoneName::Exile => [object.exiled_by, object.exiled_with],
        ZoneName::Battlefield => [object.attached_to, None],
        _ => return None,
    };
    holders.into_iter().flatten().find(|&slot| {
        find(slot).is_some_and(|host| {
            host.zone == ZoneName::Battlefield
                && host.attached_to.is_none()
                && !(object.zone == ZoneName::Battlefield && host.types.contains(&CardType::Land))
        })
    })
}

/// Whether a card names any permanent it could lie under, even if that host is absent.
pub(crate) fn names_host(object: &ObjectView) -> bool {
    object.part_of.is_some()
        || object.exiled_by.is_some()
        || object.exiled_with.is_some()
        || object.attached_to.is_some()
}

impl<'a> Zones<'a> {
    /// Cards that may lie under a permanent, in the order the table lists them.
    fn possible_tucks(&self) -> impl Iterator<Item = &'a ObjectView> + '_ {
        self.tucked
            .into_iter()
            .flatten()
            .filter_map(|&slot| self.get(slot))
            .chain(
                self.tucked
                    .is_none()
                    .then(|| self.objects())
                    .into_iter()
                    .flatten(),
            )
    }

    /// The group `object` belongs in: `Group::of`, or for a card that lies under a permanent
    /// (`host`), that permanent's row.
    pub(crate) fn group_of(&self, object: &ObjectView) -> Option<Group> {
        Group::of(self.host(object).unwrap_or(object))
    }

    /// The permanent `object` lies under, if it lies under one (`host_slot`).
    pub(super) fn host(&self, object: &ObjectView) -> Option<&'a ObjectView> {
        host_slot(object, |slot| self.get(slot)).and_then(|slot| self.get(slot))
    }

    /// The cards that lie under the permanents among `permanents`: first the cards held in
    /// exile or merged, in the order the table lists them, then the attached permanents in
    /// battlefield order, as `all_members` lists them and the web interface tucks attachments
    /// (`attachments` in `ui/src/game/zoneViews.ts`). Slot order is not battlefield order: an
    /// Aura cast onto a creature after an Equipment was attached to it can have the lower slot.
    pub(super) fn under(&self, permanents: &[&ObjectView]) -> Vec<&'a ObjectView> {
        let mut under: Vec<&'a ObjectView> = self
            .possible_tucks()
            .filter(|object| {
                self.host(object)
                    .is_some_and(|host| permanents.iter().any(|p| p.slot == host.slot))
            })
            .collect();
        let attached = under
            .iter()
            .filter(|object| object.zone == ZoneName::Battlefield)
            .count();
        if attached > 1 {
            // Stable, so the held cards keep their order before the attached ones.
            under.sort_by_key(|object| {
                (object.zone == ZoneName::Battlefield).then(|| {
                    self.battlefield
                        .iter()
                        .position(|&slot| slot == object.slot)
                })
            });
        }
        under
    }

    /// The objects that name the permanent in `slot` as one they could lie under (`host_slot`),
    /// whether or not they do.
    pub(crate) fn naming_as_host(
        &self,
        slot: ObjectId,
    ) -> impl Iterator<Item = &'a ObjectView> + '_ {
        self.possible_tucks().filter(move |object| {
            [
                object.part_of,
                object.exiled_by,
                object.exiled_with,
                object.attached_to,
            ]
            .contains(&Some(slot))
        })
    }
}
