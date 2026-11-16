//! The cards under merged permanents, which lie in their permanent's row. A merge puts a card
//! under a permanent or over it ([CR 731.2]): "To merge an object with a permanent, place that
//! object on top of or under that permanent."
//!
//! [CR 731.2]: https://yawgatog.com/resources/magic-rules/#R7312

use beryl_protocol::protocol::{ObjectView, ZoneName};

use super::{Group, Zones};

impl<'a> Zones<'a> {
    /// The group `object` belongs in: `Group::of`, or for a card under a merged permanent, that
    /// permanent's row.
    pub(crate) fn group_of(&self, object: &ObjectView) -> Option<Group> {
        match object.zone {
            ZoneName::Merged => Group::of(self.host(object)?),
            _ => Group::of(object),
        }
    }

    /// The merged permanent on the battlefield that `object`, a card in the merged zone, lies
    /// under.
    fn host(&self, object: &ObjectView) -> Option<&'a ObjectView> {
        self.get(object.part_of?)
            .filter(|host| host.zone == ZoneName::Battlefield)
    }

    /// The cards under the merged permanents among `permanents`, in the order the table lists
    /// them.
    pub(super) fn merged_under(&self, permanents: &[&ObjectView]) -> Vec<&'a ObjectView> {
        self.objects()
            .filter(|object| {
                object.zone == ZoneName::Merged
                    && object
                        .part_of
                        .is_some_and(|host| permanents.iter().any(|p| p.slot == host))
            })
            .collect()
    }
}
