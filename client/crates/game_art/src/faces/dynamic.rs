//! Faces registered during play for text-changed cards and custom-text stack abilities.
//!
//! A match's printed faces are built once from its card records. When a text-changing effect
//! alters a card's rules text or type line (`ObjectView::text` / `type_line`), or an ability on
//! the stack carries non-printed text (`StackItem::Ability::text`), these helpers append a
//! matching [`ListedFace`] to the list and bump [`Faces::number`] so the host prepares it.

use beryl_protocol::protocol::{ObjectView, PlayerView, StackItem};
use kit_ui::input::FaceId;

use super::{FaceList, Faces, ListedFace, Plate};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ChangedFace {
    base: FaceId,
    type_line: Option<String>,
    text: Option<String>,
    face: FaceId,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct CustomPlate {
    source: FaceId,
    triggered: bool,
    rules: String,
    face: FaceId,
}

impl FaceList {
    /// Registers dynamic faces for any text-changed object or custom-text stack ability in
    /// `view` not already in the list. Returns whether any face was added.
    pub fn sync_view(&mut self, view: &PlayerView) -> bool {
        let mut added = false;
        for object in &view.objects {
            if object.type_line.is_none() && object.text.is_none() {
                continue;
            }
            let Some(base) = self.find_printed_object(object) else {
                continue;
            };
            if self.find_changed_object(base, object).is_some() {
                continue;
            }
            let Some(id) = self.next_id() else {
                break;
            };
            let Some(printed) = self.faces.get(usize::from(base.0)).cloned() else {
                continue;
            };
            let mut text = printed.text;
            if let Some(type_line) = &object.type_line {
                text.type_line.clone_from(type_line);
            }
            if let Some(rules) = &object.text {
                text.rules = Some(rules.clone());
            }
            // Clear `image` so the asset worker renders Beryl's drawn frame with the updated
            // type line and rules text rather than the unmodified Scryfall scan.
            self.faces.push(ListedFace {
                image: String::new(),
                names: printed.names,
                colors: printed.colors,
                printed: printed.printed,
                text,
                plate: None,
            });
            self.changed_faces.push(ChangedFace {
                base,
                type_line: object.type_line.clone(),
                text: object.text.clone(),
                face: id,
            });
            added = true;
        }
        for item in view.stack_with_resolving() {
            let StackItem::Ability {
                name,
                triggered,
                text: Some(rules),
                ..
            } = item
            else {
                continue;
            };
            let Some(source) = self.find(name) else {
                continue;
            };
            if self.find_custom_plate(source, *triggered, rules).is_some() {
                continue;
            }
            let Some(id) = self.next_id() else {
                break;
            };
            let Some(printed) = self.faces.get(usize::from(source.0)).cloned() else {
                continue;
            };
            self.faces.push(ListedFace {
                image: printed.image,
                names: Vec::new(),
                colors: printed.colors,
                printed: None,
                text: printed.text,
                plate: Some(Plate {
                    source,
                    triggered: *triggered,
                    choice: false,
                    rules: rules.clone(),
                }),
            });
            self.custom_plates.push(CustomPlate {
                source,
                triggered: *triggered,
                rules: rules.clone(),
                face: id,
            });
            added = true;
        }
        added
    }

    /// Zero-allocation lookup of a previously registered text-changed face.
    pub(super) fn find_changed_object(&self, base: FaceId, object: &ObjectView) -> Option<FaceId> {
        self.changed_faces
            .iter()
            .find(|entry| {
                entry.base == base
                    && entry.type_line == object.type_line
                    && entry.text == object.text
            })
            .map(|entry| entry.face)
    }

    /// Zero-allocation lookup of a previously registered custom-text ability plate.
    pub(super) fn find_custom_plate(
        &self,
        source: FaceId,
        triggered: bool,
        rules: &str,
    ) -> Option<FaceId> {
        self.custom_plates
            .iter()
            .find(|entry| {
                entry.source == source && entry.triggered == triggered && entry.rules == rules
            })
            .map(|entry| entry.face)
    }

    fn next_id(&self) -> Option<FaceId> {
        u16::try_from(self.faces.len()).ok().map(FaceId)
    }
}

impl Faces {
    /// Registers any text-changed card face or custom-text ability plate in `view`. When new
    /// faces are added, resets layer state and increments [`Self::number`] so the host
    /// prepares the updated face array.
    pub fn sync_view(&mut self, view: &PlayerView) {
        if self.list.sync_view(view) {
            let count = self.list.faces.len();
            self.board.clear();
            self.board.resize(count, None);
            self.full.clear();
            self.full.resize(count, None);
            self.asked.clear();
            self.asked.resize(count, false);
            self.wanted.clear();
            self.shown_full.clear();
            self.number += 1;
        }
    }
}
